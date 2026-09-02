/* CPU copy timing while SPI2 DMA drains a product-sized internal SRAM slot. */

#include <inttypes.h>
#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>
#include <stdio.h>

#include "driver/spi_master.h"
#include "esp_attr.h"
#include "esp_chip_info.h"
#include "esp_err.h"
#include "esp_heap_caps.h"
#include "esp_rom_sys.h"
#include "esp_system.h"
#include "freertos/FreeRTOS.h"
#include "freertos/task.h"
#include "sdkconfig.h"

#define SCHEMA_VERSION "1.0.0"
#define HARNESS_VERSION "1.0.0"
#define TRIALS 100u
#define TRANSFER_PIXELS 16384u
#define TRANSFER_BYTES (TRANSFER_PIXELS * sizeof(uint16_t))
#define COPY_WORDS (TRANSFER_BYTES / sizeof(uint32_t))
#define PSRAM_SOURCE_COUNT 2u
#define SPI_CLOCK_HZ 40000000

_Static_assert(TRANSFER_BYTES == 32768u, "product transfer size changed");
_Static_assert(COPY_WORDS == 8192u, "32-bit copy operation count changed");

uint32_t dma_sram_copy_32k(uint32_t *destination, const uint32_t *source);

typedef struct {
  uint32_t *psram_sources;
  uint32_t *sram_source;
  uint32_t *copy_slot;
  uint32_t *dma_slot;
  spi_device_handle_t spi;
} probe_context_t;

static volatile bool dma_completed;
static volatile uint32_t benchmark_sink;

static inline uint32_t read_ccount(void) {
  uint32_t value;
  __asm__ __volatile__("rsr.ccount %0" : "=a"(value));
  return value;
}

static void IRAM_ATTR dma_post_callback(spi_transaction_t *transaction) {
  (void)transaction;
  dma_completed = true;
}

static void emit_metric(const char *name, const char *memory,
                        const char *pattern, uint32_t operations,
                        uint32_t bytes_per_operation,
                        const uint32_t samples[TRIALS], const char *baseline,
                        const bool *in_flight) {
  printf("CAL_RECORD {\"type\":\"metric\",\"name\":\"%s\","
         "\"memory\":\"%s\",\"access_pattern\":\"%s\","
         "\"operations_per_trial\":%" PRIu32 ","
         "\"bytes_per_operation\":%" PRIu32 ",\"ccount_samples\":[",
         name, memory, pattern, operations, bytes_per_operation);
  for (uint32_t index = 0; index < TRIALS; ++index) {
    printf("%s%" PRIu32, index == 0 ? "" : ",", samples[index]);
  }
  printf("],\"baseline\":%s", baseline == NULL ? "null" : baseline);
  if (in_flight != NULL) {
    printf(",\"dma_still_in_flight_samples\":[");
    for (uint32_t index = 0; index < TRIALS; ++index) {
      printf("%s%s", index == 0 ? "" : ",", in_flight[index] ? "true" : "false");
    }
    printf("]");
  }
  printf("}\n");
  fflush(stdout);
}

static void emit_refusal(const char *name, uint32_t ordinal,
                         const char *reason) {
  printf("CAL_RECORD {\"type\":\"refusal\",\"name\":\"%s\","
         "\"ordinal\":%" PRIu32 ",\"reason\":\"%s\","
         "\"tierCandidate\":\"distribution\"}\n",
         name, ordinal, reason);
  fflush(stdout);
}

static esp_err_t init_context(probe_context_t *context) {
  context->psram_sources = heap_caps_aligned_alloc(
      64, PSRAM_SOURCE_COUNT * TRANSFER_BYTES, MALLOC_CAP_SPIRAM);
  context->sram_source =
      heap_caps_aligned_alloc(64, TRANSFER_BYTES, MALLOC_CAP_INTERNAL);
  context->copy_slot = heap_caps_aligned_alloc(
      64, TRANSFER_BYTES, MALLOC_CAP_DMA | MALLOC_CAP_INTERNAL);
  context->dma_slot = heap_caps_aligned_alloc(
      64, TRANSFER_BYTES, MALLOC_CAP_DMA | MALLOC_CAP_INTERNAL);
  if (context->psram_sources == NULL || context->sram_source == NULL ||
      context->copy_slot == NULL || context->dma_slot == NULL) {
    return ESP_ERR_NO_MEM;
  }

  const size_t psram_words = PSRAM_SOURCE_COUNT * COPY_WORDS;
  for (size_t index = 0; index < psram_words; ++index) {
    context->psram_sources[index] = (uint32_t)index * 2246822519u + 31u;
  }
  for (size_t index = 0; index < COPY_WORDS; ++index) {
    context->sram_source[index] = (uint32_t)index * 3266489917u + 17u;
    context->copy_slot[index] = 0u;
    context->dma_slot[index] = (uint32_t)index * 668265263u + 7u;
  }

  spi_bus_config_t bus_config = {0};
  bus_config.sclk_io_num = 11;
  bus_config.data0_io_num = 4;
  bus_config.data1_io_num = 5;
  bus_config.data2_io_num = 6;
  bus_config.data3_io_num = 7;
  bus_config.max_transfer_sz = TRANSFER_BYTES;
  esp_err_t error =
      spi_bus_initialize(SPI2_HOST, &bus_config, SPI_DMA_CH_AUTO);
  if (error != ESP_OK) {
    return error;
  }

  spi_device_interface_config_t device_config = {0};
  device_config.clock_speed_hz = SPI_CLOCK_HZ;
  device_config.mode = 0;
  device_config.spics_io_num = -1;
  device_config.queue_size = 1;
  device_config.post_cb = dma_post_callback;
  device_config.flags = SPI_DEVICE_HALFDUPLEX;
  return spi_bus_add_device(SPI2_HOST, &device_config, &context->spi);
}

