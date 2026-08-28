# esp32sim — an ESP32‑S3 (Xtensa LX7) emulator in Rust

Instruction‑level emulation of the ESP32‑S3 that boots the **real mask ROM**, the real
2nd‑stage bootloader and an unmodified application image, dual‑core, with enough of the
SoC and boards modelled to run real firmware end to end — the Atech "Pocket Synth", stock
ESP-IDF examples, the Waveshare ESP32-S3-CAM autopling detector:
console over USB‑Serial/JTAG, the ST7735 display decoded from bit‑banged SPI, the
WS2812 ring decoded from RMT, audio captured from I2S/GDMA, buttons/encoder/serial
driven from a timed script. No cloud, no accounts.

```
esp32sim/
  xtensa-lx7/     the core: decoder (verified 100% against objdump over app+ROM+IDF),
                  interpreter (windowed regs, loops, XEA2 exceptions/interrupts, FPU,
                  MAC16, booleans, PIE SIMD), objdump-compatible disassembler
  esp32s3/        SoC + boards: memory map, cache MMU, SPI flash/PSRAM, SHA, RNG,
                  systimer, timer groups, interrupt matrix (per core), GPIO, USB-CDC,
                  UARTs, I2C, GDMA + I2S/LCD_CAM, RMT TX, regi2c, RTC WDT, dual core;
                  board.rs: atech14 / waveshare-cam / none; ELF/app-image loaders
  cli/            the `esp32sim` command line
  web/            browser UI (board drawing, console, audio, camera)
  hw/             JTAG differential-test scripts against a real board, wsdrive.py
  examples/       hello_world (IDF), waveshare-cam (autopling run script + test photo)
  boards/atech14/ the Atech Pocket Synth: firmware (PlatformIO), hostsim, Wokwi
                  scenarios, regression.wav, script1.txt
  tools/          PIE table generator (TRM-derived); bench.py: interleaved A/B benchmark of builds;
                  wasm-build.sh: the WebAssembly module
  wasm/           C-ABI crate wrapping Machine for the browser (web/emu.js + web/wasm/worker.js drive it)
```

## Boards

The SoC model emits pin-level events (GPIO edges, RMT symbol streams, I2S samples); a
`BoardModel` (`esp32s3/src/board.rs`) interprets them. `--board atech14` (default) is the Atech
14‑port board with its ST7735, WS2812 ring, encoder and buttons; `--board none` is a bare
module — any ESP32‑S3 firmware, console only; `--board waveshare-lcd4b` is the Waveshare
ESP32‑S3‑Touch‑LCD‑4B (ST7701S 480×480 over the LCD_CAM RGB bus, GT911 touch, TCA9554, codecs)
running the esp32-screen LVGL panel with touch; `--board waveshare-cam` is the Waveshare
ESP32‑S3‑CAM‑OV5640 (CH32V003 IO expander, OV5640 over SCCB, ES8311/ES7210 codecs on I2C0,
speaker on I2S1, OV5640 on the LCD_CAM DVP port) — runs the `waveshare-autopling` firmware
(IDF 5.5, 16 MB flash, 8 MB octal PSRAM: `--flash-mb 16 --psram-mb 8`) end to end: camera frames
from `--cam-image` or the browser (picture upload / webcam) → esp‑dl pedestrian detector on the
emulated PIE SIMD unit → pling on the ES8311. See `examples/waveshare-cam/`. Adding a board =
implementing the trait (`gpio_changes`, `rmt_frame`, `i2c_devices`, `camera_frame`, …).

## Run

```sh
cargo build --release
B=boards/atech14/firmware/.pio/build/hw
BL=~/.platformio/packages/framework-arduinoespressif32/tools/sdk/esp32s3/bin/bootloader_dio_80m.elf
./target/release/esp32sim --boot rom \
    --bootloader $B/bootloader.bin --ptable $B/partitions.bin --app $B/firmware.bin \
    --elf $B/firmware.elf --elf $BL \
    --script boards/atech14/script1.txt --wav out.wav --tft-png tft.png --max-seconds 5
```

