# ESP32-S3 DMA-on-SRAM calibration

This image measures the CPU copy that fills a 32,768-byte internal DMA slot
while SPI2 drains a second slot. It emits five 100-sample `CAL_RECORD` metrics
and ends with `CALIBRATION_DONE`. Emulator values are dry-run evidence only.

The two active-copy cells record a boolean for every sample and refuse the
cell if the SPI2 completion callback ran before the CPU copy ended. SPI2 uses
quad mode at 40 MHz with no chip select, so the panel is never addressed.

`probe-cells.json` is the capture contract. `verify_elf.py` checks the exact
IRAM copy-loop encodings, its four-byte alignment, its 8,192 iterations, and
the CCOUNT boundaries. `validate_capture.py` validates one captured log and
prints min, median, nearest-rank p90, and max for every cell plus paired
`b - a` and `c - a` distributions. It fails on missing or refused inputs.

Build, verify, and dry-run with IDF 6.1:

```text
eim run "idf.py -C calibration/esp32s3-dma-sram -B out/dma-sram build" v6.1
eim run 'python3 calibration/esp32s3-dma-sram/verify_elf.py out/dma-sram/esp32s3_dma_sram_calibration.elf out/dma-sram/elf-verification.json --objdump "$(command -v xtensa-esp32s3-elf-objdump)"' v6.1
calibration/tools/dry-run.sh calibration/esp32s3-dma-sram out/dma-sram
python3 -m pytest calibration/esp32s3-dma-sram
```

After capture, analyze either boot log with:

```text
python3 calibration/esp32s3-dma-sram/validate_capture.py ~/Archives/esp32s3/<session>/boot-1.log
```

calibration/tools/capture.py --image calibration/esp32s3-dma-sram --build out/dma-sram --boots 2 --port <serial>
