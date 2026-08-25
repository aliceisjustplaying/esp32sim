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
4. **Interpreter speed** — a basic-block / threaded interpreter. With the syscall, hashing and
   fetch overheads gone (see decisions.md) the SID player runs at real time and the panel with WiFi
   at ~2×, and ~36 % of run time is now `exec_insn` itself with another ~28 % in the step around it.
   That dispatch is what is left to attack, and it is what bit-banged display redraws still need.
5. **More boards** — Touch-LCD-4B done (`waveshare-lcd4b`: LVGL panel, touch/swipe, SID player audio).
   Next candidates as firmware needs them; `--board waveshare-*` variants share the codec/PSRAM/I2C work.
6. **Peripherals on demand** — LEDC, PCNT, ADC, SPI2/3 masters, RX sides of I2S/RMT/UART DMA,
   LCD side of LCD_CAM. Each appears as "unknown register" in the log when a firmware needs it.
7. **PIE completeness** — FFT, GPIO and s32 instruction groups (decoded, not executed).
8. **Browser build (WebAssembly)** — the emulator itself in the page rather than served by it
   ([wasm-plan.md](wasm-plan.md)). The CPU crate is already host-API-free and a spike measured wasm
   at ~47 % of native speed, so the UI and WiFi demos would run but the SID player needs item 4
   first.
9. **Packaging** — `cargo install esp32sim`, a `--net`/`--board` aware `examples/` runner,
   release binaries for macOS/Linux.

Not planned: blob-level WiFi/BLE emulation, cache-timing accuracy, Wokwi/cloud integration.
