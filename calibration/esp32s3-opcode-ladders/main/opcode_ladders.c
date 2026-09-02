/* ESP32-S3 LX7 opcode timing ladders. Hardware captures remain candidates. */

#include <inttypes.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>

#include "esp_attr.h"
#include "esp_chip_info.h"
#include "esp_rom_sys.h"
#include "esp_system.h"
#include "freertos/FreeRTOS.h"
#include "freertos/task.h"
#include "soc/extmem_reg.h"
#include "soc/soc.h"

#define SCHEMA_VERSION "1.0.0"
#define HARNESS_VERSION "1.0.0"
#define SAMPLES 100u
#define MAX_ATTEMPTS 200u
#define OPS_PER_BLOCK 256u

typedef void (*probe_fn_t)(volatile uint32_t *word);

#define PROBE_LIST(X)                                                           \
  X("beqz_taken", opcode_beqz_taken, "branch-taken")                          \
  X("beqz_not_taken", opcode_beqz_not_taken, "branch-not-taken")              \
  X("bnez_taken", opcode_bnez_taken, "branch-taken")                          \
  X("bnez_not_taken", opcode_bnez_not_taken, "branch-not-taken")              \
  X("bltz_taken", opcode_bltz_taken, "branch-taken")                          \
  X("bltz_not_taken", opcode_bltz_not_taken, "branch-not-taken")              \
  X("bgez_taken", opcode_bgez_taken, "branch-taken")                          \
  X("bgez_not_taken", opcode_bgez_not_taken, "branch-not-taken")              \
  X("beqi_taken", opcode_beqi_taken, "branch-taken")                          \
  X("beqi_not_taken", opcode_beqi_not_taken, "branch-not-taken")              \
  X("bnei_taken", opcode_bnei_taken, "branch-taken")                          \
  X("bnei_not_taken", opcode_bnei_not_taken, "branch-not-taken")              \
  X("beq_taken", opcode_beq_taken, "branch-taken")                            \
  X("beq_not_taken", opcode_beq_not_taken, "branch-not-taken")                \
  X("bne_taken", opcode_bne_taken, "branch-taken")                            \
  X("bne_not_taken", opcode_bne_not_taken, "branch-not-taken")                \
  X("blt_taken", opcode_blt_taken, "branch-taken")                            \
  X("blt_not_taken", opcode_blt_not_taken, "branch-not-taken")                \
  X("bge_taken", opcode_bge_taken, "branch-taken")                            \
  X("bge_not_taken", opcode_bge_not_taken, "branch-not-taken")                \
  X("bltu_taken", opcode_bltu_taken, "branch-taken")                          \
  X("bltu_not_taken", opcode_bltu_not_taken, "branch-not-taken")              \
  X("bgeu_taken", opcode_bgeu_taken, "branch-taken")                          \
  X("bgeu_not_taken", opcode_bgeu_not_taken, "branch-not-taken")              \
  X("blti_taken", opcode_blti_taken, "branch-taken")                          \
  X("blti_not_taken", opcode_blti_not_taken, "branch-not-taken")              \
  X("bgei_taken", opcode_bgei_taken, "branch-taken")                          \
  X("bgei_not_taken", opcode_bgei_not_taken, "branch-not-taken")              \
  X("bltui_taken", opcode_bltui_taken, "branch-taken")                        \
  X("bltui_not_taken", opcode_bltui_not_taken, "branch-not-taken")            \
  X("bgeui_taken", opcode_bgeui_taken, "branch-taken")                        \
  X("bgeui_not_taken", opcode_bgeui_not_taken, "branch-not-taken")            \
  X("bany_taken", opcode_bany_taken, "branch-taken")                          \
  X("bany_not_taken", opcode_bany_not_taken, "branch-not-taken")              \
  X("bnone_taken", opcode_bnone_taken, "branch-taken")                        \
  X("bnone_not_taken", opcode_bnone_not_taken, "branch-not-taken")            \
  X("ball_taken", opcode_ball_taken, "branch-taken")                          \
  X("ball_not_taken", opcode_ball_not_taken, "branch-not-taken")              \
  X("bnall_taken", opcode_bnall_taken, "branch-taken")                        \
  X("bnall_not_taken", opcode_bnall_not_taken, "branch-not-taken")            \
  X("bbc_taken", opcode_bbc_taken, "branch-taken")                            \
  X("bbc_not_taken", opcode_bbc_not_taken, "branch-not-taken")                \
  X("bbs_taken", opcode_bbs_taken, "branch-taken")                            \
  X("bbs_not_taken", opcode_bbs_not_taken, "branch-not-taken")                \
  X("bbci_taken", opcode_bbci_taken, "branch-taken")                          \
  X("bbci_not_taken", opcode_bbci_not_taken, "branch-not-taken")              \
  X("bbsi_taken", opcode_bbsi_taken, "branch-taken")                          \
  X("bbsi_not_taken", opcode_bbsi_not_taken, "branch-not-taken")              \
  X("beqz_n_taken", opcode_beqz_n_taken, "branch-taken")                      \
  X("beqz_n_not_taken", opcode_beqz_n_not_taken, "branch-not-taken")          \
  X("bnez_n_taken", opcode_bnez_n_taken, "branch-taken")                      \
  X("bnez_n_not_taken", opcode_bnez_n_not_taken, "branch-not-taken")          \
  X("j", opcode_j, "jump")                                                     \
  X("jx", opcode_jx, "indirect-jump")                                          \
  X("call0_ret", opcode_call0_ret, "call-return")                              \
  X("callx0_ret", opcode_callx0_ret, "call-return")                            \
  X("call8_retw", opcode_call8_retw, "call-return")                            \
  X("callx8_retw", opcode_callx8_retw, "call-return")                          \
  X("loop", opcode_loop, "loop-setup")                                         \
  X("loopnez", opcode_loopnez, "loop-setup")                                   \
  X("loopgtz", opcode_loopgtz, "loop-setup")                                   \
  X("issue_nop_baseline", opcode_issue_nop_baseline, "matched-baseline")        \
  X("mull", opcode_mull, "independent-operands")                               \
  X("mulsh", opcode_mulsh, "independent-operands")                             \
  X("muluh", opcode_muluh, "independent-operands")                             \
  X("quos", opcode_quos, "independent-operands")                               \
  X("quou", opcode_quou, "independent-operands")                               \
  X("rems", opcode_rems, "independent-operands")                               \
  X("remu", opcode_remu, "independent-operands")                               \
  X("nsa", opcode_nsa, "independent-operands")                                 \
  X("nsau", opcode_nsau, "independent-operands")                               \
  X("sext", opcode_sext, "independent-operands")                               \
  X("l32r", opcode_l32r, "literal-load")                                       \
  X("s32c1i", opcode_s32c1i, "atomic-store")                                   \
  X("memw", opcode_memw, "memory-order")                                       \
  X("extw", opcode_extw, "memory-order")                                       \
  X("rsr", opcode_rsr, "special-register")                                     \
  X("wsr", opcode_wsr, "special-register")                                     \
  X("xsr", opcode_xsr, "special-register")                                     \
  X("rsync", opcode_rsync, "synchronization")                                  \
  X("isync", opcode_isync, "synchronization")                                  \
  X("movsp", opcode_movsp, "independent-operands")                             \
  X("min", opcode_min, "independent-operands")                                 \
  X("max", opcode_max, "independent-operands")                                 \
  X("minu", opcode_minu, "independent-operands")                               \
  X("maxu", opcode_maxu, "independent-operands")                               \
  X("load_use_distance_1", load_use_distance_1, "dependent-load-distance-1")   \
  X("load_use_distance_2", load_use_distance_2, "dependent-load-distance-2")

