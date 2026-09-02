# Brief 4 Task 1 report

Date: 2026-09-02

## Commits

- `0f12e94` derives reset clock configuration from the 40 MHz XTAL.
- `491d4e6` scopes internal timing prices independently of `ChipConfig` and
  removes the SPI2 sequence total from the price table.
- `78d7b63` records the two conflicting mask-ROM fetch residuals.
- `7c96315` records the blocked exception timing derivation.
- `38518e1` records the measured-boot histogram and stop.

## Part 0

Reset now derives CPU and APB clocks as 40 MHz. The asserted reset
configuration is CPU 40 MHz, APB 40 MHz, flash mode `Other` at 160 MHz,
PSRAM mode `Other` at 160 MHz, 16 KiB four-way 16-byte-line I-cache, and
32 KiB eight-way 16-byte-line D-cache. `cpu_mhz: 0` is no longer emitted.

## Part 1

The zero-length `memset` cell has 31 matched receipt cycles and 34.5 known
priced cycles across 16 ROM instruction fetches. Its residual is -3.5 cycles,
or -7/32 cycle per fetch.

The 0x52e0-byte cell has 6,659 matched receipt cycles and 6,664.5 known priced
cycles across 6,646 ROM instruction fetches. Its residual is -5.5 cycles, or
-11/13,292 cycle per fetch.

The two candidates differ, are fractional, and are negative. Cross-cell
validation therefore fails R8 and no mask-ROM instruction-fetch row is
adopted. The disassembled paths stay in the ROM ELF `.text` region from
`0x400570c8` through `0x40057112`.

## Part 2

The level 1 entry equation targets 227 cycles, but execution stops at `l32r`
after a 17-cycle known ledger prefix. The level 1 resume equation targets 143
cycles and stops at `l32r` after a five-cycle known prefix. Since `l32r` has
only an interval price of 1 to 2 cycles, neither exact E nor exact R can be
derived.

The independent level 3 entry validation targets 222 cycles and stops after a
12-cycle prefix. The level 3 resume validation targets 139 cycles and stops
after a five-cycle prefix. The two window handlers contribute 18 known cycles
against the 35-cycle pair target. All three residuals are unavailable because
E and R are unavailable. Nothing from the exception derivation is adopted,
and the correlation remains ignored with that reason.

## Part 3

The committed histogram contains two byte-identical runs. Each run stops at
boot cycle 0 with core cycles `[0, 0]`, one `MaskRomInstructionFetch` refusal
on core 0 at `0x40000400`, symbol `_ResetVector`, and `ready: false`. The
product ELF SHA-256 is
`7f598fd3580cf52078fb6aa04a5f6fe5179b0de9d89bb6468fdb06ed5e40e424`.
The ROM ELF SHA-256 is
`c0ce0f338d1de1bdc6efbef1591779a2a42c1ab7d759d3c6ae8ae63a7dd34cfd`.

The exact stopping condition is: "Part 3 stops at the first refusal that is
neither an engine bug nor R8-derivable, or when READY is reached." The
mask-ROM refusal is neither an engine bug nor R8-derivable, so the stop rule
fired at `_ResetVector`.

Both derivation analyzers reproduce their committed `summary.json` byte for
byte. The full debug and release push gate passes with the fixture-backed
environment.
