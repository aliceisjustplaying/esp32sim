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

The next implementation boundary is therefore admitting a released second core only when it is
provably idle, plus bounding scripts by their next deadline. Opcode expansion becomes meaningful
after those permanent gates stop masking the hot code. Counts are concurrent, so refusal totals
can exceed attempt totals.

Raw counters, source identity, runtime, host, and WebAssembly hash are in `result.json`.
