# ESP32-C6 (RISC-V) — draft

The third chip in the workspace, and the second RISC-V one: a single RV32IMAC core at 160 MHz
with 512 KB of unified HP SRAM, on the board it was brought up against — the Waveshare
ESP32-C6-LCD-1.47 (ESP32-C6FH4, 4 MB embedded flash, ST7789 172×320 LCD, WS2812, TF slot).
Unmodified ESP-IDF firmware boots from the real mask ROM, through the real 2nd-stage bootloader,
into FreeRTOS; with `--board waveshare-c6-lcd147` the board's display and LED are there too, and
an IEEE 802.15.4 energy scan gets answers from the emulated MAC.

```sh
cargo build --release
H=examples/hello_world-c6/build
./target/release/esp32sim-c6 --boot rom --flash-mb 4 \
    --bootloader $H/bootloader/bootloader.bin --ptable $H/partition_table/partition-table.bin \
    --app $H/hello_world.bin --elf $H/hello_world.elf --max-seconds 27
```

prints the ROM banner, the bootloader's partition table, the IDF startup log, `Hello world!`, the
countdown, and reboots through the ROM on `esp_restart()` — three complete boots in 27 s:

```
ESP-ROM:esp32c6-20220919
Build:Sep 19 2022
rst:0x1 (POWERON),boot:0x6e (SPI_FAST_FLASH_BOOT)
I (5) boot: ESP-IDF v5.5.4 2nd stage bootloader
I (5) boot: chip revision: v0.1
I (5) boot: efuse block revision: v0.3
I (33) main_task: Calling app_main()
Hello world!
This is esp32c6 chip with 1 CPU core(s), WiFi/BLE, 802.15.4 (Zigbee/Thread), silicon revision v0.1, 2MB external flash
Minimum free heap size: 473128 bytes
```

The mask ROM ELF is picked up from `~/.espressif/tools/esp-rom-elfs/*/esp32c6_rev0_rom.elf`
(shipped with ESP-IDF); override with `--rom`. Flags mirror the C3 binary — see `--help`;
`esp32sim --chip c6` is the same thing.

## Verified against real silicon

The Waveshare board (chip rev v0.1, efuse block v0.3, MAC `dc:1e:d5:6e:8c:dc`; its efuse summary
is in `hw/c6-efuse.txt`) was flashed with the same three binaries the emulator runs, and 27 s of
its USB-Serial/JTAG console captured (`hw/c6-hello-world-real.txt`). Comparing that with the
emulator, log timestamps normalised:

| | |
| --- | --- |
| **203 of 204 lines identical** over three complete boot cycles | |
| the only difference | the ROM's `Saved PC:` line on the **first** boot |

To reproduce, tell the emulator the board's identity and boot conditions:

```sh
./target/release/esp32sim-c6 --boot rom --flash-mb 4 \
    --mac dc:1e:d5:6e:8c:dc --reset-cause 0x15 --strap 0x6e \
    --bootloader $H/bootloader/bootloader.bin --ptable $H/partition_table/partition-table.bin \
    --app $H/hello_world.bin --elf $H/hello_world.elf --max-seconds 27
```

`Saved PC` is the PC the ASSIST_DEBUG block recorded when the reset hit. After `esp_restart()`
the emulator prints the same value as silicon (`0x4001975a`, the instruction after the ROM's
`software_reset_cpu` store), because a reset now takes effect at the instruction that requests
it. The first boot's value is where the chip was when esptool reset it out of download mode,
which a cold start cannot know; that line is the one missing.

The bootloader's own timestamps are a few milliseconds earlier than the board's (`I (5)` vs
`I (23)`): the ROM's flash probing takes real time on silicon that the model does not spend.

### What the bring-up found

Four of these were invisible without the board's console to compare against; the fifth came
from the trace.