static void prepare_transaction(spi_transaction_t *transaction,
                                const probe_context_t *context) {
  *transaction = (spi_transaction_t){0};
  transaction->flags = SPI_TRANS_MODE_QIO;
  transaction->length = TRANSFER_BYTES * 8u;
  transaction->tx_buffer = context->dma_slot;
}

static esp_err_t queue_dma(probe_context_t *context,
                           spi_transaction_t *transaction) {
  dma_completed = false;
  return spi_device_queue_trans(context->spi, transaction, portMAX_DELAY);
}

static esp_err_t wait_dma(probe_context_t *context,
                          spi_transaction_t *transaction) {
  spi_transaction_t *completed = NULL;
  const esp_err_t error =
      spi_device_get_trans_result(context->spi, &completed, portMAX_DELAY);
  if (error != ESP_OK) {
    return error;
  }
  return completed == transaction ? ESP_OK : ESP_ERR_INVALID_RESPONSE;
}

static bool run_idle_copy(probe_context_t *context) {
  uint32_t samples[TRIALS];
  for (uint32_t ordinal = 0; ordinal < TRIALS; ++ordinal) {
    const uint32_t *source =
        context->psram_sources + (ordinal % PSRAM_SOURCE_COUNT) * COPY_WORDS;
    samples[ordinal] = dma_sram_copy_32k(context->copy_slot, source);
    benchmark_sink ^= context->copy_slot[(ordinal * 127u) % COPY_WORDS];
  }
  emit_metric("copy_psram_to_sram_idle", "psram-to-internal-sram",
              "plain_32bit_copy_dma_idle", COPY_WORDS, sizeof(uint32_t),
              samples, NULL, NULL);
  return true;
}

static bool run_active_copy(probe_context_t *context, bool psram_source) {
  const char *name = psram_source ? "copy_psram_to_sram_dma_active"
                                  : "copy_sram_to_sram_dma_active";
  uint32_t samples[TRIALS];
  bool in_flight[TRIALS];
  for (uint32_t ordinal = 0; ordinal < TRIALS; ++ordinal) {
    spi_transaction_t transaction;
    prepare_transaction(&transaction, context);
    if (queue_dma(context, &transaction) != ESP_OK) {
      emit_refusal(name, ordinal, "SPI2 DMA submission failed");
      return false;
    }
    const uint32_t *source = psram_source
                                 ? context->psram_sources +
                                       (ordinal % PSRAM_SOURCE_COUNT) * COPY_WORDS
                                 : context->sram_source;
    samples[ordinal] = dma_sram_copy_32k(context->copy_slot, source);
    in_flight[ordinal] = !dma_completed;
    if (wait_dma(context, &transaction) != ESP_OK) {
      emit_refusal(name, ordinal, "SPI2 DMA completion failed");
      return false;
    }
    if (!in_flight[ordinal]) {
      emit_refusal(name, ordinal,
                   "SPI2 DMA was not in flight when the CPU copy ended");
      return false;
    }
    benchmark_sink ^= context->copy_slot[(ordinal * 127u) % COPY_WORDS];
  }
  emit_metric(name,
              psram_source ? "psram-to-internal-sram"
                           : "internal-sram-to-internal-sram",
              "plain_32bit_copy_spi2_dma_active", COPY_WORDS,
              sizeof(uint32_t), samples, "\"copy_psram_to_sram_idle\"",
              in_flight);
  return true;
}

