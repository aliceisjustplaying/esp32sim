# Goal

A browser-hosted, cycle-accurate emulator of the Waveshare
ESP32-S3-Touch-AMOLED-1.8 V2 board, running in real time on a 2021 M1 Pro
MacBook. Built on esp32sim (this fork); the execution engine, SoC
models, and web shell all live here.

"Cycle-accurate" is an umbrella claim over the timing behavior firmware
can observe, at the tiered bounds below. It covers:

- the dual-core Xtensa LX7 CPU (issue rates, window exceptions, loop
  alignment, interrupt entry and resume),
- the memory hierarchy: SRAM and IRAM speed, ROM, flash and PSRAM
  reached through the caches and the shared MSPI path (latency,
  bandwidth, line-fill costs, and cross-core plus DMA contention),
- MMIO access costs,
- the display path: the CO5300-class QSPI AMOLED display controller
  and panel, including the tear signal,
- the touch controller: CST820, adopted from the on-device identity
  probe (receipt in `evidence/board-touch-identity-2026-09-01/`).

The fidelity target is what firmware observes: CCOUNT deltas,
interrupt timing, and register-visible device behavior. Wire-level
electrical behavior is out of scope unless it becomes visible to
firmware.

Board scope is exactly the Waveshare ESP32-S3-Touch-AMOLED-1.8 V2:
CO5300 panel and CST820 touch. The V1 board carries SH8601 and FT3168
and is out of scope (Waveshare documents the split at
https://docs.waveshare.com/ESP32-S3-Touch-AMOLED-1.8; the touch identity
receipt confirms V2 on the physical board). Other ESP32-S3 board
configurations require their own configuration-dependent receipts and
are refused by name when unmatched.

## Definition of done

The emulator boots the board's real merged firmware image (real ELF
plus the real mask ROM) on both cores, with display and touch working
in the browser, at real time on the M1 Pro, with cycle accounting that
passes a silicon correlation suite at these bounds:

- exact on SRAM-resident kernels,
- within 1 percent on frame-scale workloads,
- distribution agreement on RTC and PSRAM paths.

The frame-scale target is 1 percent. Work below that scale is not held to
a 0.1 percent target. PSRAM path timing varies between boots by more
than 1 percent on identical code (the idle PSRAM-to-SRAM copy measured
106,024 and 107,545 cycles on two boots,
`evidence/timing/esp32s3-dma-sram-2026-09-02/`), so the frame-scale
bound applies to the non-PSRAM component of a frame; the PSRAM
component is held to distribution agreement at any window length.

## End state: one mode, fast and accurate

The product is a single mode that is both real-time and
cycle-accounted: a JIT with timing accounting compiled into generated
code. Two supporting modes exist along the way and remain useful:

- Fast mode: upstream's behavior, unchanged, never taxed, uncosted.
  Not real-time in the browser: the wasm build is interpreter-only at
  about 105 MIPS against the 480 MIPS budget
  (`evidence/browser-speed/`). Native fast-mode throughput is
  unmeasured.
- Measured interpreter: the reference implementation of timing.
  Correct first, slow is fine. The costed JIT must reproduce its
  ledgers exactly.

Feasibility receipts (docs/evidence/browser-speed/): the worst-case
real-time budget is 480M emulated instructions per second (two cores at
240 MHz at one instruction per cycle; stalls and idle cores lower the
real requirement); the browser
interpreter measured about 105 to 109 MIPS (real time refuted for the
interpreter); the JIT ceiling measured about 4,400 to 4,600 MIPS in
Chrome on the target hardware. The open risk is how much of that
ceiling cycle accounting consumes; measuring that early is a
milestone, not an afterthought.

## Architecture stances

- Dual-core native from day one. There is one I-cache and one D-cache,
  both shared by both cores, with each core on its own bus (ESP32-S3
  Technical Reference Manual, Cache chapter). Cache configuration
  (size, ways, line bytes) comes from `ChipConfig`. The scheduler, MSPI
  arbitration point, and ledger carry two cores structurally. Core 1 is
  live from boot: TinyDraw pins its touch sampler, USB, and NTP tasks
  to core 1 (`esp32/main/hardware_app.cpp`, `usb_export.cpp`,
  `time_sync.cpp`, `vector_v2/vector_v2_touch_sampler.cpp`).
  Single-core receipts are adopted first; contended receipts are
  adopted in milestone 5. There is no single-core phase to retrofit
  later.
- Fail closed. Unknown costs block totals. Refusals are typed and
  name their tier candidate.
- Costs come only from committed hardware receipts
  (`docs/evidence/`). Correctness gates are Rust tests that replay
  committed traces and assert directly against adopted receipt
  numbers. There is no external oracle.
- Timing state is typed (no string-encoded mutations in hot paths) and
  designed so per-block costs can compile into JIT-generated code.

## In scope, deferred

Wanted, but only after the core product works (initialization stubs
are acceptable until then, labeled as stubs):

- IMU, PMIC, and RTC behavioral models (QMI8658, AXP2101, PCF85063;
  easiest first),
- the PWR and BOOT buttons,
- radio and networking.

Out of scope until a task names it: audio (the board's microphone and
speaker).

## Milestones

1. Integration trunk: harvest the `salvage/*` branches under review
   into `alice` (safeguards, board model, measured-mode material that
   survives its review findings).
2. Measured interpreter, dual-core native, with receipt-correlation
   tests green on the adopted cost classes.
3. Wasm JIT spike with cycle accounting compiled in; measure the real
   margin against the 480 MIPS budget on the M1 Pro.
4. TinyDraw boots, draws, and responds to touch in the browser.
5. Contention calibration: contended-cohort receipts adopted, cross
   core and DMA arbitration correlated.
6. Correlation suite passes at the definition-of-done bounds.