#define DECLARE_PROBE(id, symbol, pattern) extern void symbol(volatile uint32_t *word);
PROBE_LIST(DECLARE_PROBE)
#undef DECLARE_PROBE

typedef struct {
  const char *id;
  probe_fn_t function;
  const char *pattern;
} probe_t;

#define DEFINE_PROBE(id, symbol, pattern) {id, symbol, pattern},
static const probe_t probes[] = {PROBE_LIST(DEFINE_PROBE)};
#undef DEFINE_PROBE

typedef struct {
  uint32_t ibus_accesses;
  uint32_t ibus_misses;
  uint32_t dbus_accesses;
  uint32_t dbus_flash_misses;
  uint32_t dbus_psram_misses;
} cache_counters_t;

static volatile uint32_t benchmark_word = 7;

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

static void sort_samples(uint32_t *values) {
  for (uint32_t index = 1; index < SAMPLES; ++index) {
    const uint32_t value = values[index];
    uint32_t position = index;
    while (position != 0 && values[position - 1] > value) {
      values[position] = values[position - 1];
      --position;
    }
    values[position] = value;
  }
}

static void print_cycles_per_op(uint64_t doubled_cycles) {
  const uint64_t denominator = 2u * OPS_PER_BLOCK;
  const uint64_t whole = doubled_cycles / denominator;
  const uint64_t fraction = (doubled_cycles % denominator) * 1000000u / denominator;
  printf("%" PRIu64 ".%06" PRIu64, whole, fraction);
}

