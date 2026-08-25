# Roadmap

Ordered by value; each item links to its plan where one exists.

1. **WiFi (full, unmodified)** — done: open and WPA2-PSK both associate through the closed blob,
   and outbound TCP/UDP reaches the real network through a user-mode NAT, TLS included
   ([wifi-plan.md](wifi-plan.md), [networking-plan.md](networking-plan.md)). Remaining: inbound
   port forwarding (autopling's web UI from the Mac), multicast/mDNS, and a `--net tap` backend
   for real LAN presence.
2. **Testing** — hermetic CPU/SoC/board suites, conformance firmware, CI tiers
   ([testing-plan.md](testing-plan.md)). Milestone 1 (no silent skips, shared CPU harness,
   parser robustness) first.
3. **Firmware upload from the browser** — drop a `firmware.bin` on the page → written to
   flash at 0x10000 → `Machine::reboot()`. Pieces exist; ~an hour.
4. **Interpreter speed** — a basic-block / threaded interpreter. After removing the syscall and
   hashing overheads (see decisions.md), the panel runs ~2× real time and the same workload with
   WiFi ~1.4×, with ~75 % of run time now in the interpreter itself — so this is again the next
   thing worth attacking, and it is what bit-banged display redraws still need.
5. **More boards** — Touch-LCD-4B done (`waveshare-lcd4b`: LVGL panel, touch/swipe, SID player audio).
   Next candidates as firmware needs them; `--board waveshare-*` variants share the codec/PSRAM/I2C work.
6. **Peripherals on demand** — LEDC, PCNT, ADC, SPI2/3 masters, RX sides of I2S/RMT/UART DMA,
   LCD side of LCD_CAM. Each appears as "unknown register" in the log when a firmware needs it.
7. **PIE completeness** — FFT, GPIO and s32 instruction groups (decoded, not executed).
8. **Packaging** — `cargo install esp32sim`, a `--net`/`--board` aware `examples/` runner,
   release binaries for macOS/Linux.

Not planned: blob-level WiFi/BLE emulation, cache-timing accuracy, Wokwi/cloud integration.
