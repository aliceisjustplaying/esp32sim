> ARCHIVE MATERIAL. Extracted from the puck-era checkout (see
> [PROVENANCE](../../PROVENANCE.md)). Evidence and reference only: any build or run
> commands in this document executed in that repository, not this one.

# ESP32-S3 timing evidence

The `receipts/` directory and `esp32s3-rev02-tinydraw-d81e2ea-candidate.json`
are the original counter-free cohort.

[`idf61-rebaseline-3db3985/`](idf61-rebaseline-3db3985/README.md) preserves
lane zero's ESP-IDF 6.1 flag-day captures, recovered receipts, strict coverage
ledger, fixture and toolchain pins, and the retained v6.0.2-versus-v6.1 delta.
Its strict two-independent-receipt criterion is incomplete for six explicitly
listed RTC/reset groups because of repeated USB capture truncation.

`receipts-bf169bc/` contains 118 receipts from three boots of TinyDraw commit
`bf169bc0e2e3f7cb3cff9b8bbda2861ec436be7d`. That firmware records ESP32-S3
instruction and data cache counters, configures the DBUS flash classifier to
the exact data-mapped flash interval, and adds bounded SRAM assembly probes.
Boots 1 and 2 each lost one USB Serial/JTAG fragment. TinyDraw's opt-in recovery
mode omitted the affected measurement and ran every retained group through the
strict receipt assembler. Boot 3 passed the strict whole-log collector. Every
measurement identity therefore retains at least two independent complete boots.

The original raw logs are stored in `captures-bf169bc/` with deterministic gzip
headers. Their decompressed SHA-256 values are the `bootLogSha256` values in the
receipts. `esp32s3-rev02-tinydraw-bf169bc-counters-candidate.json` is the stable
118-receipt report. It retains exact cache-counter moments and remains an
unreviewed candidate. It does not adopt cycle costs or claim cache or ISA
calibration.

The corrected counters give useful bring-up values without making them adopted
costs. Long sequential D-cache fills average about 415 CPU cycles per flash
line and 111 cycles per PSRAM line. Random cold samples suggest isolated-line
candidates near 465 and 162 cycles, respectively. These map naturally to the
model's subsequent-line and first-line burst fields, but interrupt traffic and
the random working-set shape still confound a calibrated fit. Two different
two-line instruction probes measured 424 and 501 excess cycles, so instruction
flash remains context-dependent and instruction PSRAM is unmeasured.

The SRAM assembly probes measure one instruction per cycle steady-state issue,
8 independent loads or stores per 9-cycle body, and a 3-cycle dependent
`addx4` plus `l32i` recurrence. The profile adopts a one-cycle steady-state
instruction issue cost and zero additive SRAM load or store cycles at
independent throughput. The recurrence has one additional dependent load-use
cycle beyond its two issued instructions. That hazard remains an explicit
unmodeled claim-boundary value because runtime traces do not identify register
dependencies. These values are not per-opcode latency claims.

`captures-1ddd64b/` and `receipts-1ddd64b/` retain two boots of exact matched
hot-hit probes. The 120-instruction IRAM and flash bodies both take 126 cycles;
the flash body records 62 instruction-cache accesses and zero misses. The
16-load SRAM, PSRAM, and flash bodies all take 33 cycles; the external bodies
record 16 data-cache accesses and zero misses. Every retained receipt has 100
samples with one exact cycle and counter signature.

`captures-4a2c659/` and `receipts-4a2c659/` retain two further boots of the
matched dependent-load probes. SRAM, hot PSRAM, and hot flash each take exactly
16,403 cycles for all 100 samples in both boots. The external paths each record
4,096 data-cache accesses and zero misses. This shows zero additive external
hot-hit cost even when every next address depends on the prior load. It does
not remove the common one-cycle dependency hazard: that hazard remains
unmodeled because runtime traces do not carry register-dependency information.

