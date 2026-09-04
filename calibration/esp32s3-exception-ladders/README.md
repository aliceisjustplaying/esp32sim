# ESP32-S3 exception ladders

This IDF 6.1 image captures 100 cache-clean samples for each of seven cells:
non-tail `call4`,
`call8`, and `call12` recursion past the register-file knee, a syscall and
level-1 return pair, `rfe` alone, and `rfi 3` alone. The syscall handler's
known terms are `rsr.epc1`, `addi`, `wsr.epc1`, `rsync`, and `rfe`.
The final cell invokes the five-byte `xtos_p_none` implementation directly at
`0x400559a4`. Its mask-ROM path is exactly `entry; retw.n`, and every accepted
sample requires zero instruction-cache and data-cache counter deltas.

Build, verify, and dry-run:

```sh
eim run "idf.py -C calibration/esp32s3-exception-ladders -B out/exception-ladders build" v6.1
eim run "python3 calibration/esp32s3-exception-ladders/verify_elf.py out/exception-ladders/esp32s3_exception_ladders_calibration.elf out/exception-ladders/elf-verification.json --objdump \$(command -v xtensa-esp32s3-elf-objdump)" v6.1
calibration/tools/dry-run.sh calibration/esp32s3-exception-ladders out/exception-ladders
```

The verifier resolves `esp32s3_rev0_rom.elf` through the IDF 6.1
`ESP_ROM_ELF_DIR`, pins its SHA-256, proves that the application alias is an
absolute mask-ROM address, and verifies the target's `.text` placement and
both instruction encodings. Its JSON pins the application ELF and binary,
bootloader, partition table, sdkconfig, generated flasher arguments, probe
manifest, mask ROM ELF, and the exact flash layout parsed from those generated
arguments. It does not derive or adopt a timing price.
Strict capture requires the manifest-pinned IDF, harness, target, revision,
core count, clocks, sample and attempt counts, and recursion depth. The dry-run
path alone accepts emulator-reported chip revision 0; hardware capture requires
the board contract's revision 2.

Capture two clean boots:

calibration/tools/capture.py --image calibration/esp32s3-exception-ladders --build out/exception-ladders --boots 2 --port <serial>
