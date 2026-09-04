# Browser JIT scheduler eligibility receipt — 2026-09-04

This receipt reruns the five-manifest browser JIT coverage probe after removing two blanket
scheduler refusals. A released secondary core is accepted only when it is already waiting without
a pending interrupt, and receives the same 64-cycle timing-only advance when the external quantum
commits. Script actions remain ordered at the existing end-of-quantum boundary.

Run from the repository root after `tools/wasm-build.sh`:

```sh
ESP32SIM_WASM_JIT_STATS=1 node tools/wasm-test.mjs hello atech atech-sid panel panel-sid
```

Each manifest again made 360 attempts. Idle-secondary-core and script refusals fell to zero, while
the firmware results stayed clean. No quantum committed because every scheduler-eligible stream
still encountered an unsupported instruction within the required 64-instruction sequence. The
panel workloads now expose the broadest sample, led by `S32iN` (19) and `Addi` (8), followed by
branches, calls/returns, register moves, and other ALU operations.

The next layer is instruction and control-flow coverage, beginning with word stores, common ALU
operations, and the branch shapes that prevent a 64-instruction sequence today. This remains a
reachability diagnostic, not a browser speed result. Counts are concurrent.

Raw counters, source identity, runtime, host, and WebAssembly hash are in `result.json`.
