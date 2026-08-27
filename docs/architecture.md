# Architecture

esp32sim is an instruction-level emulator of the ESP32-S3: it executes the real mask ROM, the
real second-stage bootloader and an unmodified application image on two emulated Xtensa LX7
cores, with the SoC peripherals modelled at the register level and the board (what hangs off
the pins) modelled as devices that interpret pin-level events. It is written in Rust, MIT
licensed, and contains no third-party emulator code (QEMU was consulted for instruction
*semantics* only).

```
cli/          esp32sim binary: argument parsing, image loading, run loop, reports
esp32s3/      the SoC and boards
  machine.rs  Machine: two Cpus + SocBus, scheduler, scripts, web push/poll, reboot, WAV/PNG
  bus.rs      SocBus: memory map, cache MMU, peripheral dispatch, DMA pumps (I2S, camera)
  periph.rs   register models (UART, USB-Serial/JTAG, systimer, TIMG, interrupt matrix, GPIO,
              RTC_CNTL + WDT, efuse, SPI0/1 flash + PSRAM, SHA, AES, RSA/MPI, RNG, regi2c,
              GDMA, I2S, RMT, LCD_CAM, WiFi MAC)
  i2c.rs      I2C master controller + bus devices (CH32V003, OV5640, ES8311/ES7210)
  wifi.rs     virtual 802.11 access point: beacons, probe/auth/assoc, WPA2 four-way handshake
  net.rs      the emulated subnet 10.0.2.0/24: ARP, DHCP, ICMP, DNS, SNTP
  nat.rs      user-mode NAT: guest TCP/UDP relayed over ordinary host sockets
  crypto.rs   SHA-1/2, HMAC, PBKDF2, the 802.11 PRF, AES, AES key wrap, bignum arithmetic
  board.rs    BoardModel trait; Atech14, WaveshareCam, NoBoard; ST7735 and WS2812 decoders
  web.rs      dependency-free HTTP + WebSocket server
  elf.rs / image.rs / picture.rs   loaders (ELF symbols/segments, ESP app images, BMP/PPM)
xtensa-lx7/   the core
  decode.rs   instruction decoder (24/16-bit base ISA, FPU, MAC16, booleans) -> Insn
  pie.rs      PIE SIMD (ee.*) decode/format/execute; pie_table.rs is generated from the TRM
  exec.rs     interpreter: windowed registers, loops, XEA2 exceptions/interrupts, CP enable
  state.rs    Cpu: registers, special registers, user registers (ACCX/QACC/…), interrupt levels
  disasm.rs   objdump-compatible formatter (used by the differential decoder test)
web/          index.html: board drawing, console, WebAudio, camera panel (no build step)
```

## CPU core (`xtensa-lx7`)

- **Decoder**: `decode(pc, bytes) -> Insn` with fields `op, r, s, t, imm, imm2, len, raw`.
  Verified against `xtensa-esp32s3-elf-objdump` over the Pocket Synth app, the mask ROM, the
  IDF 5.5 bootloader, `hello_world` and the autopling image (977 544 instructions, 0
  mismatches, `xtensa-lx7/tests/objdump_diff.rs`).
- **PIE**: all 217 `ee.*` encodings come from the TRM chapter-1 "Instruction Word" layouts
  (`tools/gen_pie_table.py` + `tools/pie_trm.json` → `pie_table.rs`), cross-checked against
  the ESP-IDF 5.5 assembler. Execution follows the TRM "Operation" pseudo-code; PIE is
  coprocessor 3, so `CPENABLE[3]` gates it and FreeRTOS's lazy save/restore works unchanged.
- **Interpreter**: `exec_insn` executes one decoded instruction. Register windows are modelled
  with the 64-entry physical file and WindowBase/WindowStart, including overflow/underflow
  exceptions raised at the *instruction that would touch* the missing window (see
  decisions.md). Timing is 1 instruction = 1 cycle.
- **Basic-block interpreter** (`block.rs`, the normal path): a block is a straight-line run of
  up to 32 pre-decoded instructions ending at a control transfer or at anything that changes
  interrupt, timer or window state. The interrupt check, cache validation and CCOUNT/insn
  accounting happen once per block; window-overflow checks stay per instruction. Exactness is
  kept by bounding a block at the next CCOMPARE match, forcing CCOUNT/CCOMPARE/PS/INTENABLE
  accesses to start a block, ending a block when the bus reports an interrupt-line change, and
  comparing the actual `pc` with the fall-through address after every instruction. Blocks are
  validated by the write-versions of the pages they were decoded from (256-byte pages, see
  decisions.md). `step()` — one instruction per call, with a 16K-entry decode cache — remains
  for tracing, profiling, breakpoints and watchpoints.
- **Traps**: `Exception(cause)`, `Interrupt(n)`, `Unimplemented(pc, raw)`, `Simcall`. The
  machine counts them; `--stop-after-exceptions` and unimplemented instructions stop the run.

## SoC (`esp32s3`)

- **Memory map** (`bus.rs`): SRAM (IRAM `0x4037_0000`, DRAM `0x3FC8_8000` aliases of one
  buffer), mask ROM (`0x4000_0000` I / `0x3FF0_0000` D), RTC fast/slow RAM, the flash/PSRAM
  cache windows (`0x3C00_0000` D-bus, `0x4200_0000` I-bus) translated by the 512-entry MMU
  table at `0x600C_5000` (flash pages or PSRAM pages), peripherals `0x6000_0000–0x600D_0000`.
  Cache timing is not modelled; XIP from flash or PSRAM is a table lookup. A software TLB
  (512 entries of 64 KiB) caches resolved mappings for loads, stores and fetches, and a
  per-256-byte-page write version lets the CPU's block and decode caches skip re-fetching
  instruction bytes; MMU remaps bump the flash/PSRAM versions so decodes built through the old
  mapping cannot run.
