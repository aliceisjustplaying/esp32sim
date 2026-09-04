# Costed native JIT SRAM throughput

This receipt measures the first product costed-JIT slice at commit
`97198c381725973b3c89543c8c1dc82e080042fc` on the target Apple M1 Pro.
Both emulated LX7 cores are active. Each runs the same 32-instruction IRAM
block: 31 receipt-priced `movi` instructions and one receipt-priced backward
`j`. The product `Machine` scheduler, shared time, recording bus, cost model,
canonical ledger construction, and native AArch64 JIT are all in the timed
path.

Run from the repository root:

```sh
cargo run --release -p esp32s3 --example costed_jit_speed --locked
```

The harness fails closed unless it is running on an arm64 Apple M1 Pro with
the native Xtensa JIT available. It performs 20,000 untimed warmup events,
then five fresh-machine samples of 200,000 events. Every sample verifies that
both cores retired exactly 100,000 instructions and that both cores compiled
the costed block in warmup and measurement runs. The median is used only to
decide the 480 MIPS milestone threshold; the short samples are sufficient
because the result is two orders of magnitude below that threshold.

The measured median is 4.385869 aggregate MIPS. It misses the 480 MIPS
dual-core worst-case budget by 475.614131 MIPS, or 99.086277 percent. The
current one-instruction modeled dispatch is therefore a correctness slice,
not a real-time product engine. The next performance work must execute priced
blocks under a scheduler cycle deadline while preserving shared-time event
boundaries.

Exact host, toolchain, workload, raw samples, calculations, command, and
harness hash are in `result.json`.