- **The cycle counter restarts at a chip reset.** `mcycle`/`mpccr` fed the emulator's own
  monotonic instruction count, so after `esp_restart()` the bootloader's log timestamps read
  `I (44117)` instead of `I (23)` — on the **C3 as well**, hidden in its golden by timestamp
  normalisation. The guest now sees cycles since reset; the emulator's count stays monotonic.
  (A guest write to the counter also used to move the emulator's instruction count: the C3
  golden's count was inflated by a billion phantom instructions over two resets.)
- **A reset takes effect at the instruction that requests it.** The scheduler used to notice the
  reset at the end of the 64-instruction quantum, so the core ran on through the ROM's `ret`
  and back into the app. Harmless for the console, wrong for `Saved PC`.
- **PCR's hardware-fixed clock-tree fields must read their silicon values.** SOC_ROOT→HP_ROOT is
  a fixed ÷3 on the PLL path and PCR_SYSCLK_CONF says so in a read-only field; a register RAM
  read it as 0, the app derived a CPU divider of 2 from it, and `clk_ll_cpu_set_hs_divider`
  asserted. The PLL and XTAL frequencies the query register reports are fixed the same way.
- **The ROM's `SPI_init` waits for SPI0's AXI FIFOs to report empty** (a C6-only status
  register), and the bootloader's clock switch waits for `BBPLL_CAL_DONE` on the analog I2C
  master. Each is a "done" bit that must be set, or the boot stalls silently at 100% CPU.
- **The interrupt threshold is PLIC+0x90, not INTPRI's offset.** With it in the wrong place the
  interrupt handler could not mask its own level before enabling nesting, the systimer line
  re-entered 3 867 times and walked the stack down through all of SRAM into the code. The
  symptom was an illegal instruction in `rtos_int_enter`; the cause was 500 KB away.

### Reading a board yourself

```sh
python -m esptool --port /dev/cu.usbmodem101 chip_id          # identify before touching anything
python -m esptool --port /dev/cu.usbmodem101 --baud 921600 \
    read_flash 0 0x400000 backup.bin                           # back up first (the factory demo)
cd examples/hello_world-c6/build && python -m esptool --port /dev/cu.usbmodem101 \
    --baud 921600 --chip esp32c6 write_flash '@flash_args'
python -m espefuse --port /dev/cu.usbmodem101 summary          # the revision fields above
```

## In the browser

The C6 is in the WebAssembly build too — pick board `esp32c6` on the page, or open it directly:

    https://joakimeriksson.github.io/esp32sim/?fw=c6-hello
    https://joakimeriksson.github.io/esp32sim/?fw=c6-energy-scan

The first is console-only; the second is the energy scanner on the Waveshare board at real time,
with the panel, the WS2812 and a BOOT button on the page (its firmware is published under
`web/wasm/fw/public/`, and the `bb_init` stub is resolved through the manifest's `symbols` map,
so no ELF ships). See [wasm.md](wasm.md).

## What is modelled

| | |
| --- | --- |
| CPU | RV32IMAC, machine mode: the C3 core plus the A extension (`lr.w`/`sc.w`, the nine AMOs), `misa` says IMAC |
| Interrupts | the interrupt matrix (77 sources → 31 lines) with its two front-ends: the PLIC at `0x20001000` that ESP-IDF drives and INTPRI at `0x600C5000` that the ROM uses, one state; the four `FROM_CPU` software interrupts |
| Memory | 512 KB HP SRAM (one address space for code and data), 320 KB mask ROM, 16 KB LP SRAM, a 16 MB flash window through the 256-entry MMU programmed via SPI0's item index/content registers, page size from `MMU_POWER_CTRL` |
| Peripherals | UART0/1, USB-Serial/JTAG, systimer, TIMG0/1, GPIO, efuse, SPI0/1 flash controller, SHA/AES/RSA, L1 cache controller, PCR, the LP blocks (LP_CLKRST reset cause, LP_AON store registers and software reset, LP_TIMER, LP_WDT registers), the analog I2C master (regi2c, with the RF block's status the PHY polls), ASSIST_DEBUG's saved PC, hardware RNG; for the board: RMT (the S3 transmitter on the C6's register map), GDMA (three channels, the S3 model behind the C6's layout), GP-SPI2 with its DMA data phase; the 802.15.4 MAC: energy detect, TX and RX of whole frames with the timing of the air, its two timers (below) |

Peripheral models are **shared with the C3 and S3 through `esp-periph`** where the IP is the same:
the register-offset comparison of the IDF headers shows UART (the registers the model uses; the
C6's `REG_UPDATE` handshake was added), systimer, timer groups, GPIO, USB-Serial/JTAG, the SPI
flash controller, GP-SPI and efuse all match. RMT and GDMA are the same IP behind a different
register map, so `RmtC6` and `GdmaC6` translate offsets onto the shared models. `esp32c6/src/periph.rs`
is the `device_set!` table plus the chip-specific blocks.

The decoder is checked against `riscv32-esp-elf-objdump` the same way: 126,384 instructions from
the C6 mask ROM and hello_world, 0 mismatches (`riscv-rv32/tests/corpus/c6-*.dis` carry a
sample of each). Neither binary contains an atomic instruction — IDF's hello_world does not need
them — so the A extension is pinned by an assembled program in `riscv-rv32/tests/semantics.rs`;
the LVGL firmware for this board has 33 of them.

```sh
OD=~/.espressif/tools/riscv32-esp-elf/*/riscv32-esp-elf/bin/riscv32-esp-elf-objdump
$OD -d ~/.espressif/tools/esp-rom-elfs/*/esp32c6_rev0_rom.elf > /tmp/rom.dis
$OD -d examples/hello_world-c6/build/hello_world.elf > /tmp/app.dis
RISCV_DIS_FILES=/tmp/rom.dis:/tmp/app.dis cargo test -p riscv-rv32 --release -- --ignored external_
```

## The 802.15.4 MAC (`esp32c6/src/radio.rs`)

The `IEEE802154` block at `0x600A3000`, with the register layout of IDF's
`soc/ieee802154_struct.h`, modelled as far as `esp_ieee802154` (IDF 5.5) drives it. What the
driver does — read from `esp_ieee802154_dev.c` and confirmed with a register trace of Contiki-NG's
radio driver on this model:

- **transmit**: `STOP`, `TIMER0_STOP`, `TIMER1_STOP`, read and clear `EVENT_STATUS` (the previous
  operation is torn down), the PIB if it changed (`CHANNEL` = `3 + 5·(ch−11)`, `TXPOWER`,
  `ED_CFG`, `CONF`), `DMA_TX_ADDR` = the frame buffer — `buf[0]` the PSDU length *including* the
  2-byte FCS, `buf[1..]` the MAC frame without it — then `TX_START` (`0x41`). The ISR then expects
  `TX_SFD_DONE` and `TX_DONE`; a frame with the AR bit and auto-ack on moves it to `RX_ACK`.
- **receive**: the same teardown, `DMA_RX_ADDR` = a free 129-byte buffer, `RX_START` (`0x42`). A
  frame raises `RX_SFD_DONE` (the ISR stamps it with `esp_timer_get_time`) and then `RX_DONE`
  with the buffer holding `[len incl. FCS][frame][RSSI][LQI]` — the FCS positions carry RSSI and
  LQI. The ISR hands the frame up and re-arms RX (`RX_START` again) when `rx_when_idle` is set.
  Before a TX the driver reads `RX_STATUS.rx_state`; `> 1` means a frame is coming in and the TX
  is refused.
- **stop** mid-frame raises `RX_ABORT` (reason `RX_STOP`), which the driver clears at once.

The model: `TX_START` asks the bus for the frame (the device cannot see SRAM), stamps the start
for the host, and raises `TX_SFD_DONE` after 5 bytes and `TX_DONE` after `6 + len + 2` bytes of
air (32 µs each). A frame offered while listening raises `RX_SFD_DONE` at once and `RX_DONE`
after `1 + len + 2` bytes, the buffer written by the bus as the driver reads it. The two radio
timers count microseconds to their thresholds. ED keeps the synthetic scene from before. **Not
modelled** (stage 2 of the lock-step plan): auto-ACK either way, CCA (`CCA_TX_START` sends as
if clear), address filtering (every frame is delivered, promiscuous or not), pending bits,
security, enhanced ACKs. `--debug ieee802154` narrates commands, events and DMA; `--log-periph`
reports the first touch of an offset the model does not interpret.

## Cooja-NG lock-step: `--cooja`

`esp32sim-c6 --cooja` makes the C6 an *external mote* of Cooja-NG (csim,
`docs/design/external-nodes-plan.md` §4): csim owns the clock and drives the guest over NDJSON on
stdin/stdout — `hello` once, `step {t, in}` per slice, `stop`; one `done {t, wake, out}` per
message back. Times are nanoseconds; ours is the bus cycle counter at 6.25 ns per cycle, exact
integer arithmetic both ways, monotonic across a chip reset.

```sh
# what csim runs (the config's "firmware" string); csim writes hello/step/stop on its stdin
B=~/work/esp32/esp32-contiki/build-nullnet
esp32sim-c6 --cooja --boot rom --flash-mb 2 --stub bb_init=0 \
    --bootloader $B/bootloader/bootloader.bin --ptable $B/partition_table/partition-table.bin \
    --app $B/esp32-blink.bin --elf $B/esp32-blink.elf
```

Exactness is the point, not "within a slice":

- a `step` runs the guest to the first cycle at or after `t` (`Machine::run_until_cycle`), not to
  the end of a 64-instruction quantum;
- the round is cut at the instruction that writes `TX_START`: the `tx` event carries that time,
  and the reply goes out before the slice ends (`SocBus::take_host_event`). The rest of the slice
  is run when csim steps again (`wake` = `t + slice` while the guest is busy);
- an `rx` inside a slice is injected at its own time: the guest runs to `rx.t`, the frame goes to
  the radio (SFD then, `RX_DONE` after the air time), the run continues;
- a guest in `wfi` costs nothing: time jumps to the bus's next device deadline (systimer, TIMG,
  the radio's countdowns, a running RMT channel) or the slice end, whichever is first, so the
  FreeRTOS idle loop is free and `wake` is the deadline;
- nothing reads a host clock or host randomness — the MAC comes from the node id (`--mac` to
  override), the RNG is the model's xorshift — so the same NDJSON in gives the same NDJSON out,
  byte for byte (`cli/tests/cooja.rs` checks it twice over).

Console lines go out as `log` events (UART0 by default, `--console` picks), stamped within 20 µs
of the newline. Radio state changes are `radio` events. Serial input (`serial` events) reaches the
USB-Serial/JTAG console. The end-of-run figures go to stderr.

Measured with the esp32-contiki nullnet probe (ROM → bootloader → FreeRTOS → Contiki-NG, a
broadcast every 5 s, one injected): 10.3 s of simulation in ~0.1 s wall with 10 ms steps — the
guest is idle 99% of the time, and an idle second costs ~1 ms of wall time per systimer tick
it wakes for. A busy second (boot) runs at ~160 M instructions/s.

## The board: `--board waveshare-c6-lcd147`

The Waveshare ESP32-C6-LCD-1.47 wires an ST7789 172×320 panel to SPI2 (MOSI 6, SCLK 7, CS 14,
D/C 15, RST 21, backlight 22 on LEDC), a WS2812 to GPIO 8 over the RMT, a BOOT button on GPIO 9
and a TF card to the same SPI bus (MISO 5, CS 4; not modelled). `esp32c6/src/board.rs` is the
board: an `St7789` fed the SPI bytes with the D/C GPIO level (commands and parameters are
separate transactions, and the driver sets D/C right before each one, so the bus delivers GPIO
edges to the board before the bytes that follow them), the LED decoded from the RMT frame, the
button as `press boot 150` in a script.

The firmware that lives on this board — the owner's IEEE 802.15.4 energy scanner
(`examples/waveshare-c6-lcd147/`, an LVGL spectrum of channels 11–26) — runs end to end:

```sh
ENERGY_SCAN_DIR=~/work/esp32/energy_scan examples/waveshare-c6-lcd147/run.sh --max-seconds 8 --tft-png lcd.png
```

- The ST7789 goes through `esp_lcd`'s SPI panel IO: every transfer is a GDMA out-channel
  descriptor chain (the driver enables DMA for the bus), so the C6's GDMA layout and the SPI's
  DMA data phase both had to exist before the first pixel arrived. LVGL flushes ~500 frames a
  second here.
- The WS2812 driver (`led_strip` over RMT) blocks on the transmit-done interrupt; the RMT
  frames are decoded to bits by the shared model.
- `esp_ieee802154_energy_detect` programs a duration, issues `ED_START` and takes the `ED_DONE`
  interrupt; the model completes the scan after the programmed symbols with a synthetic 2.4 GHz
  picture — a −93 dBm floor with the three non-overlapping WiFi channels (802.15.4 channels
  11–14, 16–19, 21–24) sitting on it. The picture moves: every 2.5 s a network changes level or
  a burst lands on a channel and fades, and levels drift 1 dB per 100 ms toward their targets,
  all from one xorshift so a run is reproducible (`Ieee802154::set_channel_dbm` sets a level
  outright). ~260 scans in 8 s, 16 bars on the screen.
- The PHY blob's init runs, with one stub: `--stub bb_init=0` skips the baseband calibration
  (TX DC, IQ, power-detector tones), a set of hardware handshakes on undocumented registers at
  `0x600A0000` that spin forever without them. Everything before it (`rf_init`, the parameter
  tables the PLL-tracking timer later dereferences) runs; stubbing `register_chipv7_phy` as a
  whole crashed that timer. The 802.15.4 MAC model does not depend on the PHY. The blob's own
  `wait_i2c_sdm_stable` polls the RF block over regi2c for 0x5b, which the analog master answers.
- The app writes its PHY calibration into NVS on first boot ("Saving new calibration data"),
  through the shared flash model.

`--stub` and `--trace-fn` are exact on the RISC-V chips now: the core stops at the addresses the
machine wants to see, as the Xtensa block interpreter always did.

## Not there yet

- **TF card, backlight PWM.** The SD slot on SPI2 has nothing behind it; the LEDC backlight is
  register RAM (the panel is shown regardless).
- **The PHY's baseband calibration** (above): a stub, not a model.
- **`--boot app`** maps the image through the unified MMU and jumps to it, but the system
  registers the bootloader would have set up are not preset; ROM boot is the tested path.
- **Watchdogs.** The LP_WDT and the TIMG watchdogs are register RAM: they never fire.
- **WiFi 6, BLE, the LP core** — nothing of those radios or the second core is modelled. The
  802.15.4 MAC sends and receives whole frames (above) but has no auto-ACK, CCA or address filter yet.
- **Peripherals on demand**: GDMA, I2C, SPI2, LEDC, RMT, ADC, TWAI, PARL_IO. Each shows up as an
  unknown register with `--log-periph` the moment a firmware wants it. The registers hello_world
  still touches without a model are PMU, IO_MUX, HP_SYSTEM, APB_SARADC and a few LP blocks —
  all of them configuration the firmware only writes and reads back.
