# Costed JIT block-batching refutation

This exploratory prototype grouped a dual-core, 32-instruction IRAM workload into one model call and one compact ledger transaction per pair of native blocks. It cached the validated price template and coalesced repeated compact ledger records.

The median was 15.548775 aggregate MIPS on an Apple M1 Pro. That is 3.5452 times the 4.385869 MIPS single-instruction baseline, but remains 96.7607 percent below the 480 MIPS dual-core budget. Meeting the budget would require another 30.8706 times speedup. The prototype added 1,003 lines and removed 20 lines, so this direction was stopped and was not committed as product code.

The measurement ran from base `7f1893913d0e374bb3267df46e4c2ff8f8c145e4`. The retained uncommitted binary diff, after a comment-only clippy fix, had SHA-256 `4eacf769b47eea037100bce2ddf4c1e6773417cb9e0c7a87b6bf0eba8b01bace`. The working tree was retained for review when this note was written.

Command:

```sh
cargo run --release -p esp32s3 --example costed_jit_speed --locked
```

The next performance design needs a fixed-size prepared token, one native execution, and a post-verification template commit. It also needs to avoid hot-path per-instruction planning and allocation. That design is separate work, not an extension of this prototype.
