# ESP32-S3 Tier B hardware candidates

This directory adopts the four canonical captures from the 2026-09-01 Tier B
hardware session on the ESP32-S3 QFN56 revision v0.2 board. The source is
TinyDraw `fc6d9347549730a0e57aa926f8f6935e12636844`, built with ESP-IDF v6.1
and xtensa-esp-elf 15.2.0. The source archive is
`/Users/sarah/Archives/esp32s3/2026-09-01-tier-b`.

The archive's complete `SHA256SUMS` passed before adoption. The committed copy
is pinned by SHA-256
`d285d968e3d7a0cb5f4edb6b71327aa20274280892604232ff119fc0924f97eb`.
`archive-reference.json` records the archive-only ELF, binary, and sdkconfig
artifacts. The committed preflights verify the exact issue-block encodings,
instruction placement, and manifest binding for both firmware variants.

## Canonical cohort

| Variant | Boot | Boot identity | Cells | Samples | Refusals | Raw SHA-256 |
| --- | ---: | --- | ---: | ---: | ---: | --- |
| normal | 1 | `11-e2a567863778958d` | 25 | 198 | 0 | `3e857c4d491b3ea4f2fc1c60dee8d9cb76c886d9bfd26a2e7457259bd2fe4148` |
| normal | 2 | `11-d04491648c82d894` | 25 | 198 | 0 | `e96aa32d54efd59ad3f1a2de81f7773b26362c8de6a35a60d17f873000cf3faa` |
| XIP PSRAM | 1 | `11-878cb8beaa06294f` | 26 | 211 | 0 | `1a5020dc19112a63e5fe2e69b247c58fdc3a96393144c857b991b0ae41376fe0` |
| XIP PSRAM | 2 | `11-951f35cbfbd360f8` | 26 | 211 | 0 | `2bc080b899e7eabb5664fe7fa48ffc51176a0ffc5f755d9060bbd0772b817540` |

The four boot identities are distinct. Independent offline validation checks
the exact manifest cell order, every ordinal and sample count, terminal
tallies, zero refusals, raw and sidecar hashes, and agreement among runtime
metadata, ELF preflight, session metadata, sdkconfig, toolchain, source commit,
and archived artifacts. GPIO 21 is the single manifest cell excluded as open.

## Summaries and classifications

`summary.json` contains min, median, nearest-rank p90, and maximum cycles for
every cell in every boot and for both two-boot pools. Even-sized medians use
the arithmetic midpoint of the two middle sorted values. All classifications
are candidate evidence scoped to one firmware variant. No value in this
directory is an adopted measured-mode cost or a cycle-accuracy claim.

Affine diagnostics use both boots. Writeback ladders fit the per-boot median
at 1, 2, 4, 8, and 16 cache lines. Transfer sweeps fit all six byte sizes from
both boots. A fit is classified affine only with R-squared at least 0.999 and
maximum absolute residual no larger than 2 percent of its observed range, or
2 cycles when that is larger.

| Variant | Family | Candidate fit | R-squared | Maximum residual |
| --- | --- | --- | ---: | ---: |
| normal | dirty writeback | 853.500000 + 161.951612903 cycles per line | 0.999972212 | 7.403226 cycles |
| normal | SPI2 transfer | 8852.979011 + 47.982820302 cycles per byte | 0.999998473 | 1483.120489 cycles |
| normal | cache msync writeback | 1246.886506 + 2.470933121 cycles per byte | 0.999878233 | 591.345233 cycles |
| XIP PSRAM | dirty writeback | 854.500000 + 161.951612903 cycles per line | 0.999972212 | 7.403226 cycles |
| XIP PSRAM | SPI2 transfer | 8809.436561 + 47.989126257 cycles per byte | 0.999998948 | 1241.259359 cycles |
| XIP PSRAM | cache msync writeback | 1177.125063 + 2.473984276 cycles per byte | 0.999868574 | 612.116551 cycles |

The clean writeback ladders, panel QSPI sweeps, GDMA sweeps, and clean
invalidation sweeps remain unexplained by this affine criterion. Fixed-size
cells are classified exact only when every pooled sample agrees, interval only
for a one-cycle pooled range, and distribution otherwise. The detailed values
and all non-affine fit diagnostics are in `summary.json`.

All seven earlier attempts, including attempt 7's earlier successful normal
corroboration, remain noncanonical hash references in `archive-SHA256SUMS`.
They are excluded from every tally, statistic, and fit in this receipt.

Verify the committed receipt with:

```text
shasum -a 256 -c SHA256SUMS
python3 analyze.py > /tmp/tier-b-summary.json
diff -u summary.json /tmp/tier-b-summary.json
gzip -t captures/*.log.gz
jq -e . summary.json session-metadata.json archive-reference.json receipts/*.json preflight/*.json probe-cells.json
```

With the source archive present, reverify all archived bytes with:

```text
(cd /Users/sarah/Archives/esp32s3/2026-09-01-tier-b && shasum -a 256 -c SHA256SUMS)
```
