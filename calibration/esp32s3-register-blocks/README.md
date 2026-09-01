# ESP32-S3 register-block calibration

This image measures one safe TinyDraw-touched register in each available MMIO
block, plus SRAM baselines and SYSTEM/GPIO run-length ladders. It emits 69
`CAL_RECORD` metrics with 100 samples each and ends with `CALIBRATION_DONE`.

The two RTC cells carry `clockDomain: "rtc"`. The manifest records every
FIFO, interrupt-clear, eFuse-programming, command, and other side-effecting
register excluded from consideration. INTERRUPT has no observed reads, EFUSE
has no observed writes, and all observed USB Serial/JTAG writes have side
effects.

`tinydraw-mmio-touch-list.json` records the instrumented TinyDraw run and ELF
hash. Its adjacent diff is the exact throwaway esp32sim instrumentation patch.
`verify_elf.py` checks all 12 read/write ladders byte for byte, including the
1, 2, 4, 8, 16, and 256 access lengths and 4-byte function alignment.

Build, verify, and boot offline with IDF 6.1:

```text
eim run "idf.py -C calibration/esp32s3-register-blocks -B out/register-blocks build" v6.1
eim run 'python3 calibration/esp32s3-register-blocks/verify_elf.py out/register-blocks/esp32s3_register_blocks_calibration.elf out/register-blocks/elf-verification.json --objdump "$(command -v xtensa-esp32s3-elf-objdump)"' v6.1
calibration/tools/dry-run.sh calibration/esp32s3-register-blocks out/register-blocks
```

The offline run checks shape and completeness only. Its cycle values are not
hardware receipts. The maintainer capture command is:

calibration/tools/capture.py --image calibration/esp32s3-register-blocks --build out/register-blocks --boots 2 --port <serial>
