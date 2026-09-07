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
cargo test -p esp32s3 approximate_cache
```

The initial standalone revision passed three tests: sequential 32 KiB word scan produces 512 fills/7680 hits
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

## TinyDraw startup result

`receipts/run-tinydraw.sh --max-insns 100000000` loads the frozen browser
performance firmware from ROM with 16 MiB flash and 8 MiB PSRAM. Both runs
reached `TINYDRAW_LIVE_PRESENT kind=startup ... pass=1` with zero reported
memory faults. This is startup presentation, not completion of drawing gates.

| Measurement | Fill/writeback=0/0 | Fill/writeback=120/96 |
| --- | ---: | ---: |
| Startup compose, guest microseconds | 15,402 | 22,742 |
| Startup transfer wait, guest microseconds | 13,035 | 16,730 |
| Startup TE wait, guest microseconds | 12,752 | 8,980 |
| External data accesses | 16,757,213 | 13,228,913 |
| Cache hits | 16,019,811 | 12,602,321 |
| Line fills | 737,402 | 626,592 |
| Dirty evictions | 169,595 | 169,739 |
| Added provisional cycles | 0 | 91,485,984 |

Sources: `receipts/cache-zero-100m.log` and `receipts/cache-100m.log`.
Zero-price reproduction:

```
ESP32SIM_CACHE_FILL=0 ESP32SIM_CACHE_WRITEBACK=0 receipts/run-tinydraw.sh --max-insns 100000000
```

The nonzero model changes startup composition time and when firmware reaches
TE waits. The models also execute different mixes of work within the same
100-million-event limit; totals are not an equal-work speed comparison.
Native host runs took about 11–12 seconds amid other work, so no isolated
performance claim is made.

The next experiment is already handed to the production-JIT worker: bus helpers
update the same cache state, return accumulated memory penalties and disable
direct memory access initially. Its physical keys distinguish flash and PSRAM
and recognize aliases. CPU and DMA helper calls are not separated, so external
DMA buffers would incorrectly enter the CPU cache in that first adapter.
TinyDraw's internal DMA staging is the intended initial workload. Neither this
adapter nor the interpreter model handles actual cache maintenance semantics.

## Inline WASM cache hits

Build the WASM crate with `--features cache-inline`. Cache mode 3 in
`tools/approximate-jit-smoke.mjs` enables the prototype; mode 2 keeps the
selective external-helper reference. Four physical tag comparisons precede
the existing direct load/store. Hits increment the counter and stores mark
the matching line dirty. Misses execute the original helper, preserving
replacement and miss charging. PIE vector loads/stores count four word hits,
matching the helper. Existing code-page version updates remain in place.
Other cache geometries or nonzero hit prices cannot enable this fast path.

The same three-second TinyDraw run produced exactly matching guest cycles,
563,683,376 instructions, 20,238,659 hits, 751,873 fills, 171,502 writebacks
and 106,688,952 added cycles in both modes. Panel-stage times, checksums and
guards also matched. The Node host runs took 7.336 seconds with helpers and
6.223 seconds with inline hits; these were not isolated browser measurements.
Sources: `receipts/helper-cache-3s.json`, `receipts/inline-cache-3s.json`.

The feature plus `jit-tests` passed 42,014 actual WASM cases, including ten new
cases covering each cache way, misses, dirty write hits and version updates.
Source: `receipts/inline-cache-jit-tests.txt`.

The full TinyDraw gate subsequently completed with a valid, passing
`tinydraw-gate1-v1` verdict and `TINYDRAW_VECTOR_V2_GATE_HARNESS_DONE pass=1`.
It accumulated 634,546,683 hits, 11,483,553 fills, 5,493,314 writebacks and
1,905,384,504 provisional extra cycles over 55.451115808 guest seconds.
No generated modules failed compilation. This establishes functional gate
completion with the rough model, not hardware timing accuracy.
Source: `receipts/inline-cache-fullgate.json`.

A short parameter probe with `CACHE_FILL=160` kept the panel-stage checksums
and guards passing. Ring PIE staging changed from 131,268 to 166,673 guest
microseconds, linear scalar stayed at 67,397 and linear PIE at 10,618.
The parameter changes cold streaming much more than the small hot workload.
Source: `receipts/inline-cache-fill160-3s.json`. This one-workload sweep does
not establish a universal memory latency or a calibrated hardware model.
