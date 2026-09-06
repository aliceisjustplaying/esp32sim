# Timed two-core browser experiment

This is a bounded execution experiment, not a new TinyDraw execution mode. It tests whether generated WebAssembly can preserve the existing measured scheduler's behavior and measures the resulting browser speed. The production scheduler and default compiler are unchanged; the experimental module requires `scheduled-experiment`.

Two LX7 programs loop over shared, read-only internal SRAM. Core 0 has a dependent load/use pair, so its loop costs nine cycles; core 1's loop costs eight. Each loop executes six instructions, including a real backward `j`. A synthetic board device changes GPIO 0 at a declared deadline. A separate case starts with an unpriced store on core 0 and a competing read on core 1.

The reference uses the real `Machine`, LX7 interpreter, ESP32-S3 bus and `Esp32S3SramCostModel`. A test SoC keeps both cores enabled and initializes them at the fixture entry points on reset. This bypasses firmware boot, not instruction execution or scheduling. The reference also records the synthetic device edge through the normal board and GPIO observer interfaces.

The emitted module performs core selection, instruction execution, cycle accounting and the synthetic device transition inside one WebAssembly function. It chooses the earliest ready core with core-index ties, commits effects at instruction start and settles device time after each accepted instruction. Costs are generated from the existing model and checked component-for-component against its runtime ledger. Each fixture PC has one predecessor, so load-use costs are statically specialized; this is not a general solution for arbitrary block entries or runtime-dependent prices. The SRAM base is guarded at execution.

Verification compares registers, PCs, CCOUNT, instruction counts, per-core ready times, shared SRAM, device state, device-edge timestamp and the complete ordered instruction trace. It covers four deadlines, three data patterns, whole and split runs, tracing enabled and disabled, a 1,000-event case and sticky refusal. A deadline falls inside a two-cycle instruction. Single-event splits separate a load from its dependent use.

Four intentionally broken modules must fail: reversed core ties, late deadline delivery, omitted load-use cost and reversed ties before the unpriced store. In the last case the wrong order changes a guest register, not merely the trace. The unpriced store must retain its memory/PC/CCOUNT effects, add no priced cycles and prevent further execution. Read-only cases do not establish guest-visible shared-write ordering; the store case covers only refusal behavior. The device is synthetic and establishes scheduler behavior, not a new silicon timing claim.

Run from the isolated worktree:

```sh
mkdir -p target/test-tmp
TMPDIR="$PWD/target/test-tmp" cargo run -p esp32sim-wasm-jit --features scheduled-experiment --example timed_pair -- work/timed-pair
node wasm-jit/examples/timed_pair/runner.mjs work/timed-pair
node wasm-jit/examples/timed_pair/browser.mjs work/timed-pair work/timed-pair/confirm-1
```

The browser harness validates first, then measures five one-billion-event samples in a dedicated headless Chrome process. Each sample includes host chunk calls, generated dispatch and guards, shared-time arbitration, device handling, register/memory updates and timing counters. The diagnostic trace is disabled during timing. Each long sample's final state is checked against a derived expectation using the short native reference's register/PC phases and the loop's cycle patterns; there is no independent billion-event reference run. Compilation/instantiation is reported separately, and state initialization occurs before each timed sample. This is steady execution throughput, not startup or end-to-end product latency. No comparison with the production unmodeled JIT is claimed.

Real-time ratio uses the shared device horizon divided by 240 MHz, not the sum of both cores' cycles. Both cores remain busy here. The shared-horizon definition also applies when a core idles, but the relationship between instruction throughput and real-time speed changes. The result is specific to this small SRAM workload and one device transition. It omits full-device execution, cache behavior, general register windows, exception handling and full-firmware dispatch. A passing fixture does not establish full-product accuracy or speed.

Each browser result records Chrome/V8 versions, host details, source and module hashes, load averages, raw samples and final state. Only processes launched by the harness are closed. The first 10-million-event timing screen is retained separately; its roughly 30 ms samples are not confirmation evidence.
