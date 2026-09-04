#!/bin/bash
set -euo pipefail

script_dir=$(cd "$(dirname "$0")" && pwd)
repo_root=$(git -C "$script_dir" rev-parse --show-toplevel)
session="$script_dir/session.json"
simulator=${ESP32SIM:-/Users/sarah/src/a/esp32sim/target/release/esp32sim}
frame_tool=${FRAME_CORRELATION_TOOL:-"$repo_root/tools/frame_correlation.py"}
expected_sim_sha=7011f188df79bcff294a8544026f73156342058f15203d1a6e63684f7b7475f9

python3 "$script_dir/verify_session.py" "$session"
if [ ! -x "$simulator" ] || [ ! -f "$frame_tool" ]; then
  echo "missing pinned simulator or frame-correlation tool" >&2
  exit 2
fi
actual_sim_sha=$(shasum -a 256 "$simulator" | cut -d ' ' -f 1)
if [ "$actual_sim_sha" != "$expected_sim_sha" ]; then
  echo "esp32sim executable SHA-256 mismatch" >&2
  exit 2
fi

h1=$(jq -r '.captureOrder[0].bundlePath' "$session")
fast=$(jq -r '.captureOrder[1].bundlePath' "$session")
slow=$(jq -r '.captureOrder[2].bundlePath' "$session")
output=$(mktemp -d "$repo_root/out/hardware-batch-rehearsal.XXXXXX")
h1_stage="$output/h1"
mkdir -p "$h1_stage/bootloader" "$h1_stage/partition_table"
ln -s "$h1/esp32s3_exception_ladders_calibration.elf" "$h1_stage/esp32s3_exception_ladders_calibration.elf"
ln -s "$h1/esp32s3_exception_ladders_calibration.bin" "$h1_stage/esp32s3_exception_ladders_calibration.bin"
ln -s "$h1/bootloader/bootloader.bin" "$h1_stage/bootloader/bootloader.bin"
ln -s "$h1/partition_table/partition-table.bin" "$h1_stage/partition_table/partition-table.bin"

ESP32SIM="$simulator" "$repo_root/calibration/tools/dry-run.sh" \
  "$repo_root/calibration/esp32s3-exception-ladders" "$h1_stage"

run_frame() {
  local bundle="$1"
  local label="$2"
  "$simulator" \
    --rom "$bundle/rom/esp32s3_rev0_rom.elf" \
    --boot rom \
    --bootloader "$bundle/bootloader/bootloader.bin" \
    --ptable "$bundle/partition_table/partition-table.bin" \
    --app "$bundle/tinydraw_esp32.bin" \
    --elf "$bundle/tinydraw_esp32.elf" \
    --board waveshare-amoled18-v2 \
    --flash-mb 16 \
    --psram-mb 8 \
    --console usb \
    --no-dump \
    --max-seconds 2 \
    >"$output/raw-$label.log" 2>&1
  python3 "$frame_tool" normalize "$bundle/MANIFEST.json" \
    "$output/raw-$label.log" --source emulator >"$output/normalized-$label.ndjson"
}

run_frame "$fast" 80mhz
run_frame "$slow" 40mhz
python3 "$frame_tool" psram-candidate \
  "$output/normalized-40mhz.ndjson" "$output/normalized-80mhz.ndjson" \
  >"$output/candidate.json"
jq . "$output/candidate.json"
echo "rehearsal outputs: $output"
