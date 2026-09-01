#include <inttypes.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdio.h>

#include "esp_attr.h"
#include "esp_chip_info.h"
#include "esp_rom_sys.h"
#include "esp_system.h"
#include "freertos/FreeRTOS.h"
#include "freertos/task.h"
#include "soc/extmem_reg.h"
#include "soc/soc.h"

#define HARNESS_VERSION "1.0.0"
#define SAMPLE_COUNT 100u
#define MAX_ATTEMPTS 200u
#define BLOCK_ACCESSES 256u

typedef uint32_t (*read_probe_t)(volatile const uint32_t *);
typedef uint32_t (*write_probe_t)(volatile uint32_t *, uint32_t);

uint32_t register_read_1(volatile const uint32_t *address);
uint32_t register_read_2(volatile const uint32_t *address);
uint32_t register_read_4(volatile const uint32_t *address);
uint32_t register_read_8(volatile const uint32_t *address);
uint32_t register_read_16(volatile const uint32_t *address);
uint32_t register_read_256(volatile const uint32_t *address);
uint32_t register_write_1(volatile uint32_t *address, uint32_t value);
uint32_t register_write_2(volatile uint32_t *address, uint32_t value);
uint32_t register_write_4(volatile uint32_t *address, uint32_t value);
uint32_t register_write_8(volatile uint32_t *address, uint32_t value);
uint32_t register_write_16(volatile uint32_t *address, uint32_t value);
uint32_t register_write_256(volatile uint32_t *address, uint32_t value);

typedef enum {
  PROBE_READ,
  PROBE_WRITE,
} probe_kind_t;

typedef struct {
  const char *id;
  uintptr_t address;
  uint32_t operations;
  probe_kind_t kind;
  bool rtc_domain;
} cell_t;

typedef struct {
  uint32_t ibus_accesses;
  uint32_t ibus_misses;
  uint32_t dbus_accesses;
  uint32_t dbus_flash_misses;
  uint32_t dbus_psram_misses;
} cache_counters_t;

static volatile uint32_t sram_word;
static volatile uint32_t benchmark_sink;

