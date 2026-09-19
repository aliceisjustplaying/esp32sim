# TinyDraw experiment results — September 7, 2026

The round exercised 23 idea families: the 12 original ideas and 11 follow-ups. This is not 23 independent comparisons: helper-table and resumption changes were tested together, and shared-resource modeling covered CPU cache misses rather than the proposed CPU/DMA model. No experiments remain running. One additional negative-coverage-cache implementation is saved but unrun and excluded from the count.

The final integrated regression passed all 36 checks with zero JIT failures: 60.158 simulated seconds in 125.794 host seconds, or 0.478× modeled real-time. This is a functional regression run, not a fresh isolated speed comparison. [Raw summary](/Users/alice/src/a/esp32sim/work/fast-run/integrated-v4-regression/summary.json).

Earlier paired firmware timers put median cold presentation at 0.899× hardware and compute at 0.749× hardware. Baseline presentation was about 0.284× hardware. The cache/display combination improves agreement, but neither real-time nor 1% accuracy was achieved. These comparisons have initial drawing-state differences and do not establish complete state equivalence. [Integrated comparison](/Users/alice/src/a/esp32sim/work/fast-integration/docs/evidence/fast-display/integrated-findings.md) · [Baseline comparison](/Users/alice/src/a/esp32sim/work/fast-integration/docs/evidence/fast-display/README.md).

PIE refers to the ESP32-S3 vector instruction extension used in TinyDraw's pixel processing. FP means floating point. The dependency prototypes model how long a later instruction waits for an earlier instruction's result.

