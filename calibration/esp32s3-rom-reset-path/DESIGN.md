# ROM reset path: inventory result and feasible measurement design

Status: inventory B done (untimed, emulator, exploratory). Measurement design for
independent challenge. Nothing built for hardware, nothing flashed, no model
change, no price. Written 2026-09-06 after the v1 and v2 ROM fetch ladders.

## Inventory B: what the reset path executes (emulator, untimed, exploratory)

Method: the unmodeled emulator with the pinned ROM, `--boot rom --trace --break
0x403c88a4` (bootloader entry of the v1 image), trace tallied by fetch region,
opcode class, branch outcome and ROM function. Raw trace, tally script, exact
command with input hashes and the output are preserved in `inventory/` (`tally.py`, `RUN.md`, `reset-inventory.txt`) and, for the
raw trace and ROM disassembly, in `~/Archives/esp32s3/rom-reset-inventory-2026-09-06/`. These counts
are instruction counts on the emulator's path; they carry no timing.

- 130,317 instructions from reset to bootloader entry, all fetched from mask ROM.
- By class: ALU 55,717; loads 23,164; branches 18,212 (12,390 taken, 5,822 not
  taken); stores 16,392; `l32r` 4,187 (all literals in ROM); special-register
  and sync 2,807; returns 2,796; calls 2,789; `entry` 2,786; `j` 1,126 (next PC
  differs from fallthrough for 1,115; for 11 the jump target is the next
  instruction, still an executed `j`); loop setup 341. Branch "taken" above
  means next PC differs from the fallthrough PC.
- By ROM function: `ets_run_flash_bootloader` 38.9 percent, `memcpy` 13.3,
  `ets_sha_process` 7.0, `uart_tx_one_char_uart` 5.2, `.stack_ok` 4.8,
  `uart_tx_one_char` 4.3, `ets_vprintf` 3.6, `usb_uart_device_tx_one_char` 3.3,
  `ets_sha_update` 3.2, `alwaysunpack` 2.4, `ets_write_char` 2.1,
  `Cache_MMU_Init` 1.6, 64-bit division helpers 3.0, `_cvt` 1.2.
- The `callx0` fast-boot path in the reset handler is not taken on this image;
  execution goes through `.noappfastboot`.

What this means, stated within what an instruction count can show: the path is
ROM-fetched code in the flash loader, image hashing and console output routines,
with 39,556 loads and stores whose effective addresses this trace does not
record (it prints a0 to a3 only), so no MMIO, SRAM or flash access-target counts
are claimed. These percentages are instruction counts, not hardware cycle or time
fractions; how much of the elapsed hardware time is device waiting versus
instruction execution is not established by this inventory. What it does
establish is the set of cost classes the measured interpreter must price to cross
the path: ROM fetch, ROM branches and jumps, ROM literal loads, special-register
writes under ROM fetch, MMIO reads and writes (writes withheld at retirement), SPI
flash register and transfer behaviour, the SHA accelerator, and console output.

## Consequence for the blocker

The current measured refusal is unchanged: cycle 0, `MaskRomInstructionFetch`,
since no price has been adopted. Symbolically, the first category the path hits
after ROM fetch, branches and literals would be an MMIO or SPI flash register
access within the first few hundred instructions; that is a reading of the
inventory, not a measured refusal. The agreed scope remains full real timing from
reset. The inventory reports that this needs the device-timing classes above,
which is more work than one ROM fetch price. One alternative exists and is
recorded with its trade-off, not as an equivalent fulfilment:

1. **Measure from reset (the agreed scope).** Requires the device-timing classes
   above, each with its own receipts and validation. The retired program's Tier B
   data on SPI2 and cache msync shows how underdetermined such decompositions can
   get, so this is a multi-stage program, not one measurement.
2. **Alternative, with its trade-off.** Run reset to a defined point in the
   unmodeled engine and start the measured ledger there, taking `CCOUNT` and
   device state from a hardware observation of that point; boot time would enter
   as a hardware-observed constant or distribution. Trade-off: this does not model
   the intermediate timing, state and device interactions of the boot path at
   all, so it is a narrower product than the agreed one, not a different route to
   the same result. It is listed so the cost of the agreed scope can be weighed
   against it, not as a recommendation.

## What the ROM ladders can and cannot identify

With the matched-twin method (v1, v2), each pair yields one joint quantity:

    D_body = (rom_body) - (iram_body)

which is the whole-sequence ROM-versus-IRAM difference for that body, including
instruction fetch, any branch or jump inside it, the target-dependent entry and
exit effects, and layout interactions. The v1/v2 result `D = 0` for ten
straight-line bodies is evidence that the joint sum of those terms is zero for
those bodies with core 1 quiet; it does not show that each term is zero
individually. Consequently:

- A body with branches gives a joint `D`, not a branch price. A not-taken variant
  of the same body executes different instructions, and two different bodies
  differ in more than their branch count, so neither is a clean control for the
  branch term. A per-branch ROM price is not identifiable from these ladders
  without an independent control that changes only the branch, which the
  immutable ROM does not offer.
- A body with `l32r` changes both the instruction-fetch source and the literal-data
  source at once. Its `D` is the joint excess of both, not an isolated literal
  load excess.
- What the ladders can deliver honestly: joint per-sequence differences for
  specific ROM sequences, usable as sequence targets (rule R2), and a
  falsifiable prediction: "a windowed call into this ROM sequence costs the same
  as into its IRAM twin", to be tested on held-out bodies. That is a prediction
  about sequences, not a set of per-instruction prices.

## Feasibility of a reset-path cycle-count target

An endpoint reading of `CCOUNT` at bootloader entry (an IDF bootloader hook exists
in v6.1) would become a usable sequence total only if all of the following are
established from the ROM ELF and the bootloader first: where `CCOUNT` starts and
whether the ROM writes it; that nothing resets, writes or wraps it before the
hook; the CPU clock in force across the interval (reset configuration is 40 MHz,
so the count is in 40 MHz cycles until the bootloader changes the clock); and the
exact instruction and device state at the endpoint. Given the inventory, a valid
total would cover flash-loader, hashing and console routines whose hardware time
is not characterised here, so it would test far more than ROM fetch. It is
therefore not proposed as the next measurement. It is recorded as the endpoint candidate for option 2
above, where its role is to fix the origin, not to price anything.

## Proposed next step (for challenge, not approved)

No hardware capture is proposed from this document. Under the agreed scope the
next measurements are device-timing classes on the reset path (SPI flash
register writes and transfer completion, SHA accelerator, console output, MMIO
writes), each a separate bounded design with its own identification equation
and controls; the order should follow the inventory's first symbolic unsupported
category. The alternative above is recorded only for the owner to weigh.

Parked: the v1 `clzdi2` mechanism (needs a same-binary runtime-switch design);
rotation-only ladders (do not close the identified gap).
