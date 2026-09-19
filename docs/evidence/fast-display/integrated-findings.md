# Integrated TinyDraw timing comparison

The integrated browser run passes all 36 automated gates. Across 15 paced-cold
cases, median modeled compute time is **0.749× hardware**, presentation is
**0.899× hardware** and total operation time is **0.792× hardware**.
These are firmware timer ratios, not browser throughput.
[Raw paired fields](integrated-hardware-comparison.json)

Configuration: CPI 1, scheduling quantum 64, data-cache fill/writeback 160/96,
inline cache hits, timed SPI2 and measured TE. Source run:
`work/fast-run/integrated-inline-full-v2/run` in the main checkout.

| Workload | Compute / hardware | Presentation / hardware | Total / hardware |
| --- | ---: | ---: | ---: |
| overlap zoom=50 | 0.754 | 0.878 | 0.777 |
| overlap zoom=100 | 0.739 | 0.886 | 0.792 |
| overlap zoom=200 | 0.724 | 0.846 | 0.764 |
| overlap zoom=400 | 0.709 | 0.847 | 0.780 |
| adversarial_tapered_4x+evil_hairlines zoom=50 | 0.756 | 0.903 | 0.786 |
| adversarial_tapered_4x+evil_hairlines zoom=100 | 0.749 | 0.899 | 0.783 |
| adversarial_tapered_4x+evil_hairlines zoom=200 | 0.748 | 0.902 | 0.773 |
| adversarial_tapered_4x+evil_hairlines zoom=400 | 0.754 | 0.920 | 0.773 |
| owner_torture zoom=50 | 0.754 | 0.882 | 0.820 |
| owner_torture zoom=100 | 0.754 | 0.885 | 0.825 |
| owner_torture zoom=200 | 0.758 | 0.971 | 0.841 |
| owner_torture zoom=400 | 0.756 | 0.933 | 0.799 |
| seed7 zoom=400 | 0.734 | 0.912 | 0.808 |
| evil_hairlines_capacity zoom=100 | 0.738 | 0.913 | 0.808 |
| evil_hairlines_capacity zoom=400 | 0.709 | 0.899 | 0.815 |

The smaller staging windows disagree in both directions:

| Window | Hardware µs | Integrated µs | Ratio |
| --- | ---: | ---: | ---: |
| Linear PIE | 12,640 | 10,612 | 0.840 |
| Linear scalar | 68,151 | 67,400 | 0.989 |
| Ring PIE | 165,815 | 166,760 | 1.006 |
| Ring scalar | 302,845 | 357,295 | 1.180 |
| Realistic 1000-operation load | 558,052 | 264,142 | 0.473 |
| Export | 9,606,592 | 6,732,817 | 0.701 |

**The strongest next target is the missing compute time.** Increasing every
instruction cost or every cache-fill price to fix the paced-cold deficit would
also increase the already accurate linear-scalar/ring-PIE windows and the
already slow ring-scalar window. This inference follows directly from the table;
it does not identify the missing hardware mechanism.

For the next bounded experiment, inspect instruction-class and fetch/load costs
inside the realistic-load window, whose deficit is largest, then check the same
change against a paced-cold compute window. The current load loop performs
floating-point multiplication/rounding followed by `append_and_absorb`:
`tinydraw/esp32/main/vector_v2/vector_v2_gate_harness.cpp:96-119`.
Paced-cold `compute_us` measures `producer.produce_next`:
`tinydraw/esp32/main/vector_v2/vector_v2_gate_harness_render.cpp:309-311`.
Instruction-fetch timing is absent from the integrated bus's `fetch` path
(`work/fast-integration/esp32s3/src/bus.rs:925`), and the cache's default hit
penalty is zero (`esp32s3/src/approximate_cache.rs:24` in that checkout).
These are candidate omissions to distinguish, not diagnosed causes.

Startup's measured transfer wait is already 18,966 us against hardware's
19,700 us (0.963×). Keep the wire model unchanged for this next experiment.
Startup TE wait is phase-dependent, so its single-call ratio should not be used
to adjust the measured TE period.

Caveat: one hardware boot restored 38 drawing operations while the browser
started with erased data partitions. Selected workload counts match, but this
does not prove complete initial-state equivalence. Repeated non-startup
`LIVE_PRESENT` markers are excluded to avoid pairing distinct calls by accident.