static bool run_submit_to_complete(probe_context_t *context) {
  uint32_t samples[TRIALS];
  for (uint32_t ordinal = 0; ordinal < TRIALS; ++ordinal) {
    spi_transaction_t transaction;
    prepare_transaction(&transaction, context);
    dma_completed = false;
    const uint32_t start = read_ccount();
    const esp_err_t queued =
        spi_device_queue_trans(context->spi, &transaction, portMAX_DELAY);
    esp_err_t completed = ESP_FAIL;
    if (queued == ESP_OK) {
      completed = wait_dma(context, &transaction);
    }
    const uint32_t end = read_ccount();
    if (queued != ESP_OK || completed != ESP_OK || end == start) {
      emit_refusal("spi2_32k_submit_to_complete", ordinal,
                   "SPI2 DMA submit-to-complete timing failed");
      return false;
    }
    samples[ordinal] = end - start;
  }
  emit_metric("spi2_32k_submit_to_complete", "internal-sram-to-spi2",
              "queue_through_completion", 1, TRANSFER_BYTES, samples, NULL,
              NULL);
  return true;
}

static bool run_submit_only(probe_context_t *context) {
  uint32_t samples[TRIALS];
  for (uint32_t ordinal = 0; ordinal < TRIALS; ++ordinal) {
    spi_transaction_t transaction;
    prepare_transaction(&transaction, context);
    dma_completed = false;
    const uint32_t start = read_ccount();
    const esp_err_t queued =
        spi_device_queue_trans(context->spi, &transaction, portMAX_DELAY);
    const uint32_t end = read_ccount();
    if (queued != ESP_OK || end == start ||
        wait_dma(context, &transaction) != ESP_OK) {
      emit_refusal("spi2_32k_submit_only", ordinal,
                   "SPI2 DMA submission timing failed");
      return false;
    }
    samples[ordinal] = end - start;
  }
  emit_metric("spi2_32k_submit_only", "internal-sram-to-spi2",
              "queue_submission_only", 1, TRANSFER_BYTES, samples, NULL,
              NULL);
  return true;
}

void app_main(void) {
  if (xPortGetCoreID() != 0) {
    printf("CALIBRATION_FAILED app task is not pinned to core 0\n");
    return;
  }
  probe_context_t context = {0};
  const esp_err_t initialized = init_context(&context);
  if (initialized != ESP_OK) {
    printf("CALIBRATION_FAILED initialization error=%s\n",
           esp_err_to_name(initialized));
    return;
  }

  esp_chip_info_t chip = {0};
  esp_chip_info(&chip);
  const uint32_t cpu_hz = esp_rom_get_cpu_ticks_per_us() * 1000000u;
  printf("CAL_RECORD {\"type\":\"configuration\","
         "\"schema_version\":\"%s\",\"harness_version\":\"%s\","
         "\"idf_version\":\"%s\",\"target\":\"esp32s3\","
         "\"chip_revision\":%u,\"cores\":%u,\"app_core\":0,"
         "\"cpu_hz\":%" PRIu32 ",\"ccount_hz\":%" PRIu32 ","
         "\"probe\":\"dma-sram\",\"trials\":%u,"
         "\"spi_host\":2,\"spi_clock_hz\":%u,\"spi_quad\":true,"
         "\"chip_select\":-1,\"transfer_bytes\":%u,"
         "\"transfer_pixels\":%u,\"ring_depth\":2}\n",
         SCHEMA_VERSION, HARNESS_VERSION, esp_get_idf_version(), chip.revision,
         chip.cores, cpu_hz, cpu_hz, TRIALS, SPI_CLOCK_HZ,
         (unsigned)TRANSFER_BYTES, TRANSFER_PIXELS);
  fflush(stdout);

  /* Warm the exact IRAM loop without consuming a PSRAM sample region. */
  benchmark_sink ^= dma_sram_copy_32k(context.copy_slot, context.sram_source);
  spi_transaction_t warm_transaction;
  prepare_transaction(&warm_transaction, &context);
  if (spi_device_polling_transmit(context.spi, &warm_transaction) != ESP_OK) {
    printf("CALIBRATION_FAILED SPI2 DMA warm-up failed\n");
    fflush(stdout);
    return;
  }

  bool ok = run_idle_copy(&context);
  if (ok) ok = run_active_copy(&context, true);
  if (ok) ok = run_active_copy(&context, false);
  if (ok) ok = run_submit_to_complete(&context);
  if (ok) ok = run_submit_only(&context);
  if (!ok) {
    printf("CALIBRATION_FAILED dma-sram probe refused a required sample\n");
    fflush(stdout);
    return;
  }

  printf("CALIBRATION_DONE sink=%" PRIu32 "\n", benchmark_sink);
  fflush(stdout);
  while (true) {
    vTaskDelay(pdMS_TO_TICKS(1000));
  }
}
