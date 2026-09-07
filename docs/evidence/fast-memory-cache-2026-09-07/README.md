# Rough shared data-cache timing

Starting implementation: `esp32s3/src/approximate_cache.rs`.

The first model is a 32 KiB shared cache with 64-byte lines, write allocation,
writeback and configurable associativity (default four ways). Replacement is
round-robin. No allocation occurs during an access. Each access returns counts
of hits, fills and dirty evictions plus optional provisional cycle charges.
The memory-resource model can price those counts instead; do not charge both.

TinyDraw `esp32/sdkconfig.defaults:7-10` supplies capacity and line size.
Associativity and replacement are assumptions. Fill=120 and writeback=96 cycles
are initial sweep points, not calibrated constants. Both CPU data streams must
use one instance; instruction timing would use a separate instance.

Limitations: timing only, immediate functional writes, no MMU alias recognition,
no cache maintenance instruction behavior, no DMA coherence, prefetch or
concurrent misses. Only external cacheable accesses should reach this module.

## First check

```
rustc --test esp32s3/src/approximate_cache.rs -o cache-test
./cache-test
```

Three tests passed: sequential 32 KiB word scan produces 512 fills/7680 hits
and a repeated scan adds no fills; dirty replacement adds its configured cost;
an access crossing a line fills both lines and reset empties state.
These test model mechanics, not hardware accuracy or TinyDraw performance.

## Existing hardware evidence to use cautiously

Hardware worker extracted the canonical normal-boot archive at
`/Users/alice/Archives/esp32s3/2026-09-01-tier-b-decomposition/canonical-7a157d4/normal/`.
Its first-line path is 96 cycles for PSRAM and 128 for flash. The probe includes
`read_stride()` and timestamp instructions, so those are not isolated additional
cache miss penalties (`tinydraw/calibration/esp32s3-tier-b/main/tier_b_probe.cpp:583`).
Dirty minus clean explicit C2M writeback is approximately 154–158 cycles/64 B.
That probe calls `esp_cache_msync`, not an eviction instruction, so applying it
to dirty replacement is another explicit assumption (same file:666).

Next useful result: run the actual TinyDraw execution through this cache state,
compare against fixed per-access costs and report hit/fill/eviction counts as
well as workload progress. Do not infer whole-system accuracy from totals.
