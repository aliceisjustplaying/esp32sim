#include <stdint.h>

#ifndef CYCLE_ACCOUNTING
#define CYCLE_ACCOUNTING 0
#endif

#define GUEST_SOURCE 0x3fca1000u
#define GUEST_DESTINATION 0x3fca2000u
#define DCACHE_LINES 512u
#define DCACHE_LINE_SHIFT 6u

static uint32_t ar[16];
static uint8_t guest_src[4096];
static uint8_t guest_dst[4096];
static uint32_t dcache_tag[DCACHE_LINES];
static uint64_t cycles;
static uint64_t cache_misses;
static uint32_t jit_pixels;

__attribute__((export_name("jit_setup")))
uint32_t jit_setup(uint32_t pixels) {
    if (pixels == 0 || pixels * 2u > sizeof(guest_src)) return 1;
    jit_pixels = pixels;
    for (uint32_t i = 0; i < sizeof(guest_src); i++) {
        guest_src[i] = (uint8_t)(i * 31u + 7u);
        guest_dst[i] = 0;
    }
    for (uint32_t i = 0; i < DCACHE_LINES; i++) dcache_tag[i] = 0xffffffffu;
    cycles = 0;
    cache_misses = 0;
    return 0;
}

static inline void dcache_access(uint32_t guest_address) {
#if CYCLE_ACCOUNTING
    uint32_t line = guest_address >> DCACHE_LINE_SHIFT;
    uint32_t index = line & (DCACHE_LINES - 1u);
    if (dcache_tag[index] != line) {
        dcache_tag[index] = line;
        cache_misses += 1;
    }
#else
    (void)guest_address;
#endif
}

__attribute__((export_name("jit_run")))
uint32_t jit_run(uint32_t iterations) {
    if (jit_pixels == 0 || iterations == 0) return 0;
    for (uint32_t call = 0; call < iterations; call++) {
        ar[10] = 0;
        ar[11] = 0;
        ar[12] = jit_pixels;
#if CYCLE_ACCOUNTING
        cycles += 3;
#endif
#pragma clang loop vectorize(disable) unroll(disable)
        while (ar[12] != 0) {
            uint32_t src_offset = ar[10];
            dcache_access(GUEST_SOURCE + src_offset);
            uint32_t low = guest_src[src_offset];
            ar[2] = low;
            uint32_t high = guest_src[src_offset + 1u];
            ar[3] = high;
            uint32_t swapped = (ar[2] << 8) | ar[3];
            ar[4] = swapped;
            uint32_t dst_offset = ar[11];
            dcache_access(GUEST_DESTINATION + dst_offset);
            guest_dst[dst_offset] = (uint8_t)ar[4];
            guest_dst[dst_offset + 1u] = (uint8_t)(ar[4] >> 8);
            ar[10] = src_offset + 2u;
            ar[11] = dst_offset + 2u;
            ar[12] -= 1u;
#if CYCLE_ACCOUNTING
            cycles += 8;
#endif
        }
    }
    return guest_dst[0] | ((uint32_t)guest_dst[1] << 8);
}

__attribute__((export_name("jit_cycles_lo")))
uint32_t jit_cycles_lo(void) {
    return (uint32_t)cycles;
}

__attribute__((export_name("jit_cycles_hi")))
uint32_t jit_cycles_hi(void) {
    return (uint32_t)(cycles >> 32);
}

__attribute__((export_name("jit_misses_lo")))
uint32_t jit_misses_lo(void) {
    return (uint32_t)cache_misses;
}

__attribute__((export_name("jit_misses_hi")))
uint32_t jit_misses_hi(void) {
    return (uint32_t)(cache_misses >> 32);
}

__attribute__((export_name("jit_dest")))
uint32_t jit_dest(void) {
    return (uint32_t)(uintptr_t)guest_dst;
}