static const cell_t cells[] = {
    {"mmio_read_io_mux_gpio45_config", 0x600090b4u, BLOCK_ACCESSES, PROBE_READ, false},
    {"mmio_read_rtc_reset_state", 0x60008038u, BLOCK_ACCESSES, PROBE_READ, true},
    {"mmio_read_spi1_user", 0x60002018u, BLOCK_ACCESSES, PROBE_READ, false},
    {"mmio_read_sensitive_sram_usage1", 0x600c1014u, BLOCK_ACCESSES, PROBE_READ, false},
    {"mmio_read_extmem_dcache_ctrl1", 0x600c4004u, BLOCK_ACCESSES, PROBE_READ, false},
    {"mmio_read_spi0_user", 0x60003018u, BLOCK_ACCESSES, PROBE_READ, false},
    {"mmio_read_i2c0_filter_config", 0x60013050u, BLOCK_ACCESSES, PROBE_READ, false},
    {"mmio_read_spi2_user1", 0x60024014u, BLOCK_ACCESSES, PROBE_READ, false},
    {"mmio_read_sha_busy", 0x6003b018u, BLOCK_ACCESSES, PROBE_READ, false},
    {"mmio_read_systimer_unit0_value_low", 0x60023044u, BLOCK_ACCESSES, PROBE_READ, false},
    {"mmio_read_apb_saradc_ctrl", 0x60040000u, BLOCK_ACCESSES, PROBE_READ, false},
    {"mmio_read_apb_ctrl_wifi_clk_en", 0x60026014u, BLOCK_ACCESSES, PROBE_READ, false},
    {"mmio_read_timg0_int_enable", 0x6001f070u, BLOCK_ACCESSES, PROBE_READ, false},
    {"mmio_read_gdma_out_peri_select", 0x6003f0a8u, BLOCK_ACCESSES, PROBE_READ, false},
    {"mmio_read_uart0_status", 0x6000001cu, BLOCK_ACCESSES, PROBE_READ, false},
    {"mmio_read_timg1_int_enable", 0x60020070u, BLOCK_ACCESSES, PROBE_READ, false},
    {"mmio_read_efuse_repeat_data3", 0x6000703cu, BLOCK_ACCESSES, PROBE_READ, false},
    {"mmio_read_i2c_mst_config", 0x6000e044u, BLOCK_ACCESSES, PROBE_READ, false},
    {"mmio_read_usb_serial_jtag_int_raw", 0x60038008u, BLOCK_ACCESSES, PROBE_READ, false},
    {"mmio_read_assist_debug_status", 0x600ce05cu, BLOCK_ACCESSES, PROBE_READ, false},
    {"mmio_read_nrx_config", 0x6001ccd4u, BLOCK_ACCESSES, PROBE_READ, false},
    {"mmio_read_fe2_config", 0x600050f0u, BLOCK_ACCESSES, PROBE_READ, false},

    {"mmio_write_interrupt_core1_mac_map", 0x600c2800u, BLOCK_ACCESSES, PROBE_WRITE, false},
    {"mmio_write_io_mux_gpio45_config", 0x600090b4u, BLOCK_ACCESSES, PROBE_WRITE, false},
    {"mmio_write_rtc_clock_config", 0x60008088u, BLOCK_ACCESSES, PROBE_WRITE, true},
    {"mmio_write_spi1_user", 0x60002018u, BLOCK_ACCESSES, PROBE_WRITE, false},
    {"mmio_write_sensitive_sram_usage1", 0x600c1014u, BLOCK_ACCESSES, PROBE_WRITE, false},
    {"mmio_write_extmem_dcache_ctrl1", 0x600c4004u, BLOCK_ACCESSES, PROBE_WRITE, false},
    {"mmio_write_spi0_user", 0x60003018u, BLOCK_ACCESSES, PROBE_WRITE, false},
    {"mmio_write_i2c0_filter_config", 0x60013050u, BLOCK_ACCESSES, PROBE_WRITE, false},
    {"mmio_write_spi2_user1", 0x60024014u, BLOCK_ACCESSES, PROBE_WRITE, false},
    {"mmio_write_sha_mode", 0x6003b000u, BLOCK_ACCESSES, PROBE_WRITE, false},
    {"mmio_write_systimer_config", 0x60023000u, BLOCK_ACCESSES, PROBE_WRITE, false},
    {"mmio_write_apb_saradc_ctrl", 0x60040000u, BLOCK_ACCESSES, PROBE_WRITE, false},
    {"mmio_write_apb_ctrl_wifi_clk_en", 0x60026014u, BLOCK_ACCESSES, PROBE_WRITE, false},
    {"mmio_write_timg0_int_enable", 0x6001f070u, BLOCK_ACCESSES, PROBE_WRITE, false},
    {"mmio_write_gdma_out_peri_select", 0x6003f0a8u, BLOCK_ACCESSES, PROBE_WRITE, false},
    {"mmio_write_uart0_config0", 0x60000020u, BLOCK_ACCESSES, PROBE_WRITE, false},
    {"mmio_write_timg1_int_enable", 0x60020070u, BLOCK_ACCESSES, PROBE_WRITE, false},
    {"mmio_write_i2c_mst_config", 0x6000e044u, BLOCK_ACCESSES, PROBE_WRITE, false},
    {"mmio_write_assist_debug_enable", 0x600ce048u, BLOCK_ACCESSES, PROBE_WRITE, false},
    {"mmio_write_nrx_config", 0x6001ccd4u, BLOCK_ACCESSES, PROBE_WRITE, false},
    {"mmio_write_fe2_config", 0x600050f0u, BLOCK_ACCESSES, PROBE_WRITE, false},

    {"system_config_read_run_001", 0x600c0060u, 1u, PROBE_READ, false},
    {"system_config_read_run_002", 0x600c0060u, 2u, PROBE_READ, false},
    {"system_config_read_run_004", 0x600c0060u, 4u, PROBE_READ, false},
    {"system_config_read_run_008", 0x600c0060u, 8u, PROBE_READ, false},
    {"system_config_read_run_016", 0x600c0060u, 16u, PROBE_READ, false},
    {"system_config_read_run_256", 0x600c0060u, 256u, PROBE_READ, false},
    {"system_config_write_run_001", 0x600c0060u, 1u, PROBE_WRITE, false},
    {"system_config_write_run_002", 0x600c0060u, 2u, PROBE_WRITE, false},
    {"system_config_write_run_004", 0x600c0060u, 4u, PROBE_WRITE, false},
    {"system_config_write_run_008", 0x600c0060u, 8u, PROBE_WRITE, false},
    {"system_config_write_run_016", 0x600c0060u, 16u, PROBE_WRITE, false},
    {"system_config_write_run_256", 0x600c0060u, 256u, PROBE_WRITE, false},
    {"gpio_config_read_run_001", 0x600040b0u, 1u, PROBE_READ, false},
    {"gpio_config_read_run_002", 0x600040b0u, 2u, PROBE_READ, false},
    {"gpio_config_read_run_004", 0x600040b0u, 4u, PROBE_READ, false},
    {"gpio_config_read_run_008", 0x600040b0u, 8u, PROBE_READ, false},
    {"gpio_config_read_run_016", 0x600040b0u, 16u, PROBE_READ, false},
    {"gpio_config_read_run_256", 0x600040b0u, 256u, PROBE_READ, false},
    {"gpio_config_write_run_001", 0x600040b0u, 1u, PROBE_WRITE, false},
    {"gpio_config_write_run_002", 0x600040b0u, 2u, PROBE_WRITE, false},
    {"gpio_config_write_run_004", 0x600040b0u, 4u, PROBE_WRITE, false},
    {"gpio_config_write_run_008", 0x600040b0u, 8u, PROBE_WRITE, false},
    {"gpio_config_write_run_016", 0x600040b0u, 16u, PROBE_WRITE, false},
    {"gpio_config_write_run_256", 0x600040b0u, 256u, PROBE_WRITE, false},

    {"sram_read_baseline", (uintptr_t)&sram_word, BLOCK_ACCESSES, PROBE_READ, false},
    {"sram_write_baseline", (uintptr_t)&sram_word, BLOCK_ACCESSES, PROBE_WRITE, false},
};

