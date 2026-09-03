#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
receipt_dir="$repo_root/docs/evidence/native-speed-2026-09-03"
output_dir="${1:?pass a new empty output directory}"
build_root="${TINYDRAW_VECTOR_V2_BUILD:?TINYDRAW_VECTOR_V2_BUILD is required}"
rom_elf="${ESP32S3_ROM_ELF:?ESP32S3_ROM_ELF is required}"

verify_hash() {
  local expected="$1"
  local path="$2"
  local actual
  actual="$(shasum -a 256 "$path")"
  actual="${actual%% *}"
  if [[ "$actual" != "$expected" ]]; then
    echo "SHA-256 mismatch for $path: expected $expected, found $actual" >&2
    exit 2
  fi
}

verify_hash 634e8dfab00aaa24c8b4514aecd77d842d5a49438baca87abf5f3a35e474b5ab \
  "$build_root/bootloader/bootloader.bin"
verify_hash f53268312c8caffe6c7f4e6c66d4092aeca3435c142db3116466f84a6a608d2d \
  "$build_root/partition_table/partition-table.bin"
verify_hash 1352e0c415aac2050b8159a7d7deae82f74f5f4202b9bbf000fefd0bc3573936 \
  "$build_root/tinydraw_esp32.bin"
verify_hash 9cb651e09a5405bc68fa5aa4656a22977e1c54f3198cb86bd5bc9753ba1d251b \
  "$build_root/tinydraw_esp32.elf"
verify_hash c0ce0f338d1de1bdc6efbef1591779a2a42c1ab7d759d3c6ae8ae63a7dd34cfd \
  "$rom_elf"

if [[ "$(rustc --version)" != "rustc 1.98.0 (88d9e12ae 2026-08-18)" ]]; then
  echo "native READY receipt requires rustc 1.98.0 (88d9e12ae 2026-08-18)" >&2
  exit 2
fi
if [[ "$(cargo --version)" != "cargo 1.98.0 (797e8a9bc 2026-08-05)" ]]; then
  echo "native READY receipt requires cargo 1.98.0 (797e8a9bc 2026-08-05)" >&2
  exit 2
fi
if [[ -d "$output_dir" && -n "$(find "$output_dir" -mindepth 1 -print -quit)" ]]; then
  echo "output directory must be empty: $output_dir" >&2
  exit 2
fi

mkdir -p "$output_dir"
cargo build --release --locked --manifest-path "$repo_root/Cargo.toml" -p esp32sim
verify_hash 370756ec52460a9a5ac29fa5cb67817569ce1e3e095d97f4f339042ce8ab4dde \
  "$repo_root/target/release/esp32sim"

for run in 1 2 3 4 5; do
  /usr/bin/time -p "$repo_root/target/release/esp32sim" \
    --rom "$rom_elf" \
    --boot rom \
    --bootloader "$build_root/bootloader/bootloader.bin" \
    --ptable "$build_root/partition_table/partition-table.bin" \
    --app "$build_root/tinydraw_esp32.bin" \
    --elf "$build_root/tinydraw_esp32.elf" \
    --board waveshare-amoled18-v2 \
    --flash-mb 16 \
    --psram-mb 8 \
    --console usb \
    --no-dump \
    --max-insns 200000000 \
    >"$output_dir/run-$run.console.txt" \
    2>"$output_dir/run-$run.stderr.txt"
done

python3 "$receipt_dir/analyze.py" "$output_dir"
