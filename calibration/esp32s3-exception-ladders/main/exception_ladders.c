#include <inttypes.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>

#include "esp_attr.h"
#include "esp_chip_info.h"
#include "esp_rom_sys.h"
#include "freertos/FreeRTOS.h"
#include "freertos/task.h"
#include "soc/extmem_reg.h"
#include "soc/soc.h"

#define SAMPLES 100u
#define MAX_ATTEMPTS 200u
#define RECURSION_DEPTH 20u

typedef uint32_t (*probe_fn_t)(uint32_t depth);

extern uint32_t call4_window_pair(uint32_t depth);
extern uint32_t call8_window_pair(uint32_t depth);
extern uint32_t call12_window_pair(uint32_t depth);
extern uint32_t syscall_rfe_pair(uint32_t depth);
extern uint32_t rfe_alone(uint32_t depth);
extern uint32_t rfi3_alone(uint32_t depth);
extern uint32_t mask_rom_fetch_straight_line(uint32_t depth);

typedef struct {
  uint32_t ibus_accesses;
  uint32_t ibus_misses;
  uint32_t dbus_accesses;
  uint32_t dbus_flash_misses;
  uint32_t dbus_psram_misses;
} cache_counters_t;

static volatile uint32_t benchmark_sink;

static inline uint32_t IRAM_ATTR read_ccount(void) {
  uint32_t value;
  __asm__ __volatile__("rsr.ccount %0" : "=a"(value));
  return value;
}

static inline uint32_t IRAM_ATTR mask_interrupts(void) {
  uint32_t previous;
  __asm__ __volatile__("rsil %0, 15" : "=a"(previous));
  return previous;
}

static inline void IRAM_ATTR restore_interrupts(uint32_t previous) {
  __asm__ __volatile__("wsr.ps %0\n rsync" : : "a"(previous));
}

static void IRAM_ATTR clear_cache_counters(void) {
  REG_WRITE(EXTMEM_CACHE_ACS_CNT_CLR_REG,
            EXTMEM_ICACHE_ACS_CNT_CLR | EXTMEM_DCACHE_ACS_CNT_CLR);
}

static cache_counters_t IRAM_ATTR read_cache_counters(void) {
  return (cache_counters_t){
      .ibus_accesses = REG_READ(EXTMEM_IBUS_ACS_CNT_REG),
      .ibus_misses = REG_READ(EXTMEM_IBUS_ACS_MISS_CNT_REG),
      .dbus_accesses = REG_READ(EXTMEM_DBUS_ACS_CNT_REG),
      .dbus_flash_misses = REG_READ(EXTMEM_DBUS_ACS_FLASH_MISS_CNT_REG),
      .dbus_psram_misses = REG_READ(EXTMEM_DBUS_ACS_SPIRAM_MISS_CNT_REG),
  };
}

static bool counters_zero(cache_counters_t counters) {
  return counters.ibus_accesses == 0 && counters.ibus_misses == 0 &&
         counters.dbus_accesses == 0 && counters.dbus_flash_misses == 0 &&
         counters.dbus_psram_misses == 0;
}

static void emit_refusal(const char *name) {
  printf("CAL_RECORD {\"type\":\"refusal\",\"name\":\"%s\","
         "\"reason\":\"cache-counter mismatch after %u attempts\","
         "\"tierCandidate\":\"exact\"}\n", name, MAX_ATTEMPTS);
  fflush(stdout);
}

static void emit_metric(const char *name, const char *memory,
                        const char *access_pattern, const uint32_t *samples) {
  printf("CAL_RECORD {\"type\":\"metric\",\"name\":\"%s\","
         "\"memory\":\"%s\",\"access_pattern\":\"%s\","
         "\"operations_per_trial\":1,\"bytes_per_operation\":0,"
         "\"ccount_samples\":[", name, memory, access_pattern);
  for (uint32_t index = 0; index < SAMPLES; ++index) {
    printf("%s%" PRIu32, index == 0 ? "" : ",", samples[index]);
  }
  printf("],\"cache_counters_required_zero\":true}\n");
  fflush(stdout);
}

