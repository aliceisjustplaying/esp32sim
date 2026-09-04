# Costed JIT block-batching refutation

This exploratory prototype grouped a dual-core, 32-instruction IRAM workload into one model call and one compact ledger transaction per pair of native blocks. It cached the validated price template and coalesced repeated compact ledger records.

The median was 16.238929 aggregate MIPS on an Apple M1 Pro. That is 3.70256 times the 4.385869 MIPS single-instruction baseline, but remains 96.616890 percent below the 480 MIPS dual-core budget. Meeting the budget would require another 29.5586 times speedup. The prototype added 1,003 lines and removed 20 lines, so this direction was stopped and was not accepted as product code.

The measurement completed at `2026-09-04T10:16:39Z` on source commit `618c6bff69556aca46a2237b0e72990be8220c4e`, branch `codex/refuted-costed-jit-block-batching`, based on `7f1893913d0e374bb3267df46e4c2ff8f8c145e4`. The harness SHA-256 was `747296968b7645e3fd164f2713a0bcc8d6842e6bda67094128a2324b359d4e4f`.

Toolchain: Rust 1.98.0 (`88d9e12ae178fab0fb5cc050a94da85685d449ea`), Cargo 1.98.0 (`797e8a9bc`), LLVM 22.1.8, host `aarch64-apple-darwin`.

Command:

```sh
cargo run --release -p esp32s3 --example costed_jit_speed --locked
```
