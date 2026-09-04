#include <inttypes.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdio.h>

#include "esp_attr.h"
#include "esp_chip_info.h"
#include "esp_rom_sys.h"
#include "freertos/FreeRTOS.h"
#include "freertos/task.h"
#include "soc/extmem_reg.h"
#include "soc/soc.h"

#define SAMPLES 100u
#define MAX_ATTEMPTS 200u

typedef uint32_t (*probe_fn_t)(uint32_t unused);

extern uint32_t rfe_alone(uint32_t unused);
extern uint32_t rfi3_alone(uint32_t unused);
extern uint32_t syscall_rfe_pair(uint32_t unused);
extern uint32_t window_overflow8_entry(uint32_t unused);
extern uint32_t window_overflow8_control(uint32_t unused);
extern uint32_t window_underflow8_entry(uint32_t unused);
extern uint32_t window_underflow8_control(uint32_t unused);
extern uint32_t rfwo_alone(uint32_t unused);
extern uint32_t rfwu_alone(uint32_t unused);
extern uint32_t mask_rom_fetch_straight_line(uint32_t unused);
extern uint32_t iram_fetch_matched_control(uint32_t unused);

volatile uint32_t h2_vector_timestamp;
static volatile uint32_t benchmark_sink;
static volatile uint32_t state_rejections;

typedef struct {
  uint32_t ibus_accesses;
  uint32_t ibus_misses;
  uint32_t dbus_accesses;
  uint32_t dbus_flash_misses;
  uint32_t dbus_psram_misses;
} cache_counters_t;

typedef struct {
  uint32_t ps;
  uint32_t windowbase;
  uint32_t windowstart;
  uint32_t epc1;
  uint32_t epc3;
  uint32_t eps3;
  uint32_t excsave1;
  uint32_t excsave2;
  uint32_t excsave3;
  uint32_t exccause;
  uint32_t vecbase;
  uint32_t sar;
} probe_state_t;

#define READ_SPECIAL(name, destination) \
  __asm__ __volatile__("rsr %0, " #name : "=a"(destination))

static inline void IRAM_ATTR snapshot_state(probe_state_t *state) {
  READ_SPECIAL(ps, state->ps);
  READ_SPECIAL(windowbase, state->windowbase);
  READ_SPECIAL(windowstart, state->windowstart);
  READ_SPECIAL(epc1, state->epc1);
  READ_SPECIAL(epc3, state->epc3);
  READ_SPECIAL(eps3, state->eps3);
  READ_SPECIAL(excsave1, state->excsave1);
  READ_SPECIAL(excsave2, state->excsave2);
  READ_SPECIAL(excsave3, state->excsave3);
  READ_SPECIAL(exccause, state->exccause);
  READ_SPECIAL(vecbase, state->vecbase);
  READ_SPECIAL(sar, state->sar);
}

static bool IRAM_ATTR same_state(const probe_state_t *left,
                                 const probe_state_t *right) {
  return left->ps == right->ps && left->windowbase == right->windowbase &&
         left->windowstart == right->windowstart && left->epc1 == right->epc1 &&
         left->epc3 == right->epc3 &&
         left->eps3 == right->eps3 && left->excsave1 == right->excsave1 &&
         left->excsave2 == right->excsave2 && left->excsave3 == right->excsave3 &&
         left->exccause == right->exccause && left->vecbase == right->vecbase &&
         left->sar == right->sar;
}

static inline void IRAM_ATTR disable_window_exceptions(void) {
  uint32_t ps;
  __asm__ __volatile__(
      "rsr %0, ps\n"
      "movi a3, 1\n"
      "slli a3, a3, 18\n"
      "or %0, %0, a3\n"
      "xor %0, %0, a3\n"
      "wsr %0, ps\n"
      "rsync"
      : "=&a"(ps)
      :
      : "a3");
}

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
         "\"reason\":\"cache or state mismatch after %u attempts; state_rejections=%" PRIu32 "\"," 
         "\"tierCandidate\":\"exact\"}\n", name, MAX_ATTEMPTS,
         state_rejections);
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
measure_elapsed_samples(probe_fn_t function, uint32_t *samples) {
  uint32_t accepted = 0;
  probe_state_t before;
  probe_state_t after;
  uint32_t previous = mask_interrupts();
  snapshot_state(&before);
  benchmark_sink += function(0);
  snapshot_state(&after);
  restore_interrupts(previous);
  if (!same_state(&before, &after)) {
    ++state_rejections;
    return 0;
  }
  for (uint32_t attempt = 0;
       attempt < MAX_ATTEMPTS && accepted < SAMPLES; ++attempt) {
    clear_cache_counters();
    previous = mask_interrupts();
    snapshot_state(&before);
    const uint32_t elapsed = function(0);
    snapshot_state(&after);
    restore_interrupts(previous);
    const cache_counters_t counters = read_cache_counters();
    benchmark_sink += elapsed;
    const bool state_matches = same_state(&before, &after);
    state_rejections += state_matches ? 0u : 1u;
    if (elapsed != 0u && counters_zero(counters) && state_matches) {
      samples[accepted++] = elapsed;
    }
  }
  return accepted;
}

