# ESP32-S3 opcode ladders

This image measures 88 LX7 opcode cells as 256-operation IRAM issue
blocks. Each cell records 100 CCOUNT samples and reports cycles per
operation as minimum, median, p90, and maximum values. Hardware cache
counters must remain zero.

The conditional-branch cells cover the full LX7 set plus `bltz` and
`bgez`, which were observed in the TinyDraw dynamic histogram. Every
conditional branch has matched taken and not-taken cells with aligned
targets. The image also covers direct and indirect jumps, call and return
pairs, loop setup, the requested nonbranch opcodes, and two load-use
distances.

`tinydraw-opcode-histogram.json` records the ROM-to-first-autosave run,
including the TinyDraw ELF SHA-256, instruction counts, and shares. The
adjacent `.diff` is the exact zero-context instrumentation patch used in
the throwaway esp32sim worktree.

Build and verify with IDF 6.1:

```sh
eim run "idf.py -C calibration/esp32s3-opcode-ladders -B out/opcode-ladders build" v6.1
eim run "python3 calibration/esp32s3-opcode-ladders/verify_elf.py out/opcode-ladders/esp32s3_opcode_ladders_calibration.elf out/opcode-ladders/elf-verification.json" v6.1
calibration/tools/dry-run.sh calibration/esp32s3-opcode-ladders out/opcode-ladders
```

Capture two clean hardware boots with:

calibration/tools/capture.py --image calibration/esp32s3-opcode-ladders --build out/opcode-ladders --boots 2 --port <serial>
