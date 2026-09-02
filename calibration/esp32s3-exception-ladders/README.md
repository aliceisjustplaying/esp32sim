# ESP32-S3 exception ladders

This IDF 6.1 image captures 100 cache-clean samples for non-tail `call4`,
`call8`, and `call12` recursion past the register-file knee, a syscall and
level-1 return pair, `rfe` alone, and `rfi 3` alone. The syscall handler's
known terms are `rsr.epc1`, `addi`, `wsr.epc1`, `rsync`, and `rfe`.

Build, verify, and dry-run:

```sh
eim run "idf.py -C calibration/esp32s3-exception-ladders -B out/exception-ladders build" v6.1
eim run "python3 calibration/esp32s3-exception-ladders/verify_elf.py out/exception-ladders/esp32s3_exception_ladders_calibration.elf out/exception-ladders/elf-verification.json" v6.1
calibration/tools/dry-run.sh calibration/esp32s3-exception-ladders out/exception-ladders
```

Capture two clean boots:

calibration/tools/capture.py --image calibration/esp32s3-exception-ladders --build out/exception-ladders --boots 2 --port <serial>