static inline uint32_t mask_interrupts(void) {
  uint32_t previous;
  __asm__ __volatile__("rsil %0, 15" : "=a"(previous));
  return previous;
}

static inline void restore_interrupts(uint32_t previous) {
  __asm__ __volatile__("wsr.ps %0\n rsync" : : "a"(previous));
}

static IRAM_ATTR void clear_cache_counters(void) {
  REG_WRITE(EXTMEM_CACHE_ACS_CNT_CLR_REG,
            EXTMEM_ICACHE_ACS_CNT_CLR | EXTMEM_DCACHE_ACS_CNT_CLR);
}

static IRAM_ATTR cache_counters_t read_cache_counters(void) {
  return (cache_counters_t){
      .ibus_accesses = REG_READ(EXTMEM_IBUS_ACS_CNT_REG),
      .ibus_misses = REG_READ(EXTMEM_IBUS_ACS_MISS_CNT_REG),
      .dbus_accesses = REG_READ(EXTMEM_DBUS_ACS_CNT_REG),
      .dbus_flash_misses = REG_READ(EXTMEM_DBUS_ACS_FLASH_MISS_CNT_REG),
      .dbus_psram_misses = REG_READ(EXTMEM_DBUS_ACS_SPIRAM_MISS_CNT_REG),
  };
}

static bool counters_zero(cache_counters_t counters) {
  return counters.ibus_accesses == 0u && counters.ibus_misses == 0u &&
         counters.dbus_accesses == 0u && counters.dbus_flash_misses == 0u &&
         counters.dbus_psram_misses == 0u;
}

static read_probe_t read_probe(uint32_t operations) {
  switch (operations) {
    case 1:
      return register_read_1;
    case 2:
      return register_read_2;
    case 4:
      return register_read_4;
    case 8:
      return register_read_8;
    case 16:
      return register_read_16;
    default:
      return register_read_256;
  }
}

static write_probe_t write_probe(uint32_t operations) {
  switch (operations) {
    case 1:
      return register_write_1;
    case 2:
      return register_write_2;
    case 4:
      return register_write_4;
    case 8:
      return register_write_8;
    case 16:
      return register_write_16;
    default:
      return register_write_256;
  }
}

static IRAM_ATTR uint32_t measure_once(const cell_t *cell,
                                       cache_counters_t *counters) {
  volatile uint32_t *address = (volatile uint32_t *)cell->address;
  const bool is_read = cell->kind == PROBE_READ;
  const read_probe_t reader = read_probe(cell->operations);
  const write_probe_t writer = write_probe(cell->operations);
  const uint32_t same_value = *address;
  const uint32_t previous = mask_interrupts();
  clear_cache_counters();
  uint32_t elapsed;
  if (is_read) {
    elapsed = reader(address);
  } else {
    elapsed = writer(address, same_value);
  }
  *counters = read_cache_counters();
  restore_interrupts(previous);
  benchmark_sink ^= elapsed;
  return elapsed;
}

