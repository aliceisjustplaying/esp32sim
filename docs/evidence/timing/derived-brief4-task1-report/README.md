# Brief 4 Task 1 report

Date: 2026-09-02

## Commits

- `0f12e94`: derive reset clock configuration from the 40 MHz XTAL.
- `491d4e6`: scope internal prices and remove the SPI2 sequence price.
- `78d7b63`: record the conflicting mask-ROM fetch residuals.
- `7c96315`: record the blocked exception timing derivation.
- `38518e1`: record the measured-boot histogram and stop.

## Part 0

Reset derives CPU and APB clocks as 40 MHz. The asserted configuration has
flash mode `Other` at 160 MHz, PSRAM mode `Other` at 160 MHz, a 16 KiB
four-way 16-byte-line I-cache, and a 32 KiB eight-way 16-byte-line D-cache.
`cpu_mhz: 0` is no longer emitted.

## Part 1

The zero-length `memset` cell has 31 matched and 34.5 known priced cycles over
16 ROM fetches: residual -3.5 cycles, or -7/32 per fetch. The 0x52e0-byte cell
has 6,659 matched and 6,664.5 known priced cycles over 6,646 ROM fetches:
residual -5.5 cycles, or -11/13,292 per fetch.

The candidates differ, are fractional, and are negative. Cross-cell validation
fails R8, so no mask-ROM row is adopted. Both paths stay in ROM ELF `.text`
from `0x400570c8` through `0x40057112`.

## Part 2

Level 1 entry targets 227 cycles and stops at `l32r` after a 17-cycle known
prefix. Level 1 resume targets 143 and stops there after five known cycles.
Because `l32r` has only an interval price of 1 to 2 cycles, exact E and R
cannot be derived.

Level 3 entry targets 222 cycles and stops after a 12-cycle prefix. Level 3
resume targets 139 and stops after five. The window handlers contribute 18
known cycles against the 35-cycle pair target. All three validation residuals
are unavailable without E and R. Nothing is adopted, and the correlation
remains ignored with that reason.

## Part 3

The histogram has two byte-identical runs. Each stops at boot cycle 0 with
core cycles `[0, 0]`, one `MaskRomInstructionFetch` refusal on core 0 at
`0x40000400`, symbol `_ResetVector`, and `ready: false`.

Product ELF SHA-256: `7f598fd3580cf52078fb6aa04a5f6fe5179b0de9d89bb6468fdb06ed5e40e424`.
ROM ELF SHA-256: `c0ce0f338d1de1bdc6efbef1591779a2a42c1ab7d759d3c6ae8ae63a7dd34cfd`.

Exact stopping condition: "Part 3 stops at the first refusal that is neither
an engine bug nor R8-derivable, or when READY is reached." The mask-ROM refusal
meets that condition, so the stop rule fired at `_ResetVector`.

Both analyzers reproduce their `summary.json` byte for byte. The full debug
and release push gate passes with the fixture-backed environment.