The mask ROM ELF is picked up from `~/.espressif/tools/esp-rom-elfs/*/esp32s3_rev0_rom.elf`
(shipped with ESP‑IDF). `--boot app` skips ROM+bootloader and loads the app image directly.

A plain ESP‑IDF project, e.g. `examples/hello_world` (the IDF 5.5 get-started example built with
`idf.py set-target esp32s3 && idf.py build`):

```sh
H=examples/hello_world/build
./target/release/esp32sim --board none --boot rom --console uart0 \
    --bootloader $H/bootloader/bootloader.bin --ptable $H/partition_table/partition-table.bin \
    --app $H/hello_world.bin --elf $H/hello_world.elf --elf $H/bootloader/bootloader.elf --max-seconds 26
```

prints the ROM banner, the bootloader and app logs on UART0, `Hello world!`, the countdown, and
then reboots through the RTC watchdog exactly as silicon does (`rst:0xc (RTC_SW_CPU_RST)`),
~30× faster than real time. Chip resets (software, RTC watchdog) restart the machine from the
ROM with the right reset cause; `--no-reboot` stops at the first reset instead.

## Scripts (host actions at emulated time)

```
1.5  press btn1 150                       # GPIO17 low for 150 ms
2.2  knob cw 2                            # two quadrature detents on CLK5/DT4
3.4  press btn2 120
4.0  serial {"action":"set_note","value":"9"}   # into USB-CDC RX
5.0  stop
```

## WiFi

`--wifi ssid=NAME[,psk=PASS,chan=N,bssid=..]` attaches a virtual access point that the **unmodified**
Espressif WiFi blob associates with — scan, authentication, association and, with a passphrase, the
WPA2-PSK four-way handshake — and a virtual network behind it (DHCP, ARP, ICMP, DNS, SNTP off the
host clock). `--net nat` (the default) relays the guest's TCP and UDP over ordinary host sockets, so
firmware reaches the real network; `--net none` refuses it. No firmware changes, no root, no tun
device.

```sh
./target/release/esp32sim --board waveshare-lcd4b --boot rom --flash-mb 16 --psram-mb 8 \
    --bootloader $P/bootloader/bootloader.bin --ptable $P/partition_table/partition-table.bin \
    --app $P/energy_panel.bin --console usb --wifi "ssid=home,psk=secret" --max-seconds 45
```

runs the esp32-screen energy panel: it joins, takes a lease, syncs its clock, fetches two days of
electricity prices over **HTTPS** and polls a real Home Assistant on the LAN.
[docs/networking-howto.md](docs/networking-howto.md) is the how-to (flags, debugging, limits);
[docs/wifi-plan.md](docs/wifi-plan.md) and [docs/networking-plan.md](docs/networking-plan.md)
describe how the MAC model and the packet path work.

## In the browser (WebAssembly)

```sh
tools/wasm-build.sh && python3 -m http.server -d web 8790     # then open http://127.0.0.1:8790/?wasm
```

The same emulator compiled to WebAssembly, running inside the page in a Web Worker: pick a board,
load the ROM ELF and firmware from disk (or `?wasm&fw=<name>` for a hosted manifest), press Boot.
hello_world, the Touch-LCD-4B panel with its SID player, and the Atech board run at real time in
Chrome; there is no NAT (the browser has no sockets) and no JIT. See [docs/wasm.md](docs/wasm.md).
**Live: https://joakimeriksson.github.io/esp32sim/** — hello_world, or your own firmware from disk.

## Debugging

`--trace [--trace-from N]`, `--break ADDR`, `--watch ADDR` (stop when a word changes),
`--peek ADDR[,N]`, `--profile` (pc histogram), `--log-periph` (first touch of every
unknown register), `--stop-after-exceptions N`, `--gram-png` (raw ST7735 GRAM), `--no-jit`
(interpret instead of running native code — must give identical results).
Env: `ESP_EMU_DEBUG`, `ESP_EMU_DEBUG_SPI`, `ESP_EMU_DEBUG_USB`, `ESP_EMU_DEBUG_WIFI[_FRAMES]`,
`ESP_EMU_DEBUG_NET`, `ESP_EMU_DEBUG_AES`, `ESP_EMU_DEBUG_SHA`, `ESP_EMU_DEBUG_RSA`.

