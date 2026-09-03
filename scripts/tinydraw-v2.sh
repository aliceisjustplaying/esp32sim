#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
action="${1:-run}"
endpoint="${2:-}"
web_port="${endpoint:-${TINYDRAW_WEB_PORT:-8766}}"
build_root="${TINYDRAW_VECTOR_V2_BUILD:-${HOME}/Archives/esp32s3/pinned-builds/tinydraw-vector-v2-9cb651e0}"

usage() {
  printf '%s\n' \
    "usage: $0 {build|verify|run|smoke} [WEB_PORT]" \
    "       $0 flash [SERIAL_PORT]" \
    "" \
    "TinyDraw artifacts come from the immutable TINYDRAW_VECTOR_V2_BUILD pin." \
    "This script never reads from or builds the TinyDraw checkout."
}

verify_build() {
  local expected relative actual
  while read -r expected relative; do
    if [[ ! -f "$build_root/$relative" ]]; then
      echo "pinned TinyDraw artifact missing: $build_root/$relative" >&2
      exit 2
    fi
    actual="$(shasum -a 256 "$build_root/$relative")"
    actual="${actual%% *}"
    if [[ "$actual" != "$expected" ]]; then
      echo "pinned TinyDraw artifact hash mismatch: $relative" >&2
      echo "expected $expected, found $actual" >&2
      exit 2
    fi
  done <<'HASHES'
634e8dfab00aaa24c8b4514aecd77d842d5a49438baca87abf5f3a35e474b5ab bootloader/bootloader.bin
f53268312c8caffe6c7f4e6c66d4092aeca3435c142db3116466f84a6a608d2d partition_table/partition-table.bin
1352e0c415aac2050b8159a7d7deae82f74f5f4202b9bbf000fefd0bc3573936 tinydraw_esp32.bin
9cb651e09a5405bc68fa5aa4656a22977e1c54f3198cb86bd5bc9753ba1d251b tinydraw_esp32.elf
HASHES
}

build_emulator() {
  cargo build --release --manifest-path "$repo_root/Cargo.toml" -p esp32sim
}

run_emulator() {
  "$repo_root/target/release/esp32sim" \
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
    "$@"
}

case "$action" in
  build)
    build_emulator
    ;;
  verify)
    verify_build
    ;;
  run)
    verify_build
    build_emulator
    if command -v open >/dev/null 2>&1; then
      (sleep 1; open "http://127.0.0.1:$web_port/") >/dev/null 2>&1 &
    fi
    run_emulator --web "$web_port" --web-dir "$repo_root/web"
    ;;
  smoke)
    verify_build
    build_emulator
    smoke_log="$(mktemp "${TMPDIR:-/tmp}/tinydraw-v2-smoke.XXXXXX")"
    smoke_script="$(mktemp "${TMPDIR:-/tmp}/tinydraw-v2-touch.XXXXXX")"
    trap 'rm -f "$smoke_log" "$smoke_script"' EXIT
    printf '%s\n' \
      '1.2 touch 80 140 1' \
      '1.3 touch 110 170 1' \
      '1.4 touch 140 200 1' \
      '1.5 touch 170 230 1' \
      '1.6 touch 200 260 1' \
      '1.7 touch 230 290 1' \
      '1.8 touch 260 320 1' \
      '1.9 touch 260 320 0' >"$smoke_script"
    run_emulator --script "$smoke_script" \
      --max-seconds 120 2>&1 | tee "$smoke_log"
    grep -q 'TINYDRAW_VECTOR_V2_READY' "$smoke_log"
    grep -qE 'TINYDRAW_LIVE_STROKE .*samples=([2-9]|[1-9][0-9]+)' "$smoke_log"
    grep -q 'TINYDRAW_LIVE_STROKE_DONE committed=1 refresh=1 commit_failed=0' "$smoke_log"
    if grep -qE 'Guru Meditation|task_wdt|TG1WDT_SYS_RST|stack overflow|TINYDRAW_LIVE_FAIL' "$smoke_log"; then
      echo "TinyDraw V2 smoke test reported a crash or product failure" >&2
      exit 1
    fi
    echo "TinyDraw V2 smoke test passed"
    ;;
  flash)
    verify_build
    serial_port="$endpoint"
    if [[ -z "$serial_port" ]]; then
      ports=(/dev/cu.usbmodem*)
      if [[ ! -e "${ports[0]}" || "${#ports[@]}" -ne 1 ]]; then
        echo "expected exactly one /dev/cu.usbmodem* device; pass the serial port explicitly" >&2
        exit 2
      fi
      serial_port="${ports[0]}"
    fi
    exec python -m esptool --chip esp32s3 --port "$serial_port" --baud 921600 \
      --before default_reset --after hard_reset write_flash \
      --flash-mode dio --flash-size 16MB --flash-freq 80m \
      0x0 "$build_root/bootloader/bootloader.bin" \
      0x8000 "$build_root/partition_table/partition-table.bin" \
      0x10000 "$build_root/tinydraw_esp32.bin"
    ;;
  -h|--help|help)
    usage
    ;;
  *)
    usage >&2
    exit 2
    ;;
esac
