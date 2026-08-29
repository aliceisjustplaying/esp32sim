# ESP32-C3 (RISC-V) — draft

A second chip in the same workspace: one RV32IMC core at 160 MHz instead of two Xtensa LX7s.
Unmodified ESP-IDF firmware boots from the real mask ROM, through the real 2nd-stage bootloader,
into FreeRTOS.

```sh
cargo build --release
H=examples/hello_world-c3/build
./target/release/esp32sim-c3 --boot rom --flash-mb 4 \
    --bootloader $H/bootloader/bootloader.bin --ptable $H/partition_table/partition-table.bin \
    --app $H/hello_world.bin --elf $H/hello_world.elf --max-seconds 26
```

prints the ROM banner, the bootloader's partition table, the IDF startup log, `Hello world!`, the
countdown, and then reboots through the ROM on `esp_restart()` — the same end-to-end path the S3
`hello_world` test covers:

```
ESP-ROM:esp32c3-api1-20210207
rst:0x1 (POWERON),boot:0xf (SPI_FAST_FLASH_BOOT)
I (4) boot: ESP-IDF v5.5.4 2nd stage bootloader
I (4) boot: chip revision: v0.4
I (28) main_task: Calling app_main()
Hello world!
This is esp32c3 chip with 1 CPU core(s), WiFi/BLE, silicon revision v0.4, 2MB external flash
```

The mask ROM ELF is picked up from `~/.espressif/tools/esp-rom-elfs/*/esp32c3_rev3_rom.elf`
(shipped with ESP-IDF); override with `--rom`. Flags mirror the S3 binary — see `--help`;
`--trace`, `--break`, `--watch`, `--peek`, `--disasm` and `--log-periph` all work and print
RISC-V mnemonics with symbols.

## What is modelled

| | |
| --- | --- |
| CPU | RV32IMC, machine mode, `mstatus`/`mtvec`/`mepc`/`mcause`/`mtval`, vectored traps, WFI |
| Interrupts | the C3 interrupt matrix: 62 sources → 31 lines, per-line priority and threshold, level and edge, plus the four `FROM_CPU` software interrupts |
| Memory | 400 KB SRAM (SRAM1 dual-mapped IRAM/DRAM), mask ROM, RTC slow RAM, 8 MB flash cache windows through a 128-entry MMU |
| Peripherals | UART0/1, USB-Serial/JTAG, systimer, TIMG0/1 + WDTs, GPIO, RTC_CNTL, efuse, SPI0/1 flash controller, GDMA, SHA/AES/RSA, cache controller, hardware RNG |

Peripheral models are **shared with the `esp32s3` crate** wherever the IP is identical (which is
most of it — same UART, same systimer, same timer groups, same USB-Serial/JTAG, same SPI flash
controller). Only the address map, the cache controller and the interrupt matrix are C3-specific.
`esp32c3` therefore depends on `esp32s3`; if a third chip ever appears, lift those models into
their own `esp-periph` crate rather than deepening that dependency.

The RV32IMC decoder is checked against `riscv32-esp-elf-objdump` the same way the Xtensa one is:
161,388 instructions from the C3 mask ROM and a real app, 0 mismatches.

```sh
OD=~/.espressif/tools/riscv32-esp-elf/*/riscv32-esp-elf/bin/riscv32-esp-elf-objdump
$OD -d ~/.espressif/tools/esp-rom-elfs/*/esp32c3_rev3_rom.elf > /tmp/rom.dis
$OD -d examples/hello_world-c3/build/hello_world.elf > /tmp/app.dis
RISCV_DIS_FILES=/tmp/rom.dis:/tmp/app.dis cargo test -p riscv-rv32 --release
```

## Not there yet

- **`--boot app`** (skipping the ROM and bootloader). The C3 has one 128-entry MMU table shared by
  the data and instruction buses, and software keeps their page ranges disjoint; a direct app boot
  needs the bootloader's split, which is not modelled. The flag fails with that message.
- **WiFi/BLE** — nothing of the C3 radio is modelled. (The S3's blob-level WiFi work does not
  carry over: different MAC, and the C3's is not the one that was reverse-engineered.)
- **No board model.** The C3 target is a bare module: console only. `BoardModel` is an S3 concept
  today; a C3 board would need it lifted out.
- **Peripherals on demand**: I2C, SPI2 master, LEDC, RMT, ADC, TWAI. Each shows up as an unknown
  register with `--log-periph` the moment a firmware wants it.
- **Speed work** — the C3 bus does a plain address-range walk per access; it has none of the S3's
  software TLB, block interpreter or JIT. It still runs ~200–300 Minsn/s (well above the C3's
  160 MHz) because the workload is light, but a busy firmware would want the same treatment.

## Gotchas found bringing it up

These cost real time; they are the reason the boot log looks the way it does.

- **Interrupt source numbers come from the `INTERRUPT_CORE0_*_MAP_REG` order, not from
  `soc/interrupts.h`.** That enum omits the NMI entries, so its indices are shifted by up to 7 and
  every source lands on the wrong line — the systimer silently never fires. The register order in
  `interrupt_core0_reg.h` is the hardware's.
- **A line whose priority equals the threshold fires.** IDF enables interrupts by setting the
  threshold to 1 and allocates handlers at priority 1, so modelling the comparison as `pri > thresh`
  masks everything, forever, with no error.
- **FreeRTOS yields through a software interrupt.** `xPortStartScheduler` raises
  `SYSTEM_CPU_INTR_FROM_CPU_0` (SYSTEM + 0x28) and expects never to return. Without that register
  the yield falls through, `vTaskStartScheduler` returns, and the app lands in a `j .` trap loop
  having printed a *complete and healthy* startup log — a very quiet failure.
- **The ROM's `unpackloop` will zero its own data if you let it.** The reset path copies RAM
  initialisers out of ROM using a table of `(dst_start, dst_end, rom_src)` entries between
  `_data_start` and `_data_end`. The ELF carries the RAM copies but not the ROM-side originals, so
  the copy overwrites them with zeroes; `load_rom` back-fills the ROM side first. Symptom without
  it: a store through a null `rom_spiflash_legacy_data` about 350 instructions in.
- **The efuse wafer version lives in BLK1 bits 114 (minor low), 183 (minor high) and 184 (major)** —
  a different layout from the S3's. Get it wrong and the bootloader stops with
  `chip revision check failed. Required >= v0.3, found v0.0.` We report v0.4, which is what current
  C3 silicon reads back.
- **`ets_delay_us` busy-waits on CSR 0x802**, Espressif's user-mode cycle counter, not on a timer.
  Return 0 from it and the ROM hangs before the bootloader ever runs.
- **EXTMEM register offsets differ from the S3's**: ICACHE sync-done is bit 1 at 0x028 (the S3 has
  DCACHE there), freeze is at 0x0CC, cache state at 0x0B0. Each wrong one is a separate silent hang
  in a `Cache_*` poll loop.
- **The hardware RNG is at APB_CTRL + 0xB0**, and the bootloader's `bootloader_fill_random` spins
  on it before it will load the app.
- **The ROM mirrors its console to both UART0 and USB-Serial/JTAG**, so `--console both` prints
  everything twice; the default is `uart0`.