## Documentation

`docs/` — [architecture](docs/architecture.md), [peripheral coverage](docs/peripherals.md),
[boards](docs/boards.md), [CLI reference](docs/cli.md), [web UI protocol](docs/web-ui.md),
[design decisions & gotchas](docs/decisions.md), [roadmap](docs/roadmap.md),
[networking how-to](docs/networking-howto.md), the [WiFi](docs/wifi-plan.md) and
[networking](docs/networking-plan.md) design notes, and the [testing](docs/testing-plan.md) plan.

## Provenance

Written from the ESP‑IDF register headers, the Xtensa core config shipped with ESP‑IDF and
observed firmware behaviour. QEMU was consulted only to confirm instruction semantics
(no code copied). MIT.

## Differential testing against real silicon (`hw/`)

`DIFF_DIR=hw/<board> FLASH_MB=8 hw/difftest.sh 3000` — the scripts read efuses/strap from the
attached chip over JTAG, then step it and the emulator in lock-step on the same flash dump
(`flash-8M.bin` if present). Atech board, 2026-08-25: 3000 steps from reset, 0 divergences.

Any ESP32‑S3 board on USB works (its built‑in USB‑Serial/JTAG carries both the console
and JTAG). The flow, all scripted:

```sh
# one-time: dump the board's bootloader/partition table/app start (esptool) into hw/flash-0-1M.bin
hw/difftest.sh 8000                 # reset → single-step 8000 instructions on the chip and in the emulator, diff
hw/difftest-at.sh 403c8948 6000     # run both to a PC (here the 2nd-stage bootloader entry), then step 6000 and diff
```

`difftest*.sh` read the chip's efuses (`hw/efuse.txt`), strapping pins and the peripheral
reset state (`hw/reset-regs.txt`, dumped over JTAG at `reset halt`) and start the emulator
from the same image and state. `hw/compare.py` diffs `pc a0..a15 ps windowbase` per
instruction, masks `PS.INTLEVEL` (forced during single‑step), hides window‑exception
handlers (the debugger steps over them atomically) and resynchronises across CCOUNT‑timed
delay loops, which iterate a different number of times when each step takes milliseconds.

Result so far: the ROM reset path (8000 steps) and 3000+ steps of the IDF 5.5 bootloader
run with zero PC divergence; remaining register differences are RTC‑domain values that
depend on the previous boot.

## Live board UI

```sh
./target/release/esp32sim --boot rom --bootloader … --ptable … --app … --web 8766
# open http://127.0.0.1:8766/
```

`--web PORT` runs the emulator in real time and serves `web/index.html`: the 14‑port board
(knob + LED ring, buttons, speaker VU, the ST7735 in its physical orientation plus a readable
copy), USB‑CDC and UART0 consoles, an action box for the SDK JSON protocol, and audio through
WebAudio. Inputs: click the buttons, wheel/drag/←→ on the knob, click the cap to push it.

## Performance / real time

Idle cores (`waiti`) are skipped, time advances in 256‑cycle chunks while both cores sleep,
decoded instructions are cached, and interrupt lines are recomputed only when a source changes.
The Pocket Synth firmware runs at real time with margin while idle or playing notes; the one
place it cannot keep up is a full display redraw, where core 1 runs 100 % busy bit‑banging SPI
(4.8 M instructions per 20 ms — 240 Minsn/s needed, ~70 Minsn/s achieved on an M‑series
MacBook). Each redraw therefore costs ~0.3 s of lag, which the UI's adaptive audio buffer
absorbs (it grows on underrun, up to 400 ms) and the pacer recovers afterwards; if the
emulator ever falls more than 0.5 s behind it resynchronises instead of bursting. The header
shows `real time`, `⚠ N s behind` and the resync count.

`ESP_EMU_RT_LOG=1` prints every 20 ms window that took > 40 ms wall with both cores'
instruction counts and PCs. `hw/wsdrive.py [port] [seconds]` drives the UI protocol without a
browser (button presses + knob turns) and reports push gaps, lag and audio delivered — use it
to measure changes to the scheduler.
