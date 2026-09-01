# ESP32-S3 Tier B decomposition candidates

This receipt records the 2026-09-01 cache-msync and SPI2 decomposition cohort
from ESP32-S3 QFN56 revision v0.2 silicon. The source is clean TinyDraw
`7a157d44a9da3312b1ecda2b45b116af2de28e63`, built with ESP-IDF v6.1 and
xtensa-esp-elf 15.2.0. The source archive is
`/Users/sarah/Archives/esp32s3/2026-09-01-tier-b-decomposition/canonical-7a157d4`.
Its complete checksum file passed and has SHA-256
`f8fdad6a863d6484e1a29ab3a103a3f36d5b29005111c0c4108b538a0ea3653a`.

## Canonical cohort

| Variant | Boot | Boot identity | Cells | Samples | Refusals | Raw SHA-256 |
| --- | ---: | --- | ---: | ---: | ---: | --- |
| normal | 1 | `11-e098c0b3353cc871` | 43 | 360 | 0 | `f5f84ce39a1a6390c186ab251f9248eab72e7e1f43619ccb48f0d5a51d41623c` |
| normal | 2 | `11-50f6ad54f11b5c37` | 43 | 360 | 0 | `f1a3c48810e0959d0ee76be90e431958dea17363d79836a894ac1a1e7a4008ce` |
| XIP PSRAM | 1 | `11-e0dfb6331da2978f` | 44 | 373 | 0 | `d032f095432ab461cb431ae8f7ff7ac6c510e554687e08b10c99005d04f99271` |
| XIP PSRAM | 2 | `11-bce20af617052ee2` | 44 | 373 | 0 | `7572d2291c08801764c883556d4ae0d9307489a08a4b6618f46d3146fc6d348b` |

The four boot identities are distinct. `analyze.py` independently checks every
manifest cell, ordinal, count, terminal tally, refusal count, receipt hash,
runtime and ELF provenance field, clock readback, PSRAM service counter, and
SPI2 phase reconciliation. Large ELFs, binaries, and sdkconfig files remain
archive-only and are pinned in `archive-reference.json`.

## Analysis method

`summary.json` is generated deterministically with exact-rational matrix rank
and least squares. Each primary fit uses one median for every condition in each
independent boot and firmware variant. Raw-repeat residuals and per-cell
minima, medians, nearest-rank p90 values, and maxima remain alongside the fit.

Classifications use exact, affine, interval, distribution, or unexplained.
An affine fit requires R-squared at least 0.999 and maximum median residual no
larger than 2 percent of its observed range, or 2 cycles when larger. A
well-fitted total does not make its components affine. No value here is an
adopted product cost or a cycle-accuracy claim.

## Cache-msync decomposition

The full `[1,L,D,S40,L*S40,D*S40]` design has rank 6. Its total median fit is:

```text
cycles = 869.113612667
       + 1.241970872 * addressed_lines
       + 161.334240342 * dirty_lines
       - 7.301527805 * slow_40mhz
       + 0.014719868 * addressed_lines * slow_40mhz
       + 125.245246779 * dirty_lines * slow_40mhz
```

The total fit has R-squared 0.999999625077 and maximum median residual
56.080863 cycles, but it is classified unexplained because its separable
matched-clean baseline fails the affine threshold. The clean baseline has
R-squared 0.993652781892, maximum residual 28.416306 cycles, and an observed
range of only 640 cycles.

The dirty-writeback delta is an affine candidate at 161.334240342 cycles per
dirty line at 80 MHz plus 125.245246779 cycles per dirty line at 40 MHz. Its
rank-2 fit has R-squared 0.999999112236 and maximum residual 105.271794 cycles
over a 146,582-cycle range. Both boot medians agree within each image; the two
images differ by at most 8 cycles at 512 dirty lines.

The transaction boundary is exactly the CCOUNT interval around
`esp_cache_msync(..., ESP_CACHE_MSYNC_FLAG_DIR_C2M)`. That source-level IDF
boundary is not yet a non-double-counted measured transaction in esp32sim, so
the affine dirty delta is not product-adopted. The independent 4 KiB,
64-miss PSRAM service control remains a distribution: 40 MHz has median 19,698
cycles and range 19,697 to 19,977; 80 MHz has median 10,860 and range 10,670
to 10,862. It corroborates the two clock conditions, not a universal line
cost.

## SPI2 phase decomposition

The phase-expanded design has rank 8. Every one of 216 samples reconciles
exactly as total cycles equal submission plus completion. Submission is a
distribution because placement and warmup effects fail the affine fit
(R-squared 0.922404216896). Completion's broad fit has high R-squared, but its
fixed cost is a distribution: 20 MHz offsets span 2,625 to 2,674 cycles and
40 MHz offsets span 2,610 to 2,920 cycles.

The device serialization slopes are narrower exact candidates:

- 20 MHz: exactly 96 cycles per byte across both 64 to 4,096 and 4,096 to
  32,768 byte intervals in all four boots.
- 40 MHz: exactly 48 cycles per byte across 4,096 to 32,768 bytes in all four
  boots. The 64 to 4,096 byte transition is a distribution from
  47.933779762 to 47.935267857 cycles per byte.

The phased total minus the prior blocking SPI2 sweep is exactly -47 cycles at
4,096 and 32,768 bytes. At 64 bytes the difference is a
distribution from -2,165 to -1,541 cycles. The phased data therefore does not
replace the prior blocking receipt wholesale. Submission and completion are
source-level IDF API intervals, not product transaction boundaries, so the
serialization candidates remain unadopted.

## Disposition

- Cache-msync total: unexplained. Dirty C2M delta: affine candidate within its
  typed hardware boundary. Clean baseline: unexplained.
- SPI2 total: unexplained. Submission: distribution. Completion fixed cost:
  distribution. Scoped device serialization: exact candidates.
- Product adoption: none. `adoptedMeasuredModeCosts` is empty.
- GPIO 21 edge timing remains open from the parent Tier B cohort.

Verify the committed receipt with:

```text
shasum -a 256 -c SHA256SUMS
python3 analyze.py > /tmp/tier-b-decomposition-summary.json
diff -u summary.json /tmp/tier-b-decomposition-summary.json
gzip -t captures/*.log.gz
jq -e . summary.json archive-reference.json receipts/*.json preflight/*.json probe-cells.json
```

With the source archive present, verify every archived byte with:

```text
(cd /Users/sarah/Archives/esp32s3/2026-09-01-tier-b-decomposition/canonical-7a157d4 && shasum -a 256 -c SHA256SUMS)
```
