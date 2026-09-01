# ESP32-S3 core-timing calibration

This image is the reference calibration scaffold for CPU-side timing probes.
It emits 29 `CAL_RECORD` metrics and ends with `CALIBRATION_DONE`.

The cells cover:

- 15 call-window depths
- one empty-call baseline and five 256-op IRAM issue blocks
- four zero-overhead-loop body alignments
- level 1 and level 3 interrupt entry and resume

`probe-cells.json` is the validation contract. `verify_elf.py` checks all five
issue blocks byte for byte and checks the four loop-body residues from the
built ELF. Values from an emulator dry run are not hardware receipts.

The imported probe tried the level-1-only SW0 source before every interrupt
level. Under IDF 6.1, the level 3 attempt entered an invalid error-log path at
ROM PC `0x40057254`. This copy selects SW0 for level 1 and SW1 for level 3,
matching the fixed interrupt levels of those internal sources.

Build and verify with IDF 6.1:

```text
eim run "idf.py -C calibration/esp32s3-core-timing -B out/core-timing build" v6.1
eim run 'python3 calibration/esp32s3-core-timing/verify_elf.py out/core-timing/esp32s3_core_timing_calibration.elf out/core-timing/elf-verification.json --objdump "$(command -v xtensa-esp32s3-elf-objdump)"' v6.1
```

The offline gate boots the built image with the ROM and product board flags,
then checks the complete contract without treating emulator cycle values as
receipts:

```text
calibration/tools/dry-run.sh calibration/esp32s3-core-timing out/core-timing
```
