# TinyDraw with delayed SPI2 completion and measured TE

The complete frozen TinyDraw battery passes all 36 final gates with both
experiments enabled, ending with `ssaa_receipt=yellow`.
See [the full native run](tinydraw-timed-full.json) and [raw log](tinydraw-timed-full.log).
The run continued to 75 simulated seconds, including an idle tail, in 72.8 seconds
wall time on a busy host. This is a functional result, not a browser speed comparison.

Across 15 matched paced-cold records, median presentation time is 0.601 times
the hardware timer value; the preceding baseline comparison was 0.284.
Median compute time remains 0.726 times hardware.
For `overlap zoom=50`, presentation changes from the preceding baseline's 20,048 us
to 40,313 us, versus hardware's 73,917 us.
Ring PIE staging remains 25,428 us versus hardware's 165,815 us: this experiment
does not address that memory/CPU mismatch.
See [the paired fields](hardware-comparison.json). The hardware boot restored
drawing state while this simulator run started with erased data partitions, so
matching selected workload fields does not establish complete state equality.

Two independent switches:

- CLI `--spi2-timing`: defer DMA descriptor owner writeback, GDMA completion,
  panel delivery and SPI `TRANS_DONE` until the register-derived SDR wire deadline.
- CLI `--measured-te`: use a fixed 16,773 us TE period with 578 us high,
  from TinyDraw's `docs/receipts/hardware/CO5300_PANEL_LIMITS_2026-08-15.md`.

The browser equivalents are preboot `esp32sim_set_spi2_timing(e, 1)` and
`esp32sim_set_measured_te(e, 1)`. Both return 0 on success or 1 for an unsupported
chip/board or an already booted emulator. Passing 0 selects baseline behavior.
Both experiments default off.

Wire timing uses SPI command/address/data lane flags, phase lengths and the clock
divider, with an assumed 80 MHz source clock. DMA payload is snapshotted at
submission. There is no progressive memory access, contention, DMA setup cost,
CS setup/hold cost or optical scanout model. CPU-fed SPI transfers remain immediate.
`CMD.USR` reads busy while a transfer is pending.

[Tests](tests.log): 49 esp32s3 tests and 14 esp-periph tests pass, including delayed
owner/interrupt/pixel delivery, waveform edge timestamps and SPI wire-clock decoding.
[WASM build](wasm-build.log) succeeds using:

```sh
DYLD_FALLBACK_LIBRARY_PATH=/Users/alice/.rustup/toolchains/1.98.0-aarch64-apple-darwin/lib \
PATH=/Users/alice/.rustup/toolchains/1.98.0-aarch64-apple-darwin/bin:$PATH \
cargo build -p esp32sim-wasm --release --target wasm32-unknown-unknown
```

Firmware inputs: `/Users/alice/src/a/esp32sim/work/perf-pie-confirm/candidate/assets.json`.
CLI smoke used its ROM, bootloader, partition table, application and ELF with
`--board waveshare-amoled18-v2 --flash-mb 16 --psram-mb 8 --boot rom
--spi2-timing --measured-te --max-seconds 5 --no-dump`.

Reproduce the full run with `node docs/evidence/fast-display/run-native.mjs`, then
`node docs/evidence/fast-display/compare-hardware.mjs`. The latter reads the
hardware agent's `summary-1.json` in its sibling `fast-hardware` checkout.