static uint32_t IRAM_ATTR __attribute__((noinline))
measure_probe_samples(probe_fn_t function, uint32_t depth, uint32_t *samples) {
  uint32_t accepted = 0;
  benchmark_sink += function(depth);
  for (uint32_t attempt = 0;
       attempt < MAX_ATTEMPTS && accepted < SAMPLES; ++attempt) {
    clear_cache_counters();
    const uint32_t previous = mask_interrupts();
    const uint32_t start = read_ccount();
    benchmark_sink += function(depth);
    const uint32_t end = read_ccount();
    restore_interrupts(previous);
    const cache_counters_t counters = read_cache_counters();
    const uint32_t elapsed = end - start;
    if (elapsed != 0u && counters_zero(counters)) {
      samples[accepted++] = elapsed;
    }
  }
  return accepted;
}

static uint32_t IRAM_ATTR __attribute__((noinline))
measure_direct_samples(probe_fn_t function, uint32_t *samples) {
  uint32_t accepted = 0;
  benchmark_sink += function(0);
  for (uint32_t attempt = 0;
       attempt < MAX_ATTEMPTS && accepted < SAMPLES; ++attempt) {
    clear_cache_counters();
    const uint32_t previous = mask_interrupts();
    const uint32_t elapsed = function(0);
    restore_interrupts(previous);
    const cache_counters_t counters = read_cache_counters();
    benchmark_sink += elapsed;
    if (elapsed != 0u && counters_zero(counters)) {
      samples[accepted++] = elapsed;
    }
  }
  return accepted;
}

static void run_probe(const char *name, probe_fn_t function, uint32_t depth,
                      const char *memory, const char *access_pattern) {
  uint32_t samples[SAMPLES];
  const uint32_t accepted = measure_probe_samples(function, depth, samples);
  if (accepted == SAMPLES) {
    emit_metric(name, memory, access_pattern, samples);
  } else {
    emit_refusal(name);
  }
}

static void run_direct_probe(const char *name, probe_fn_t function) {
  uint32_t samples[SAMPLES];
  const uint32_t accepted = measure_direct_samples(function, samples);
  if (accepted == SAMPLES) {
    emit_metric(name, "iram", "exception-ladder", samples);
  } else {
    emit_refusal(name);
  }
}

void app_main(void) {
  esp_chip_info_t chip = {0};
  esp_chip_info(&chip);
  const uint32_t cpu_hz = esp_rom_get_cpu_ticks_per_us() * 1000000u;
  printf("CAL_RECORD {\"type\":\"configuration\","
         "\"schema_version\":\"1.0.0\",\"harness_version\":\"1.2.0\","
         "\"idf_version\":\"%s\",\"target\":\"esp32s3\","
         "\"chip_revision\":%u,\"cores\":%u,\"cpu_hz\":%" PRIu32 ","
         "\"ccount_hz\":%" PRIu32 ",\"probe\":\"exception-ladders\","
         "\"samples_per_cell\":%u,\"max_attempts_per_cell\":%u,"
         "\"recursion_depth\":%u}\n", esp_get_idf_version(), chip.revision,
         chip.cores, cpu_hz, cpu_hz, SAMPLES, MAX_ATTEMPTS, RECURSION_DEPTH);
  fflush(stdout);

  run_probe("call4_window_pair", call4_window_pair, RECURSION_DEPTH, "iram",
            "exception-ladder");
  run_probe("call8_window_pair", call8_window_pair, RECURSION_DEPTH, "iram",
            "exception-ladder");
  run_probe("call12_window_pair", call12_window_pair, RECURSION_DEPTH, "iram",
            "exception-ladder");
  run_direct_probe("syscall_rfe_pair", syscall_rfe_pair);
  run_direct_probe("rfe_alone", rfe_alone);
  run_direct_probe("rfi3_alone", rfi3_alone);
  run_probe("mask_rom_fetch_straight_line", mask_rom_fetch_straight_line, 0,
            "rom", "straight-line");

  printf("CALIBRATION_DONE sink=%" PRIu32 "\n", benchmark_sink);
  fflush(stdout);
  while (true) {
    vTaskDelay(pdMS_TO_TICKS(1000));
  }
}
