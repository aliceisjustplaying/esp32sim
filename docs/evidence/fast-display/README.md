# TinyDraw with delayed SPI2 completion and measured TE

Initial result: TinyDraw boots and reaches `TINYDRAW_GATE1_TILE_PUBLISH pass=1`
with both experiments enabled. The five-second simulated run records startup
`transfer_wait_us=18161` and `tear_edge_timeout=0`.
See [the native boot log](tinydraw-timed-boot.log).
This is a functional smoke, not a completed drawing battery or a speed comparison.

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
The initial boot receipt precedes the final change making `CMD.USR` read busy while
a transfer is pending; the separate unit test covers that change.

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
