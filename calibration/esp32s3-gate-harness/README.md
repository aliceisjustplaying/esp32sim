# ESP32-S3 TinyDraw gate harness

This contract describes one complete replay from TinyDraw main commit
`7a157d44a9da3312b1ecda2b45b116af2de28e63`. It requires five
`TINYDRAW_LIVE_PRESENT` lines, one `TINYDRAW_LIVE_STRESS` line, and the
`TINYDRAW_GATE1_AUTOMATED_DONE` terminal line. The listed microsecond fields
are parsed as unsigned counters. Emulator fast-mode values have no timing
meaning.

From TinyDraw's `esp32` directory, build only this image under IDF 6.1:

```sh
eim run "idf.py -B out/build/esp32-vector-v2-gate-harness -DTINYDRAW_FIRMWARE_VARIANT=gate -DTINYDRAW_VECTOR_V2_TILE_SLOTS=604 build" v6.1
```

The resulting ELF SHA-256 is
`1d67c35762fe58b72202a19b1c06912f0b9503a7331ba881cda3928648b54cd6`.
Its sdkconfig SHA-256 is
`7490046d6e8b00d80f2bb550439821fa9d4a50da762e6e46d2aa9bdf8d520b8b`
and contains `CONFIG_SPIRAM_MODE_OCT=y` and
`CONFIG_ESP_MAIN_TASK_STACK_SIZE=20480`.

Verify and dry-run from the esp32sim tree:

```sh
python3 calibration/esp32s3-gate-harness/verify_elf.py <out>/tinydraw_esp32.elf <out>/elf-verification.json
calibration/tools/dry-run.sh calibration/esp32s3-gate-harness <out>
```

The emulator dry-run is blocked and exits 2. At 400 million instructions it
reports both `live_present` and `live_stress` missing. A longer run stopped at
50 billion instructions, 208.3 emulated seconds, with zero `TINYDRAW_LIVE_*`
lines. Core 0 remains in `spi_device_polling_end` because the small panel-init
SPI2 DMA transfer does not complete. No fast-mode reference was committed
because the emulator produced no counters.

The 2026-09-03 post-fix rerun was refused before emulator execution because
the hash-pinned build bytes were absent from the canonical local archive. The
similarly named archived build has different source and artifact hashes. The
post-fix marker and counter result remains unknown. Receipt:
[`../../docs/evidence/gate-harness-fast-rerun-2026-09-03/`](../../docs/evidence/gate-harness-fast-rerun-2026-09-03/README.md).

calibration/tools/capture.py --image calibration/esp32s3-gate-harness --build <out> --boots 3 --port <serial> --timeout-s 180
