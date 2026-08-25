# Design decisions and gotchas

Things that cost real time to find out, recorded so they do not have to be found twice.

## Core
- **Window overflow size** is the distance to the next set WindowStart bit, not the CALL
  increment. `CALLn` writes `a[n*4]` and that write is what triggers the spill.
- **Zero-overhead loops**: the loop-back happens only on fall-through to LEND, not when a
  branch inside the body targets LEND.
- **ROM data lives at its physical address**: the mask-ROM ELF's writable sections must be
  loaded and the reset handler's copy table back-filled, otherwise ROM code reads garbage.
- **Interrupt lines must be up to date before the next instruction** — recompute on register
  writes that flag `irq_dirty`; the 32-cycle poll is only a backstop for sources that change
  on their own (timers, DMA).
- **`salt/saltu`, `s32nb`, `any4/all8` operand masking, MAC16 `mx/my` bit positions** were
  all missing until the IDF 5.5 toolchain's output was run through the objdump test.
  The decoder test over several real binaries is the cheapest correctness tool we have.
- **PIE encodings come from the TRM, not from guessing**: the "Instruction Word" tables are
  machine-readable after `pdftotext`; the generated table is cross-checked by assembling
  every mnemonic with the real assembler. 24-bit forms live in op0 = 4, 32-bit in 0xE/0xF.
- **Coprocessor numbering**: FPU is CP0, PIE is CP3; both are gated by CPENABLE so FreeRTOS's
  lazy context save works — do not execute PIE with CP3 disabled.

## SoC
- **Derived clocks need delivered-tick accounting**: compute ticks from the running cycle
  total and deliver the difference; per-quantum rounding otherwise drifts and timers fire late.
- **`esp_restart()` on IDF 5 does not write a reset bit** — it arms the RTC watchdog through
  ROM `wdt_hal` and spins. Model the watchdog (stages, feed, write-protect key) and the
  `SW_PROCPU_RST` bit; reboot from the ROM with the right cause instead of stopping.
- **JEDEC capacity must follow `--flash-mb`**, or IDF's flash probe refuses a 16 MB image.
- **Octal PSRAM** is a device on SPI1 chip-select 1 speaking 16-bit commands
  (0x4040 read MR, 0xC0C0 write MR, 0x8080/0x0000 sync write/read); vendor 0x0D, density 3
  = 8 MB. IDF verifies with a write/read of `0x5a6b7c8d` at address 0.
- **GDMA in-channel registers sit at channel×0xC0 + 0x00…**, out-channels at +0x60…;
  `IN_PERI_SEL` is +0x48. The camera is trigger 5 on the in side.
- **LCD_CAM `cam_start` is CTRL1 bit 29 and takes effect directly** — not gated by
  `CAM_UPDATE`; `cam_ll_start` sets update first, then start.
- **`cycles * 1e9 / CPU_HZ` overflows u64 at 76.86 s** — keep wall/emulated time in f64.
- **WebSocket sends must not block the emulator thread** — a frozen browser tab stalled
  emulation until sends moved to per-client writer threads with bounded queues.

## Boards and firmware
- **Atech's own Pocket Synth build drives the ST7735 over hardware SPI2 and reads the encoder
  with PCNT**; our PlatformIO build of the same sketch bit-bangs SPI and uses a GPIO ISR (older
  SDK modules). Both paths are modelled; the display decoder accepts bytes from either.
- **The real board boots app1 (0x340000)**: its partition table is the Arduino 8 MB OTA layout and
  `otadata` selects the slot with the higher sequence — a 1 MB flash dump is not enough, and
  `pio run -t upload` alone would not change what runs (erase `otadata` at 0xE000 first).
- **ST7735 output is decoded from bit-banged GPIO**: a full redraw is ~4.8 M instructions per
  20 ms (100 % of core 1), the one place the Pocket Synth firmware cannot run at real time
  in an interpreter (~70 Minsn/s dual core vs 240 needed). The UI absorbs it.
- **Direct-audio/PIE firmware runs ~55–75 Minsn/s**; when a firmware spins (WiFi PHY
  calibration with no RF) it eats half the emulator — see networking-plan.md.
- **The generated Pocket Synth firmware's waveform button was a stub** (all names
  "TRIANGLE", oscillator ignoring `waveform`); the emulator was faithful. Now replaced by
  the SID engine in `boards/atech14/firmware/lib/sid`.
- **Chrome remote-control clicks are not user gestures**: WebAudio stays suspended in
  automated tests; verify audio by WAV capture, not by listening.

## Process
- Bring-up loop: run → first unknown register / unimplemented instruction (`--log-periph`,
  `Unimplemented(pc, raw)`) → model it → rerun. Keep the objdump test and the hardware
  differential test green after every core change.
- Reference emulator for behaviour questions: Espressif's QEMU (`~/.espressif/tools/qemu-xtensa`),
  never for code.
