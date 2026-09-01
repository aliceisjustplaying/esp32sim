# ESP32-S3 Tier A hardware receipt

This directory adopts the accepted artifacts from the 2026-09-01 Tier A
hardware batch on the ESP32-S3 QFN56 revision v0.2 board. The source archive
is `/Users/sarah/Archives/esp32s3/2026-09-01-tier-a`. Every entry in its
`SHA256SUMS` passed before adoption; `receipt.json` pins the archive manifest,
checksum file, source and tool commits, toolchain, and archive-only binaries.

The two accepted core logs are complete independent boots with one
configuration record, 29 metric records, and a terminal `CALIBRATION_DONE`.
They corroborate the committed ESP-IDF 6.1 exact values: 35 cycles for a
window overflow plus underflow pair, 1 cycle per straight-line SRAM
instruction, an additional 1 cycle per iteration at loop-body residue 3
modulo 4, level 1 interrupt entry and resume at 227 and 143 cycles, and level
3 entry and resume at 222 and 139 cycles. This receipt adopts no new cost.
The ELF verification confirms five 256-operation issue blocks and loop-body
residues 0, 1, 2, and 3. Boot 1 is rejected because a stale partial pre-reset
record prefixes its ROM banner; its archive hash remains listed only as a
noncanonical diagnostic.

The product cohort contains 30 contiguous successful reset-to-ready samples.
Reset-to-ready is retained as a distribution only: minimum
2.788178125003469 seconds, median 2.7968129584987764 seconds, nearest-rank p90
2.8008790419989964 seconds, and maximum 2.8038100420089904 seconds. The
internally measured tear telemetry is diagnostic only. Its period is 16,784
to 16,820 microseconds with median 16,806 and nearest-rank p90 16,814; its
high time is 577 to 580 microseconds with median and nearest-rank p90 579.
All 30 samples report 76 edges and level 0.

Values are sorted ascending. For 30 samples, the median is the arithmetic
mean of ranks 15 and 16. The p90 uses nearest rank, the one-based value at
`ceil(0.90 * n)`, which is rank 27. Core metric arrays have odd length, so
their median is their middle sorted sample. Exact core derivations are stated
in `receipt.json` alongside the independently recomputed per-boot values.

The superseded product cohort, one-run diagnostic, diagnostic serial log,
and failed older 30-run attempt are noncanonical diagnostics. They are not
mixed into the accepted cohort. The large ELFs, firmware binaries, and
sdkconfig files remain in the source archive and are referenced only by
SHA-256.

Verify the committed bundle with:

```text
shasum -a 256 -c SHA256SUMS
gzip -t core-boot-2.log.gz core-boot-3.log.gz
jq -e . receipt.json core-elf-verification.json product-boot-cohort-validated.json
```
