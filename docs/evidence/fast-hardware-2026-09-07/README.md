# Current board memory and transfer measurements

Two independent watchdog-reset boots of the connected Waveshare ESP32-S3 Touch
AMOLED 1.8 V2 reproduce the earlier SPI2 and writeback results. Each completed
43 cells and 360 samples with zero refusals. The existing TinyDraw NDJSON
validator accepted both captures: [boot 1 validation](validation-1.json),
[boot 2 validation](validation-2.json).

| Measured operation | Result in CPU cycles | Interpretation |
| --- | --- | --- |
| First cold PSRAM data line | 93 or 96 by boot | Includes the probe traversal overhead; not a pure incremental miss penalty |
| First cold flash data line | 128 | Same qualification |
| Dirty minus clean writeback, 1/2/4/8/16 lines | 154/308/630/1260/2506 | Roughly 154–158 additional cycles per 64-byte dirty line in these explicit flush probes |
| SPI2 4096-byte transfer at 40 MHz | submission 5601, completion 199218 | Single-bit SPI, completion window includes software waiting |
| SPI2 32768-byte transfer at 40 MHz | submission 5826, completion 1575474 | Large-transfer completion fits 48 cycles/byte + 2610 exactly for these two sizes |
| SPI2 32768-byte transfer at 20 MHz | submission 5825, completion 3148353 | Roughly 96 cycles/byte + 2625 |
| Quad panel call, 1024/4096/16384 bytes | 239801 each | Whole blocking call, approximately one millisecond; do not use as wire time |
| Quad panel call, 32768 bytes | 479801 | Approximately two milliseconds |

Values are combined medians unless a boot range is stated. Full per-cell ranges
and payload groups are in [current summary](current-summary.json), with raw
[boot 1](boot-all-1.log) and [boot 2](boot-all-2.log). PSRAM traversal varies
between boots by roughly 1.8%; the fixed SPI2 phase medians and writeback deltas
match the earlier [archive summary](archive-summary.json). These are useful
model inputs and checks, not proof of whole-system accuracy within 1%.

The image uses CPU 240 MHz, PSRAM 80 MHz, ESP-IDF v6.1 and silicon revision 0.2.
It is the archived normal image from TinyDraw commit
`7a157d44a9da3312b1ecda2b45b116af2de28e63`, whose ELF SHA256 was checked against
its archived verification record:
`861f6c69f8c348cd5f0f3664ae5e2b73a24b2b87d7a932b0f67036f491f8f949`.
The runtime metadata in each raw capture records the same identity.

Artifact source on this machine:
`/Users/alice/Archives/esp32s3/2026-09-01-tier-b-decomposition/canonical-7a157d4/builds/normal`.
Earlier captures used for comparison are its sibling `normal/boot-1/serial.log`
and `normal/boot-2/serial.log`. No rebuild or TinyDraw checkout edits were needed.

The original 16 MiB flash was read completely before flashing the probe and is
retained outside this committed evidence in `work/hardware-private`, SHA256
`d51939aa80a01a91b24b106a7d4a6a7be9e91643f35d2ec661156e6c66edfc90`.
The probe write changed only bootloader, partition-table and app image ranges.

To repeat, use the small [capture script](../../../tools/fast-hardware/capture.py)
through `uv` with an isolated virtual environment containing `esptool` and
`pyserial`. Pass `--cells all`: the long selective command was rejected by the
existing firmware, while the short command completed promptly. The script uses
watchdog reset and opens serial with DTR/RTS deasserted because control-line
reset puts this board in download mode.

The [summary script](../../../tools/fast-hardware/summarize.py) accepts one or
more raw capture paths. It groups sweep results by payload as well as cell so
different transfer sizes are not mistaken for repeated measurements.

The [cache estimates](cache-estimates.json) separate observed quantities from
unidentified model parameters. A cold 64-line traversal costs 166.73–169.69
cycles per line at 80 MHz including its loop, while the first single-line
probe costs 93–96 cycles. Explicit dirty-minus-clean flush costs 154–161.33
cycles per line; automatic eviction cost is not separately measured. These
data support experimenting with service/overlap behavior, not claiming a
universal extra fill latency of 160 cycles.

Geometry clarification: both firmware configurations request eight data-cache
ways, but the supplied ROM [wrapper](rom-cache-wrapper.txt) passes that choice
to [Cache_Set_DCache_Mode](rom-cache-set-mode.txt), which does not read its ways
argument (`a3`). It changes size and line-size bits only. This agrees with the
ROM header's fixed-four-way statement. Keep four ways as the simple-model
default; neither associativity nor replacement policy was measured by these
captures. The supplied ROM is revision 0, so this is not an independent readback
of revision 0.2 ROM.
