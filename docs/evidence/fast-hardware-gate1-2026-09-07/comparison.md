# Next timing-model experiments from the hardware comparison

Prioritize **cache-dependent PSRAM cost and display timing**, rather than
uniformly doubling every instruction. The [numeric comparison](timing-comparison.json)
uses firmware timers from the hardware capture, browser baseline
`work/fast-run/cache-screen/1-baseline/events.json` and CPI2/quantum64
`work/fast-timed-jit/work/receipts/cpi2-q64-full.json`.

| Operation | Hardware µs | Baseline µs | CPI2 µs |
| --- | ---: | ---: | ---: |
| Ring PIE staging | 165815 | 25425 | 51346 |
| Ring scalar staging | 302845 | 210268 | 424732 |
| Hot linear PIE staging | 12640 | 10608 | 21421 |
| Hot linear scalar staging | 68151 | 67397 | 136136 |
| Fixed seed-7 document load | 558052 | 239758 | 484416 |
| SVG/PNG export | 9606592 | 4726624 | absent: preceding gate failed |

1. **Ring memory traffic is the largest clean mismatch.** The loop streams
   368 × 372 × 2 × 48 = 13,142,016 bytes from PSRAM to an internal DMA buffer.
   Hardware achieves an empirical 79.26 MB/s; baseline implies 516.89 MB/s.
   The hot linear case repeatedly accesses less than 1 KiB and already has
   scalar timing within 1.1%. Inference: charge cold/cache-miss traffic while
   keeping hot hits cheap. The observed rate includes loop and cache costs;
   it is not a theoretical bus ceiling. Octal PSRAM uses DTR.
2. **Presentation is still much too cheap.** Across 15 matching paced-cold
   records, median baseline/hardware ratios are 0.726 for compute and 0.284
   for presentation. CPI2 changes them to 1.466 and 0.574: compute overshoots
   while presentation remains underpriced. Inference: try DMA completion and
   measured TE scheduling independently of instruction issue costs.
3. **Ink timing changes the workload.** CPI2 brings fast-curve-400 display
   latency p95 from 2156 to 3842 µs toward hardware's 4260 µs, but consumed
   events are 411/392/380 and coalesced events are 3/22/34 respectively.
   Compare event accounting too; matching one latency alone is insufficient.
4. **Export needs its own comparison.** Baseline is 0.492 of hardware duration
   with matching output sizes and CRC. CPI2 skipped export after its long-gesture
   failure. Inference: compare encoder computation versus flash I/O separately
   before applying a global factor.

Hardware restored a drawing before the battery. Comparisons favor native
staging and later fixed fixtures. The JSON flags selected count differences,
and HARD records include operation/sample counts in their identity so an early
1039-operation hardware fixture is not paired with a 1001-operation simulator
fixture. This still does not establish complete initial-state equivalence.

Staging source: TinyDraw `esp32/main/vector_v2/vector_v2_gate_harness_kernels.cpp`,
lines 73–181. Ring source allocation is 273,792 bytes in external RAM, output
is eight 368-pixel rows in internal DMA RAM and each pass changes row wrapping.
No physical display transfer occurs inside that staging benchmark.
