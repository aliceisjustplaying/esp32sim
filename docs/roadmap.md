# Roadmap

Ordered by value; each item links to its plan where one exists.

1. **WiFi (full, unmodified)** — open networks work end to end: scan, auth, association, DHCP, IP
   ([wifi-plan.md](wifi-plan.md)). Next: WPA2 4-way handshake (real firmware uses a password), then
   a NAT backend (libslirp) for traffic beyond the emulated subnet.
   The `esp_wifi` shim ([networking-plan.md](networking-plan.md)) remains the fallback if the
   blob route stalls; either unblocks the panel/autopling network features.
2. **Testing** — hermetic CPU/SoC/board suites, conformance firmware, CI tiers
   ([testing-plan.md](testing-plan.md)). Milestone 1 (no silent skips, shared CPU harness,
   parser robustness) first.
3. **Firmware upload from the browser** — drop a `firmware.bin` on the page → written to
   flash at 0x10000 → `Machine::reboot()`. Pieces exist; ~an hour.
4. **Interpreter speed** — a basic-block / threaded interpreter for the ~3.5× needed to run
   bit-banged display redraws at real time. Executor is now the dominant cost (≈50 %).
5. **More boards** — Touch-LCD-4B done (`waveshare-lcd4b`: LVGL panel, touch/swipe, SID player audio).
   Next candidates as firmware needs them; `--board waveshare-*` variants share the codec/PSRAM/I2C work.
6. **Peripherals on demand** — LEDC, PCNT, ADC, SPI2/3 masters, RX sides of I2S/RMT/UART DMA,
   LCD side of LCD_CAM. Each appears as "unknown register" in the log when a firmware needs it.
7. **PIE completeness** — FFT, GPIO and s32 instruction groups (decoded, not executed).
8. **Packaging** — `cargo install esp32sim`, a `--net`/`--board` aware `examples/` runner,
   release binaries for macOS/Linux.

Not planned: blob-level WiFi/BLE emulation, cache-timing accuracy, Wokwi/cloud integration.