`esp32s3-rev02-tinydraw-1ddd64b-4a2c659-hot-hit-adoption.json` pins both
firmware commits, all four boot IDs, compressed and decompressed capture
hashes, strict receipt hashes, exact distributions, cache-counter signatures,
and the adopted zero additive instruction-fetch and load hit costs. Store-hit,
cold-miss, writeback, and per-opcode latency costs are outside this adoption.
Each raw log contains an isolated malformed USB fragment in an unrelated
measurement; complete-measurement recovery omits those groups and runs every
retained target group through the unchanged strict receipt assembler.

`captures-a91d1d7/` contains three raw boot logs from TinyDraw commit
`a91d1d74af0ff4c1b55aebc3ed584e9074821394`, built with ESP-IDF 6.0.2 for an
ESP32-S3 revision 0.2 at 240 MHz, QIO flash at 80 MHz, and octal DTR PSRAM at
80 MHz. The logs have deterministic gzip headers. Their decompressed SHA-256
values are, in boot order, `396a188e4eea3663cc3c90e6e647264ce9684cb68c4c51a39591022ddf8f5c74`,
`f7f23872490223437313b00a12b9833802ae51481fab4505389092db6ad0fb33`, and
`6cbf1b1c8380bbf57850c272be86a6f47a303eab8668939329c56729b9796ecd`.
Recovery retained 93, 94, and 96 of 96 measurement groups. Every newly added
cache-burst cell is complete in boots 2 and 3. `receipts-a91d1d7/` keeps only
the instruction and data cache burst receipts used by the analyses.

The instruction-cache flash ladder measured cold-minus-hot penalties of 204,
469, 1002, and 2065 cycles for 1, 2, 4, and 8 cache lines. The adopted burst
cost is 204 cycles for the first line and 266 cycles for each subsequent line.
The rounded least-squares tail has a maximum ladder residual of one cycle.

The data-cache flash ladder measured penalties of 115, 588, 1532, 3425, and
7209 cycles for 1, 2, 4, 8, and 16 lines. The adopted cost is 115 cycles for
the first line and 473 cycles for each subsequent line, with a maximum rounded
fit residual of two cycles. The PSRAM ladder measured 82, 252, 590, 1271, and
2631 cycles and adopts 82 plus 170 cycles, also with a maximum residual of two
cycles. These burst ladders supersede the earlier long-sequential and random
estimates. Instruction PSRAM, cache-store hits, writeback latency, and the
dependent SRAM load-use hazard remain uncalibrated or unmodeled.

`esp32s3-rev02-tinydraw-a91d1d7-cache-burst-adoption.json` records the adopted
values and claim boundaries. The adjacent instruction-cache and data-cache
analysis files contain the complete fitted ladders and source receipt hashes.

`captures-2bf3ffd/` and `receipts-2bf3ffd/` retain two clean boots of matched
conditional-branch probes from TinyDraw commit `2bf3ffd861115b95451df1860623618c06e22dcf`.
The baseline and not-taken `beqz` paths each measured exactly 28,692 cycles in
all 100 samples of both boots. The taken path measured exactly 36,884 cycles.
Each path executes 4,096 matched body iterations, so the 8,192-cycle delta is
two additional cycles per taken branch. All five cache-counter deltas are zero.

`esp32s3-rev02-tinydraw-2bf3ffd-beqz-adoption.json` pins the firmware, ELF,
sdkconfig, raw capture, receipt, and boot identities. It adopts a one-cycle
not-taken cost and three-cycle taken cost only for the exact measured `beqz`
encoding and observed route. Other conditional branches, interrupts, and
whole-core timing remain outside the claim.

Regenerate the report with:

```sh
bun run packs/esp32-s3-touch-amoled-18/timing/calibration-report.ts \
  --output candidate.json \
  receipts-bf169bc/boot-1 receipts-bf169bc/boot-2 receipts-bf169bc/boot-3
```