static void emit_refusal(const char *name) {
  printf("CAL_RECORD {\"type\":\"refusal\",\"name\":\"%s\","
         "\"reason\":\"cache-counter mismatch after %u attempts\","
         "\"tierCandidate\":\"distribution\"}\n",
         name, MAX_ATTEMPTS);
  fflush(stdout);
}

static void emit_metric(const char *name, const char *pattern,
                        const uint32_t *samples) {
  uint32_t ordered[SAMPLES];
  memcpy(ordered, samples, sizeof(ordered));
  sort_samples(ordered);
  printf("CAL_RECORD {\"type\":\"metric\",\"name\":\"%s\","
         "\"memory\":\"iram\",\"access_pattern\":\"%s\","
         "\"operations_per_trial\":%u,\"bytes_per_operation\":0,"
         "\"ccount_samples\":[",
         name, pattern, OPS_PER_BLOCK);
  for (uint32_t index = 0; index < SAMPLES; ++index) {
    printf("%s%" PRIu32, index == 0 ? "" : ",", samples[index]);
  }
  printf("],\"cycles_per_op\":{\"min\":");
  print_cycles_per_op((uint64_t)ordered[0] * 2u);
  printf(",\"median\":");
  print_cycles_per_op((uint64_t)ordered[49] + ordered[50]);
  printf(",\"p90\":");
  print_cycles_per_op((uint64_t)ordered[89] * 2u);
  printf(",\"max\":");
  print_cycles_per_op((uint64_t)ordered[99] * 2u);
  printf("},\"distribution\":\"min-median-nearest-rank-p90-max\"}\n");
  fflush(stdout);
}

static uint32_t IRAM_ATTR __attribute__((noinline))
measure_probe_samples(probe_fn_t function, uint32_t *samples) {
  uint32_t accepted = 0;
  function(&benchmark_word);
  for (uint32_t attempt = 0;
       attempt < MAX_ATTEMPTS && accepted < SAMPLES; ++attempt) {
    clear_cache_counters();
    const uint32_t previous = mask_interrupts();
    const uint32_t start = read_ccount();
    function(&benchmark_word);
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

static void run_probe(const char *name, probe_fn_t function,
                      const char *pattern) {
  uint32_t samples[SAMPLES];
  const uint32_t accepted = measure_probe_samples(function, samples);
  if (accepted != SAMPLES) {
    emit_refusal(name);
  } else {
    emit_metric(name, pattern, samples);
  }
}

void app_main(void) {
  esp_chip_info_t chip = {0};
  esp_chip_info(&chip);
  const uint32_t cpu_hz = esp_rom_get_cpu_ticks_per_us() * 1000000u;
  printf("CAL_RECORD {\"type\":\"configuration\","
         "\"schema_version\":\"%s\",\"harness_version\":\"%s\","
         "\"idf_version\":\"%s\",\"target\":\"esp32s3\","
         "\"chip_revision\":%u,\"cores\":%u,\"cpu_hz\":%" PRIu32 ","
         "\"ccount_hz\":%" PRIu32 ",\"probe\":\"opcode-ladders\","
         "\"samples_per_cell\":%u,\"max_attempts_per_cell\":%u,"
         "\"operations_per_block\":%u}\n",
         SCHEMA_VERSION, HARNESS_VERSION, esp_get_idf_version(), chip.revision,
         chip.cores, cpu_hz, cpu_hz, SAMPLES, MAX_ATTEMPTS, OPS_PER_BLOCK);
  fflush(stdout);

  for (uint32_t index = 0; index < sizeof(probes) / sizeof(probes[0]); ++index) {
    const char *const name = probes[index].id;
    const probe_fn_t function = probes[index].function;
    const char *const pattern = probes[index].pattern;
    run_probe(name, function, pattern);
  }

  printf("CALIBRATION_DONE sink=%" PRIu32 "\n", benchmark_word);
  fflush(stdout);
  while (true) {
    vTaskDelay(pdMS_TO_TICKS(1000));
  }
}