static uint32_t IRAM_ATTR __attribute__((noinline))
measure_target_samples(probe_fn_t function, uint32_t *samples) {
  uint32_t accepted = 0;
  probe_state_t before;
  probe_state_t after;
  uint32_t previous = mask_interrupts();
  disable_window_exceptions();
  snapshot_state(&before);
  benchmark_sink += function(0);
  snapshot_state(&after);
  restore_interrupts(previous);
  if (!same_state(&before, &after)) {
    ++state_rejections;
    return 0;
  }
  for (uint32_t attempt = 0;
       attempt < MAX_ATTEMPTS && accepted < SAMPLES; ++attempt) {
    clear_cache_counters();
    previous = mask_interrupts();
    disable_window_exceptions();
    snapshot_state(&before);
    const uint32_t start = read_ccount();
    benchmark_sink += function(0);
    const uint32_t end = read_ccount();
    snapshot_state(&after);
    restore_interrupts(previous);
    const cache_counters_t counters = read_cache_counters();
    const uint32_t elapsed = end - start;
    const bool state_matches = same_state(&before, &after);
    state_rejections += state_matches ? 0u : 1u;
    if (elapsed != 0u && counters_zero(counters) && state_matches) {
      samples[accepted++] = elapsed;
    }
  }
  return accepted;
}

static void run_elapsed(const char *name, probe_fn_t function) {
  uint32_t samples[SAMPLES];
  const uint32_t accepted = measure_elapsed_samples(function, samples);
  if (accepted == SAMPLES) {
    emit_metric(name, "iram", "exception-rank", samples);
  } else {
    emit_refusal(name);
  }
}

static void run_target(const char *name, const char *memory,
                       probe_fn_t function) {
  uint32_t samples[SAMPLES];
  const uint32_t accepted = measure_target_samples(function, samples);
  if (accepted == SAMPLES) {
    emit_metric(name, memory, "matched-straight-line", samples);
  } else {
    emit_refusal(name);
  }
}

void app_main(void) {
  esp_chip_info_t chip = {0};
  esp_chip_info(&chip);
  const uint32_t cpu_hz = esp_rom_get_cpu_ticks_per_us() * 1000000u;
  printf("CAL_RECORD {\"type\":\"configuration\"," 
         "\"schema_version\":\"1.0.0\",\"harness_version\":\"2.0.0\"," 
         "\"idf_version\":\"%s\",\"target\":\"esp32s3\"," 
         "\"chip_revision\":%u,\"cores\":%u,\"cpu_hz\":%" PRIu32 ","
         "\"ccount_hz\":%" PRIu32 ",\"probe\":\"exception-rank-followup\"," 
         "\"samples_per_cell\":%u,\"max_attempts_per_cell\":%u,"
         "\"recursion_depth\":1}\n",
         esp_get_idf_version(), chip.revision, chip.cores, cpu_hz, cpu_hz,
         SAMPLES, MAX_ATTEMPTS);
  fflush(stdout);

  run_elapsed("rfe_alone", rfe_alone);
  run_elapsed("rfi3_alone", rfi3_alone);
  run_elapsed("syscall_rfe_pair", syscall_rfe_pair);
  run_elapsed("window_overflow8_entry", window_overflow8_entry);
  run_elapsed("window_overflow8_control", window_overflow8_control);
  run_elapsed("window_underflow8_entry", window_underflow8_entry);
  run_elapsed("window_underflow8_control", window_underflow8_control);
  run_elapsed("rfwo_alone", rfwo_alone);
  run_elapsed("rfwu_alone", rfwu_alone);
  run_target("mask_rom_fetch_straight_line", "rom",
             mask_rom_fetch_straight_line);
  run_target("iram_fetch_matched_control", "iram",
             iram_fetch_matched_control);

  printf("CALIBRATION_DONE sink=%" PRIu32 "\n", benchmark_sink);
  fflush(stdout);
  while (true) {
    vTaskDelay(pdMS_TO_TICKS(1000));
  }
}