| # | Experiment family | Outcome and scope | Receipt |
| --- | --- | --- | --- |
| 1 | Interpreted firmware timing | Boots and draws; provisional costs, native only. | [Evidence](/Users/alice/src/a/esp32sim/work/fast-timed-interpreter/docs/evidence/approximate-timing-2026-09-07/README.md) |
| 2 | Production JIT timing | Integrated CPI 1 passes; uniform CPI 2 fails timing gates and disagrees with hardware. | [Evidence](/Users/alice/src/a/esp32sim/work/fast-integration/docs/evidence/fast-hardware-gate1-2026-09-07/timing-comparison.json) |
| 3 | Execution boundaries | Integrated boundary scheduling passes; first-priced-miss yield also exists as an optional prototype. | [Evidence](/Users/alice/src/a/esp32sim/work/fast-integration/esp32s3/src/bus.rs) |
| 4 | Fixed memory costs | Startup sweeps ran; retained as a comparison, not calibrated. | [Evidence](/Users/alice/src/a/esp32sim/work/fast-memory-fixed/receipts/memory-summary.json) |
| 5 | Data-cache hit/miss timing | Integrated rough cache changes workload timing and passes the full battery. | [Evidence](/Users/alice/src/a/esp32sim/work/fast-integration/docs/evidence/fast-memory-cache-2026-09-07/README.md) |
| 6 | Shared resource contention | CPU cache-fill contention exercised. Planned progressive DMA contention remains unimplemented. | [Evidence](/Users/alice/src/a/esp32sim/work/fast-integration/esp32s3/src/rough_memory.rs) |
| 7 | Timed SPI2 completion | Integrated delayed completion, ownership and pixel delivery. | [Evidence](/Users/alice/src/a/esp32sim/work/fast-integration/docs/evidence/fast-display/README.md) |
| 8 | Measured display synchronization | Integrated measured TE period and pulse width. | [Evidence](/Users/alice/src/a/esp32sim/work/fast-integration/docs/evidence/fast-display/README.md) |
| 9 | Hardware probes and drawing | Physical memory, SPI2 and frozen drawing measurements saved. | [Evidence](/Users/alice/src/a/esp32sim/work/fast-hardware/docs/evidence/fast-hardware-2026-09-07/README.md) |
| 10 | Boxed code cache | 6.18% lower browser wall time in one isolated pair; promising screen, needs confirmation. | [Evidence](/Users/alice/src/a/esp32sim/work/fast-run/cache-screen/summary.json) |
| 11 | Shared helper table | Tested together with faster resumption: effectively flat; discarded. | [Evidence](/Users/alice/src/a/esp32sim/work/fast-speed-dispatch/docs/evidence/fast-dispatch-2026-09-07/README.md) |
| 12 | Faster block resumption | Same combined test as row 11; no independent attribution. | [Evidence](/Users/alice/src/a/esp32sim/work/fast-speed-dispatch/docs/evidence/fast-dispatch-2026-09-07/README.md) |
| 13 | Interpreter access-buffer reuse | About 26% lower native wall time in a nonisolated ABBA screen; integrated, no browser claim. | [Evidence](/Users/alice/src/a/esp32sim/work/fast-timed-interpreter/docs/evidence/approximate-timing-2026-09-07/README.md) |
| 14 | Inline cache-hit checks | Matching short-run counters and full battery pass; integrated. Node screen improved, browser gain unisolated. | [Evidence](/Users/alice/src/a/esp32sim/work/fast-integration/docs/evidence/fast-memory-cache-2026-09-07/README.md) |
| 15 | Hoisted bounds checks | 0.39% lower browser wall time in one pair; flat, parked. | [Evidence](/Users/alice/src/a/esp32sim/work/fast-run/bounds-screen/summary.json) |
| 16 | Direct region self-loop | 1.52% lower browser wall time in one pair; weak signal, parked. | [Evidence](/Users/alice/src/a/esp32sim/work/fast-speed-dispatch/docs/evidence/fast-dispatch-2026-09-07/README.md) |
| 17 | Remove cache-hit telemetry | 120.92 versus 121.06 host seconds; flat, parked. | [Evidence](/Users/alice/src/a/esp32sim/work/fast-run/speed-v3-nohits/summary.json) |
| 18 | Contiguous cache tags and SIMD comparison | 0.47% slower in one browser screen; parked. | [Evidence](/Users/alice/src/a/esp32sim/work/fast-speed-dispatch/docs/evidence/fast-dispatch-2026-09-07/README.md) |
| 19 | Instruction-cache timing | Native exploratory model and workload evidence; not integrated. | [Evidence](/Users/alice/src/a/esp32sim/work/fast-timed-interpreter/docs/evidence/approximate-timing-2026-09-07/icache.md) |
| 20 | Requested-word readiness versus line service | Short parameter sweep completed; optional plumbing integrated, not a fitted model. | [Evidence](/Users/alice/src/a/esp32sim/work/fast-integration/work/receipts/split-ready-summary.json) |
| 21 | PIE instruction and dependency timing | Fixed-cost sensitivity tested; measured dependency prototype parked. Latest mask extension has directed tests but no full TinyDraw validation. | [Evidence](/Users/alice/src/a/esp32sim/work/fast-memory-cache/docs/evidence/fast-pie-scoreboard-2026-09-07/README.md) |
| 22 | Floating-point dependency timing | Interpreter and JIT prototypes validated; JIT load probe closes 6.1% of its timing deficit. Full FP battery deferred. | [Evidence](/Users/alice/src/a/esp32sim/work/fast-timed-jit/docs/evidence/fp-scoreboard-2026-09-07/README.md) |
| 23 | Control-flow census | Counted branches and calls in the load window; simple illustrative prices explain only part of the gap. | [Evidence](/Users/alice/src/a/esp32sim/work/fast-timed-jit/docs/evidence/control-census-2026-09-07/README.md) |

Validated integrated work is saved on `codex/fast-integration`, implementation head `d13b7f93`. FP and measured PIE dependency prototypes remain in their experiment branches. Keep the boxed-cache candidate for a repeat browser comparison and preserve the validated timing plumbing. Park the flat speed variants; the remaining compute deficit still needs an explanation.

The unrun negative-coverage cache is commit `9cebb15c` in `work/fast-coverage-miss`. It has no performance or correctness result.

The board's original 16 MiB image was restored and explicitly verified against the backup. It booted its original calibration firmware through `CALIBRATION_DONE`; that hammer workload emitted CPU1 watchdog warnings. [Full-flash verification](/Users/alice/src/a/esp32sim/work/fast-hardware/docs/evidence/fast-restoration-2026-09-07/verify-original.log) · [Restored boot](/Users/alice/src/a/esp32sim/work/fast-hardware/docs/evidence/fast-restoration-2026-09-07/restored-original-boot.log).
