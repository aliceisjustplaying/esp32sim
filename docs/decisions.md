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
- **LCD_CAM RGB output**: timing registers hold value−1; pclk = src/(div_num + b/a)/(clkcnt_n+1),
  clk_sel 3 = PLL160M, 2 = PLL240M; the engine's 16-word async FIFO runs ahead of the pixel clock and
  the RGB driver relies on that when it restarts the DMA link mid-frame (skips FIFO depth + 1 pixels) —
  without the lookahead the picture drifts 17 px per restart; a one-byte misalignment byte-swaps colours.
- **Touch controllers must latch**: LVGL polls the GT911 every 30 ms; a UI click shorter than that must
  stay readable until the driver has seen it, or taps are lost.
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

## WiFi and networking
- **Emulate the MAC, do not shim `esp_wifi`.** A shim (or the OpenCores Ethernet MAC IDF ships a
  driver for) needs a firmware config change, so the binary under test stops being the binary that
  runs on the board. Modelling the MAC registers keeps "unmodified firmware" literally true, and
  the blob's state machine then has to be walked to CONNECTED by frames a real AP would send —
  no register shortcuts it.
- **A pure-Rust NAT, not libslirp.** Terminating TCP/UDP in the emulator and relaying over ordinary
  host sockets — the way Contiki-NG's NAT64 does — is a few hundred lines, adds no C dependency and
  needs no root, tun device or entitlement. The cost is what user-mode NAT always costs: no
  multicast/mDNS, and inbound needs explicit forwarding.
- **`DSCR_RELOAD` (0x60033084 bit 0) must not rewind the RX pointer.** Software rewrites
  `BASE_RX_DSCR` every time it recycles descriptors; treating that as "restart here" made every
  second frame land in a descriptor the stack had moved past, so it was batch-recycled instead of
  indicated (`wDev_ProcessRxSucData` `a3=0xa` rather than `0x1`).
- **`rx_ctrl` word 0 must set filter-match bit 28**, plus bit 29 for unicast. With bit 29 alone the
  frame is dropped inside `wDev_ProcessRxSucData` before it is ever indicated — silicon shows
  `0x111b20ad` for a broadcast beacon.
- **The EAPOL MIC covers exactly the 802.1X frame**, not the whole 802.11 payload, which can carry
  trailing bytes; and **group-addressed downlink frames need CCMP key id 1** (the GTK) or the
  station drops them silently.
- **Trim IP payloads to the header's total-length field.** An 802.11 frame carries a 4-byte FCS;
  without trimming, the NAT hands those bytes to the peer as TCP payload and the guest's real
  request then arrives at a sequence number the connection has already passed.
- **Debugging aid that paid for itself**: rebuild the specimen firmware with
  `CONFIG_WPA_DEBUG_PRINT=y` and `CONFIG_LOG_MAXIMUM_LEVEL_DEBUG=y` and read the supplicant's own
  verdicts instead of guessing from the emulator side.

## Crypto accelerators
- **They are not optional.** mbedTLS and the WPA supplicant route everything through hardware, so a
  missing or subtly wrong accelerator does not raise an error — it hangs in a polling loop or
  returns a plausible wrong answer. WPA2 died at handshake message 3 without AES; TLS hung in the
  MPI driver without RSA; certificates failed to verify with a wrong SHA.
- **RSA `0x818` is an idle status, not the interrupt latch.** It reads 1 whenever the unit is done
  and stays 1; `0x81c` clears only the interrupt signal. Model it as a latch and every
  interrupt-driven `mbedtls_mpi_exp_mod` deadlocks — the ISR clears the flag, then the result path
  waits for it forever. `0x808` (QUERY_CLEAN) must read 1 or firmware spins before the first op.
- **Compute the arithmetic exactly, ignore Montgomery.** The silicon works in the Montgomery domain,
  which is why the driver also loads M' and R⁻¹; a model that computes `X*Y mod M` and `X^Y mod M`
  directly produces the same results the driver expects, including the failover case where it sets
  M = 2^n − 1, M' = 1, R⁻¹ = 1 to get a plain multiply.
- **mbedTLS hashes through GDMA and asks for SHA-384**, so the block interface alone is not enough
  and the 64-bit SHA-512 core is required. H_MEM words read back byte-swapped, 64-bit state stored
  high-half first, so the driver's plain `memcpy` yields digest order.
- **AES CTR (block mode 3) is used by the TLS record layer**; executing it as ECB produces traffic
  the server answers with a fatal alert rather than anything diagnosable.
- **Check the primitives against published vectors, not against the firmware.** RFC 3174/2202/3394,
  FIPS-197, 802.11i Annex H, and 2048-bit modexp vectors generated with Python — every one of these
  bugs above would otherwise have looked like "the network is broken".

## Performance
- **A host syscall in the tick costs more than emulating the CPU.** The NAT polled its sockets every
  scheduling round — ~7.5 M `recvfrom` calls per emulated second — which put 69 % of run time in the
  kernel and only 26 % in the interpreter. Polling on an emulated-time cadence (500 µs, far below
  anything the guest's TCP stack notices) made WiFi workloads 3× faster.
- **Nothing per-instruction may hash.** `--stub` and `--trace-fn` looked their PC up in a `HashMap`
  on every instruction; SipHash alone was 16 % of run time. A 64-bit bloom bit (`1 << ((pc >> 2) & 63)`)
  in front of the map removes it, and the map stays the authority.
- **Scheduling quantum 64**, not 32: half the device-tick overhead for ~9 % more throughput, and the
  Atech WAV regression stays bit-identical. 128 gains almost nothing and costs interrupt latency.
- **Profile the emulator, not just the guest.** `--profile` reports guest PCs and disables idle
  skipping, so an idle core shows up as a hot `waiti` — an artefact, not work. For emulator-side
  cost use `sample <pid>` (macOS) against a normal run.
- **Check for leftover runs before benchmarking.** A background emulator from an earlier session at
  100 % CPU is indistinguishable from "the emulator got slower".

## Process
- Bring-up loop: run → first unknown register / unimplemented instruction (`--log-periph`,
  `Unimplemented(pc, raw)`) → model it → rerun. Keep the objdump test and the hardware
  differential test green after every core change.
- Reference emulator for behaviour questions: Espressif's QEMU (`~/.espressif/tools/qemu-xtensa`),
  never for code.
