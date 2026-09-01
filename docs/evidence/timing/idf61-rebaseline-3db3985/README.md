# ESP-IDF 6.1 rebaseline

This directory preserves lane zero's 2026-08-31 ESP-IDF 6.1 hardware
rebaseline. TinyDraw commit `3db39856f0a04266a42aef8cd5ead1be6fc8eca4`
built the timing probe with ESP-IDF v6.1, xtensa-esp-elf 15.2.0, and the
same ESP32-S3 QFN56 revision v0.2 board used by the historical v6.0.2
receipts. The timing ELF is pinned at
`4d70b3bea29c88f1d56f026b09b72f6949e861a27e08016660dbc543d3e4c233`
and sdkconfig at
`20ba6a9133738c3ab131cf19679984580b3180f5f28704a7d164e5989e8fbe02`.

`captures/` contains the unmodified serial logs. `receipts/` contains one
deterministic recovered-receipt archive per timing boot. Their entry counts
are 199, 203, 199, and 201. All 802 recovered receipts pass the production
receipt parser and report `pass: true`.

The four-boot union contains 210 measurement identities. Exactly 204 have at
least two complete independent receipts. The strict exit criterion remains
incomplete for these six identities:

- zero: `mmio_read_rtc_xtal_freq_4096_aligned_core1_contended`
- zero: `reset_reason_core1_read_rtc_state_4096_core1_contended`
- one: `mmio_read_rtc_date_4096_aligned_core1_contended`
- one: `mmio_read_rtc_date_4096_aligned_single_core`
- one: `mmio_read_rtc_store1_4096_aligned_core1_contended`
- one: `reset_reason_core0_read_rtc_state_4096_core1_contended`

The repeated loss is a capture-transport blocker. The probe has compile-time
cohorts and no runtime selective rerun, so another full-suite boot was not
used after boot four. `toolchain-delta.json` records the adopted and retained
v6.0.2-versus-v6.1 comparisons. `SHA256SUMS` pins every artifact here.

Verify the bundle with:

```text
shasum -a 256 -c SHA256SUMS
gzip -t captures/*.gz receipts/*.tar.gz
for f in receipts/*.tar.gz; do tar -tzf "$f" | wc -l; done
```
