// ESP32-S3 mask-ROM instruction-fetch ladder.
//
// Ten straight-line mask-ROM functions (entry ... retw.n, no branch, load,
// store, call or loop) are each timed twice through one driver: once at their
// ROM address, once as a byte-identical copy in IRAM placed at the same address
// modulo 128. The driver, the call instruction, the register window state, the
// operands and the instruction bytes are identical for both cells of a pair;
// only the call target's memory differs. The per-pair difference is the whole-call
// ROM-minus-IRAM difference, including any target-dependent call and return
// effects, not an isolated fetch cost. No price is derived here.
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
#define RECURSION_DEPTH 1u
#define TWIN_ALIGN 128u
#define OPERAND_A 0x00000123u
#define OPERAND_B 0x00000010u

typedef uint32_t (*probe_fn_t)(uint32_t a, uint32_t b);

typedef struct {
  const char *name;
  uint32_t rom_address;
  uint32_t bytes;
  probe_fn_t twin;
} rom_cell_t;

// Build-time IRAM twins (rom_twins.S), byte-identical to the ROM bodies.
extern uint32_t twin_p_none(uint32_t, uint32_t);
extern uint32_t twin_abs(uint32_t, uint32_t);
extern uint32_t twin_mulsi3(uint32_t, uint32_t);
extern uint32_t twin_clzsi2(uint32_t, uint32_t);
extern uint32_t twin_temp_to_power(uint32_t, uint32_t);
extern uint32_t twin_roundup2(uint32_t, uint32_t);
extern uint32_t twin_ctzsi2(uint32_t, uint32_t);
extern uint32_t twin_ffssi2(uint32_t, uint32_t);
extern uint32_t twin_clzdi2(uint32_t, uint32_t);
extern uint32_t twin_ctzdi2(uint32_t, uint32_t);

// Exact ESP32-S3 rev 0 mask-ROM bodies; see rom-cells.json and README.md.
static const rom_cell_t ROM_CELLS[] = {
    {"p_none", 0x400559a4u, 5u, twin_p_none},
    {"abs", 0x4002e314u, 8u, twin_abs},
    {"mulsi3", 0x40056080u, 8u, twin_mulsi3},
    {"clzsi2", 0x400560a8u, 8u, twin_clzsi2},
    {"temp_to_power", 0x40055c70u, 11u, twin_temp_to_power},
    {"roundup2", 0x40056064u, 15u, twin_roundup2},
    {"ctzsi2", 0x400560b0u, 20u, twin_ctzsi2},
    {"ffssi2", 0x400560c4u, 20u, twin_ffssi2},
    {"clzdi2", 0x4005636cu, 20u, twin_clzdi2},
    {"ctzdi2", 0x40056380u, 30u, twin_ctzdi2},
};
#define ROM_CELL_COUNT (sizeof(ROM_CELLS) / sizeof(ROM_CELLS[0]))

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
                        const uint32_t *samples) {
  printf("CAL_RECORD {\"type\":\"metric\",\"name\":\"%s\","
         "\"memory\":\"%s\",\"access_pattern\":\"straight-line\","
         "\"operations_per_trial\":1,\"bytes_per_operation\":0,"
         "\"ccount_samples\":[", name, memory);
  for (uint32_t index = 0; index < SAMPLES; ++index) {
    printf("%s%" PRIu32, index == 0 ? "" : ",", samples[index]);
  }
  printf("],\"cache_counters_required_zero\":true}\n");
  fflush(stdout);
}

// One driver for every cell. The ROM cell and its IRAM twin differ only in the
// value of `function`; the instructions executed around the call are the same.
static uint32_t IRAM_ATTR __attribute__((noinline))
measure_probe_samples(probe_fn_t function, uint32_t *samples) {
  uint32_t accepted = 0;
  benchmark_sink += function(OPERAND_A, OPERAND_B);
  for (uint32_t attempt = 0;
       attempt < MAX_ATTEMPTS && accepted < SAMPLES; ++attempt) {
    clear_cache_counters();
    const uint32_t previous = mask_interrupts();
    const uint32_t start = read_ccount();
    const uint32_t result = function(OPERAND_A, OPERAND_B);
    const uint32_t end = read_ccount();
    restore_interrupts(previous);
    benchmark_sink += result;
    const cache_counters_t counters = read_cache_counters();
    const uint32_t elapsed = end - start;
    if (elapsed != 0u && counters_zero(counters)) {
      samples[accepted++] = elapsed;
    }
  }
  return accepted;
}

