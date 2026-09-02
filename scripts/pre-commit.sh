#!/usr/bin/env bash
set -euo pipefail

cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.."

cargo fmt --all -- --check
cargo check --workspace --all-targets --all-features --locked
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo test --workspace --all-targets --all-features --locked
if [[ -n "${ESP32SIM_ROM_DIR:-}" ]]; then
    cargo test --workspace --release --locked -- --include-ignored
else
    cargo test --workspace --all-targets --all-features --release --locked
fi
RUSTDOCFLAGS="-D warnings" \
    cargo doc --workspace --all-features --no-deps --locked
