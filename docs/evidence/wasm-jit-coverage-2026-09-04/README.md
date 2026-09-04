# Browser JIT coverage receipt — 2026-09-04

This receipt measures whether the guarded browser JIT handoff is reached by five real ESP32-S3
firmware manifests. It does not measure browser speed. Each manifest runs for three emulated
seconds under Node and offers the JIT one scheduling quantum at each 2 M-cycle driver boundary.

Run from the repository root after `tools/wasm-build.sh`:

```sh
ESP32SIM_WASM_JIT_STATS=1 node tools/wasm-test.mjs hello atech atech-sid panel panel-sid
```

Each of the five runs made 360 attempts and committed zero JIT quanta. An idle released second
core blocked 261–358 attempts per workload, while an active second core blocked 1–97. The `atech`,
`atech-sid`, and `panel-sid` manifests also carried a script refusal for all 360 attempts. Core 0
was waiting at most boundaries; the few eligible instruction streams first encountered `J`,
`Addi`, `AddiN`, `Bbci`, or `BnezN`.

Core 0 waiting is the binding constraint at these sampling instants. Even removing every other
refusal could admit at most 2, 3, 6, 56, and 56 of the 360 attempts for `hello`, `atech`,
`atech-sid`, `panel`, and `panel-sid`, respectively: 123 of 1,800 attempts. Idle-peer and script
admission can expose those remaining instruction streams, but cannot make a waiting core execute.
The per-bit counts overlap, so they do not identify which gate is the sole blocker.

The next performance step is dispatch inside the normal scheduler, where runnable work is
actually selected, followed by profiling and compiling its hot blocks. This receipt measures
eligibility at 360 sampling instants, not the fraction of workload instructions compiled. With
one 64-instruction quantum per sampled boundary, even 100% admission at these 360 instants would
cover only 23,040 instructions. The attempt count changes once commits cause the driver to retry
without an interpreter slice. Counts are concurrent, so refusal totals can exceed attempt totals.

The historical counters below are unchanged. Their original profiling run suppressed the
fallback console assertion for manifests without `expect`; the corrected harness always checks
console output, the two Atech manifests now name their expected transport line, and `panel-sid` expects the
same application-start line as `panel`.

Raw counters, source identity, runtime, host, and WebAssembly hash are in `result.json`.