static void sort_samples(uint32_t samples[SAMPLE_COUNT]) {
  for (uint32_t index = 1; index < SAMPLE_COUNT; ++index) {
    const uint32_t value = samples[index];
    uint32_t position = index;
    while (position > 0u && samples[position - 1u] > value) {
      samples[position] = samples[position - 1u];
      --position;
    }
    samples[position] = value;
  }
}

static uint64_t cycles_per_operation_x1000000(uint32_t cycles,
                                              uint32_t operations) {
  return ((uint64_t)cycles * 1000000u) / operations;
}

static void emit_metric(const cell_t *cell, const uint32_t *samples) {
  uint32_t ordered[SAMPLE_COUNT];
  for (uint32_t index = 0; index < SAMPLE_COUNT; ++index) {
    ordered[index] = samples[index];
  }
  sort_samples(ordered);
  printf("CAL_RECORD {\"type\":\"metric\",\"name\":\"%s\","
         "\"memory\":\"%s\",\"access_pattern\":\"back_to_back_%s\","
         "\"operations_per_trial\":%" PRIu32 ",\"bytes_per_operation\":4,"
         "\"ccount_samples\":[",
         cell->id, cell->address == (uintptr_t)&sram_word ? "sram" : "mmio",
         cell->kind == PROBE_READ ? "read" : "same_value_write",
         cell->operations);
  for (uint32_t index = 0; index < SAMPLE_COUNT; ++index) {
    printf("%s%" PRIu32, index == 0u ? "" : ",", samples[index]);
  }
  printf("],\"cycles_per_operation_x1000000\":{"
         "\"min\":%" PRIu64 ",\"median\":%" PRIu64 ","
         "\"p90\":%" PRIu64 ",\"max\":%" PRIu64 "},"
         "\"cache_counters_asserted_zero\":true,\"clock_domain\":\"%s\"}\n",
         cycles_per_operation_x1000000(ordered[0], cell->operations),
         cycles_per_operation_x1000000(ordered[49], cell->operations),
         cycles_per_operation_x1000000(ordered[89], cell->operations),
         cycles_per_operation_x1000000(ordered[99], cell->operations),
         cell->rtc_domain ? "rtc" : "apb");
  fflush(stdout);
}

static void run_cell(const cell_t *cell) {
  uint32_t samples[SAMPLE_COUNT];
  uint32_t accepted = 0;
  for (uint32_t attempt = 0; attempt < MAX_ATTEMPTS && accepted < SAMPLE_COUNT;
       ++attempt) {
    cache_counters_t counters;
    const uint32_t elapsed = measure_once(cell, &counters);
    if (elapsed != 0u && counters_zero(counters)) {
      samples[accepted++] = elapsed;
    }
    if ((attempt & 15u) == 15u) {
      vTaskDelay(1);
    }
  }
  if (accepted != SAMPLE_COUNT) {
    printf("CAL_RECORD {\"type\":\"refusal\",\"name\":\"%s\","
           "\"reason\":\"cache-counter mismatch after %u attempts\","
           "\"tier_candidate\":\"distribution\"}\n",
           cell->id, MAX_ATTEMPTS);
    fflush(stdout);
    return;
  }
  emit_metric(cell, samples);
}

void app_main(void) {
  esp_chip_info_t chip = {0};
  esp_chip_info(&chip);
  const uint32_t cpu_hz = esp_rom_get_cpu_ticks_per_us() * 1000000u;
  printf("CAL_RECORD {\"type\":\"configuration\","
         "\"schema_version\":\"1.0.0\",\"harness_version\":\"%s\","
         "\"idf_version\":\"%s\",\"target\":\"esp32s3\","
         "\"chip_revision\":%u,\"cores\":%u,\"cpu_hz\":%" PRIu32 ","
         "\"ccount_hz\":%" PRIu32 ",\"probe\":\"register-blocks\","
         "\"samples_per_cell\":%u,\"cache_counters_required_zero\":true}\n",
         HARNESS_VERSION, esp_get_idf_version(), chip.revision, chip.cores,
         cpu_hz, cpu_hz, SAMPLE_COUNT);
  fflush(stdout);

  for (uint32_t index = 0; index < sizeof(cells) / sizeof(cells[0]); ++index) {
    run_cell(&cells[index]);
  }

  printf("CALIBRATION_DONE sink=%" PRIu32 "\n", benchmark_sink);
  fflush(stdout);
  while (true) {
    vTaskDelay(pdMS_TO_TICKS(1000));
  }
}
