#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
action="${1:-run}"
tinydraw_root="${2:-${TINYDRAW_DIR:-$(cd "$repo_root/.." && pwd)/tinydraw}}"
web_port="${3:-${TINYDRAW_WEB_PORT:-8766}}"
build_root="$tinydraw_root/out/build/esp32-vector-v2"

usage() {
  printf '%s\n' \
    "usage: $0 {build|run|smoke|flash} [TINYDRAW_DIR] [WEB_PORT_OR_SERIAL_PORT]" \
    "" \
    "Keep tinydraw next to esp32sim and run: $0 run" \
    "Or set TINYDRAW_DIR once for a different checkout location."
}

require_tinydraw() {
  if [[ ! -x "$tinydraw_root/scripts/esp32" ]]; then
    echo "TinyDraw checkout not found at $tinydraw_root" >&2
    usage >&2
    exit 2
  fi
}

build_all() {
  require_tinydraw
  "$tinydraw_root/scripts/esp32" build
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
    build_all
    ;;
  run)
    build_all
    if command -v open >/dev/null 2>&1; then
      (sleep 1; open "http://127.0.0.1:$web_port/") >/dev/null 2>&1 &
    fi
    run_emulator --web "$web_port" --web-dir "$repo_root/web"
    ;;
  smoke)
    build_all
    smoke_log="$(mktemp "${TMPDIR:-/tmp}/tinydraw-v2-smoke.XXXXXX")"
    trap 'rm -f "$smoke_log"' EXIT
    run_emulator --script "$repo_root/examples/tinydraw-v2/touch.script" \
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
    require_tinydraw
    serial_port="${3:-}"
    if [[ -z "$serial_port" ]]; then
      ports=(/dev/cu.usbmodem*)
      if [[ ! -e "${ports[0]}" || "${#ports[@]}" -ne 1 ]]; then
        echo "expected exactly one /dev/cu.usbmodem* device; pass the serial port explicitly" >&2
        exit 2
      fi
      serial_port="${ports[0]}"
    fi
    exec "$tinydraw_root/scripts/esp32" vector-v2 "$serial_port"
    ;;
  -h|--help|help)
    usage
    ;;
  *)
    usage >&2
    exit 2
    ;;
esac