static void run_cell(const char *name, const char *memory, probe_fn_t function) {
  uint32_t samples[SAMPLES];
  const uint32_t accepted = measure_probe_samples(function, samples);
  if (accepted == SAMPLES) {
    emit_metric(name, memory, samples);
  } else {
    emit_refusal(name);
  }
}

// Check the build-time IRAM twin against the ROM body word for word (reads
// only; IRAM accepts 32-bit accesses) and print both placements.
static bool check_twin(const rom_cell_t *cell) {
  const uint32_t rom = cell->rom_address;
  const uint32_t twin = (uint32_t)(uintptr_t)cell->twin;
  if (rom % TWIN_ALIGN != twin % TWIN_ALIGN) {
    printf("CALIBRATION_FAILED twin %s alignment rom=0x%08" PRIx32 " twin=0x%08" PRIx32 "\n",
           cell->name, rom, twin);
    fflush(stdout);
    return false;
  }
  const uint32_t first_word = rom & ~3u;
  const uint32_t last_word = (rom + cell->bytes + 3u) & ~3u;
  const uint32_t words = (last_word - first_word) / 4u;
  const volatile uint32_t *source = (const volatile uint32_t *)(uintptr_t)first_word;
  const volatile uint32_t *copy = (const volatile uint32_t *)(uintptr_t)(twin & ~3u);
  const uint32_t lead = rom & 3u;
  const uint32_t tail = (rom + cell->bytes) & 3u;
  for (uint32_t index = 0; index < words; ++index) {
    uint32_t mask = 0xffffffffu;
    if (index == 0 && lead != 0) mask &= 0xffffffffu << (8u * lead);
    if (index + 1 == words && tail != 0) mask &= 0xffffffffu >> (8u * (4u - tail));
    if ((copy[index] & mask) != (source[index] & mask)) {
      printf("CALIBRATION_FAILED twin %s word %" PRIu32 " mismatch\n", cell->name, index);
      fflush(stdout);
      return false;
    }
  }
  printf("ROM_TWIN name=%s rom=0x%08" PRIx32 " twin=0x%08" PRIx32 " bytes=%" PRIu32
         " words=%" PRIu32 " data=", cell->name, rom, twin, cell->bytes, words);
  for (uint32_t index = 0; index < words; ++index) {
    printf("%08" PRIx32, source[index]);
  }
  printf("\n");
  fflush(stdout);
  return true;
}

void app_main(void) {
  // Host-attach delay: the capture host reopens the USB serial/JTAG port after
  // the reset that starts this boot; nothing timed happens during the wait.
  vTaskDelay(pdMS_TO_TICKS(1000));
  esp_chip_info_t chip = {0};
  esp_chip_info(&chip);
  const uint32_t cpu_hz = esp_rom_get_cpu_ticks_per_us() * 1000000u;
  printf("CAL_RECORD {\"type\":\"configuration\","
         "\"schema_version\":\"1.0.0\",\"harness_version\":\"1.3.0\","
         "\"idf_version\":\"%s\",\"target\":\"esp32s3\","
         "\"chip_revision\":%u,\"cores\":%u,\"cpu_hz\":%" PRIu32 ","
         "\"ccount_hz\":%" PRIu32 ",\"probe\":\"rom-fetch-ladder\","
         "\"samples_per_cell\":%u,\"max_attempts_per_cell\":%u,"
         "\"recursion_depth\":%u}\n", esp_get_idf_version(), chip.revision,
         chip.cores, cpu_hz, cpu_hz, SAMPLES, MAX_ATTEMPTS, RECURSION_DEPTH);
  fflush(stdout);

  char rom_name[32];
  char iram_name[32];
  for (uint32_t index = 0; index < ROM_CELL_COUNT; ++index) {
    const rom_cell_t *cell = &ROM_CELLS[index];
    if (!check_twin(cell)) {
      return;
    }
    snprintf(rom_name, sizeof rom_name, "rom_%s", cell->name);
    snprintf(iram_name, sizeof iram_name, "iram_%s", cell->name);
    run_cell(rom_name, "rom", (probe_fn_t)(uintptr_t)cell->rom_address);
    run_cell(iram_name, "iram", cell->twin);
  }

  printf("CALIBRATION_DONE sink=%" PRIu32 "\n", benchmark_sink);
  fflush(stdout);
  while (true) {
    vTaskDelay(pdMS_TO_TICKS(1000));
  }
}
