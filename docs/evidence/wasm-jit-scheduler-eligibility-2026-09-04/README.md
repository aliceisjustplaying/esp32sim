# Browser JIT scheduler eligibility receipt — 2026-09-04

This receipt reruns the five-manifest browser JIT coverage probe after removing two blanket
scheduler refusals. A released secondary core is accepted only when it is already waiting without
a pending interrupt, and receives the same 64-cycle timing-only advance when the external quantum
commits. Script actions remain ordered at the existing end-of-quantum boundary.

Run from the repository root after `tools/wasm-build.sh`:

```sh
ESP32SIM_WASM_JIT_STATS=1 node tools/wasm-test.mjs hello atech atech-sid panel panel-sid
```

Each manifest again made 360 attempts. `atech` increased from 0 to 2 attempts reaching decode,
and `panel` from 5 to 55, capturing nearly all their respective upper bounds of 3 and 56 attempts
where core 0 was not waiting. The panel workloads expose the broadest instruction sample, led by
`S32iN` (19) and `Addi` (8), followed by branches, calls/returns, register moves, and other ALU
operations. Idle-secondary-core and script counters are zero by construction after removing those
refusal bits; their disappearance is not independent evidence of correctness or speed.

No quantum committed. This receipt exercises admission and decode, but does not exercise the new
secondary-core advancement path; scheduler differential tests must establish that behavior.
Further removal of scheduler refusals cannot make waiting cores execute. Workload performance
needs dispatch within the ordinary scheduler and a profile of runnable hot blocks, with opcode
expansion guided by that profile. This remains a reachability diagnostic, not a browser speed
result. Counts are concurrent.

The historical counters are unchanged. The original profiling run suppressed fallback console
assertions, as documented in the preceding coverage receipt; the corrected harness always checks
the manifest's expected console output.

Raw counters, source identity, runtime, host, and WebAssembly hash are in `result.json`.
