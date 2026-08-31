# Continuous integration

`.github/workflows/ci.yml` defines the repository's required proof surfaces.
It pins Rust 1.98.0 and every action by full commit SHA, uses
`ubuntu-24.04`, and grants read-only repository permissions.

The jobs are:

- `CI policy`: rejects mutable action references and explicit network
  downloads without SHA-256 verification. Its self-test injects both
  defects before checking the real workflows.
- `Rust format`: runs `rustfmt --check` over every Rust file added or changed
  by this CI and conformance series.
- `Rust test`: runs `cargo test --workspace --locked`.
- `Rust clippy`: runs `cargo clippy --workspace --all-targets --locked`.
- `Decoder conformance`: always executes the committed Xtensa and RISC-V
  corpora with `--nocapture`, so counts and digests are visible in the
  required log.

The mandatory decoder corpora live under each CPU crate's
`tests/corpus/` directory. Their `.S` source and `.provenance` sidecars
pin the independent GNU objdump oracle, toolchain, generation command,
source digest, corpus digest, case count, and required mnemonic set.
`XTENSA_DIS_FILES` and `RISCV_DIS_FILES` can add larger local corpora.
Every path named by either variable must exist and contain at least one
parsed case.

The committed mandatory results are:

- Xtensa LX7: 10 cases, SHA-256
  `7e684d22347931c81770ddbea6c7fb1878542c8fecc0dc51340edcfc8b1c591f`.
- RISC-V RV32IMC: 9 cases, SHA-256
  `d6ee3d1719bd31eb878667c6b742d5597d50e73cc9bc75ce4ce1efade59933ab`.

The Pages workflow uses the pinned 20241011 ESP ROM archive and verifies
SHA-256
`921f000164a421c7628fbfee55b173384aafaa51883adc65cd27bf9b0af9e9a9`
before extraction.

## Rustfmt baseline debt

Whole-workspace formatting is not green at the upstream base. The exact
receipt is
[`receipts/2026-08-31-rustfmt-baseline.txt`](receipts/2026-08-31-rustfmt-baseline.txt).
At `main` commit `2114ffc92039b4605264d2cfb4ee5543acbf98c1`,
`cargo fmt --all -- --check` exits 1 with 893 diff hunks across 39 files.
This series does not rewrite those unrelated source files. The focused
format job prevents new formatting debt on every Rust file it touches.