- **Peripheral dispatch**: address bits 12–19 select a block; unknown registers land in a
  generic register RAM and are logged on first touch with `--log-periph`.
- **Interrupts**: every source has a level computed by its model (`Peripherals::source_status`);
  the per-core interrupt matrix maps sources to the 32 Xtensa interrupt lines. Lines are
  recomputed when a register write flags `irq_dirty` or every 32 cycles, then written into
  `cpu.interrupt` so the next `step()` sees them.
- **DMA**: GDMA out-channels feed I2S0/I2S1 (audio → `pcm` samples at the configured rate),
  in-channels are fed by the LCD_CAM camera engine (one frame per sensor period). Descriptor
  chains are walked in guest memory exactly as the driver builds them.
- **Reset**: `Machine::reboot()` re-creates the digital peripherals, keeps SRAM, RTC memories,
  efuses and the captured audio, sets `RESET_CAUSE`, and restarts both cores at the ROM reset
  vector — the path used by `esp_restart()` (RTC watchdog) and `SW_PROCPU_RST`.

## Scheduling and time

`Machine::run` interleaves the cores in quanta of 64 instructions. A core sitting in `waiti`
with nothing pending costs nothing; when both cores are idle time advances in 512-cycle
chunks. Device models see time lazily: cycles accumulate in the bus and are delivered in one
batch when a timer alarm is due, when a peripheral register is accessed (so registers always
read exact time), or after 256 cycles at most. Peripheral clocks (APB 80 MHz, systimer 16 MHz,
RTC slow 150 kHz) are derived from the 240 MHz cycle counter with delivered-tick accounting.
With `--web` the machine is paced to wall time (sleeping when ahead, resynchronising rather
than bursting if it falls > 0.5 s behind). Work that costs host syscalls — reading the NAT's
sockets — runs on its own emulated-time cadence rather than every round, because at 240 MHz a
per-round syscall costs more than the instructions it interleaves with.

## Networking

Nothing about the network is faked at the API level: the firmware runs Espressif's own closed
`libpp`/`libnet80211` against a modelled MAC, and what comes out the other end is 802.11 frames.
Five layers turn those into packets on the host's network:

```
esp_wifi + libpp/libnet80211        unmodified blob, drives the MAC registers
  WifiMac (periph.rs)               TX queues, RX descriptor ring, interrupt events, TSF
  VirtualAp (wifi.rs)               beacons, probe/auth/assoc responses, WPA2 four-way handshake,
                                    802.11 <-> Ethernet conversion, CCMP framing
  VirtualNet (net.rs)               10.0.2.0/24: ARP, DHCP, ICMP echo, DNS, SNTP from the host clock
  Nat (nat.rs)                      everything past the gateway -> host sockets
```

- **The air**: `wifi_air_step()` in `bus.rs` delivers one frame at a time into the RX ring —
  spaced ~400 µs apart, and never before the driver has recycled the previous descriptor —
  then raises the MAC's RX interrupt. Management frames are delivered ahead of beacons so a
  response never waits behind a beacon the ring may drop.
- **Encryption**: the four-way handshake is real (PMK, PTK, MIC, AES-key-wrapped GTK), but data
  frames carry plaintext *framed* as CCMP — protected bit, 8-byte CCMP header with the right key
  id, 8 bytes of MIC space — which is exactly what firmware sees when silicon encrypts in place.
- **Off the subnet**: `Nat` terminates each guest flow. A SYN starts `TcpStream::connect` on a
  worker thread and the SYN/ACK waits for it; guest payload is written to the socket; socket
  reads come back as segments the emulator sequences, acknowledges, retransmits and closes.
  UDP is a bound socket per flow with an idle reaper. Name lookups go to the host's own resolver.
- **Crypto accelerators**: mbedTLS drives AES (block + GDMA, ECB/CBC/CTR/OFB), SHA (block + GDMA,
  SHA-1 through SHA-512) and the RSA/MPI unit; the WPA supplicant unwraps the group key on AES.
  All three are modelled at the register level, so a TLS session exercises the same peripherals
  it would on silicon — see peripherals.md and networking-plan.md.

## Boards

`BoardModel` (board.rs) receives GPIO output edges, decoded RMT frames, owns the I2C devices
and the camera source, and exposes optional display/ring/camera state for the UI. The SoC
never knows what board it is on; `--board` selects the implementation. See boards.md.

## Web UI

`web.rs` serves `web/index.html` and one WebSocket per tab. The machine pushes state 50 times
per emulated second (console text, display frames, audio, ring colours, statistics) and polls
inputs (buttons, encoder, serial lines, camera pictures). Protocol in web-ui.md.

## Provenance and verification

- Instruction semantics: Xtensa ISA reference + ESP32-S3 TRM; PIE from the TRM only.
- Peripherals: ESP32-S3 TRM register maps and the ESP-IDF `hal/*_ll.h` drivers as the
  "what does software expect" reference.
- Ground truth: `hw/difftest*.sh` single-step a real ESP32-S3 over USB-JTAG (openocd + gdb)
  and compare PC/registers with the emulator running the same flash image — zero divergence
  over the ROM reset path and the bootloader.
