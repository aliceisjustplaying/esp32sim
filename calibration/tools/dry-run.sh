#!/bin/bash
set -euo pipefail

if [ "$#" -ne 2 ]; then
  echo "usage: $0 <image-dir> <build-dir>" >&2
  exit 2
fi

image_dir=$(cd "$1" && pwd)
build_dir=$(cd "$2" && pwd)
script_dir=$(cd "$(dirname "$0")" && pwd)
repo_root=$(git -C "$script_dir" rev-parse --show-toplevel)
simulator=${ESP32SIM:-"$repo_root/target/release/esp32sim"}
if [ ! -x "$simulator" ]; then
  common_dir=$(git -C "$script_dir" rev-parse --path-format=absolute --git-common-dir)
  canonical_root=${common_dir%/.git}
  simulator="$canonical_root/target/release/esp32sim"
fi
if [ ! -x "$simulator" ]; then
  echo "missing esp32sim release binary: $simulator" >&2
  exit 2
fi

set -- "$build_dir"/*.elf
if [ "$#" -ne 1 ] || [ ! -f "$1" ]; then
  echo "build directory must contain exactly one top-level ELF" >&2
  exit 2
fi
app_elf=$1
app_bin=${app_elf%.elf}.bin
bootloader="$build_dir/bootloader/bootloader.bin"
partition_table="$build_dir/partition_table/partition-table.bin"
manifest="$image_dir/probe-cells.json"
for path in "$app_bin" "$bootloader" "$partition_table" "$manifest"; do
  if [ ! -f "$path" ]; then
    echo "missing dry-run input: $path" >&2
    exit 2
  fi
done

"$simulator" \
  --boot rom \
  --bootloader "$bootloader" \
  --ptable "$partition_table" \
  --app "$app_bin" \
  --elf "$app_elf" \
  --board waveshare-amoled18-v2 \
  --flash-mb 16 \
  --psram-mb 8 \
  --console usb \
  --no-dump \
  --max-insns 400000000 \
  2>&1 | python3 "$script_dir/ndjson.py" \
    --dry-run \
    --manifest "$manifest" \
    --variant normal \
    --cells all
