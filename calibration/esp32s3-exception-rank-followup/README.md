# ESP32-S3 H2 exception rank follow-up

This IDF 6.1 image emits nine 100-sample cells. Two matched window controls
separate WindowOverflow8 and WindowUnderflow8 trigger-to-vector residuals,
and direct cells separate `rfwo` and `rfwu`. `DESIGN.md` defines the equations
and adoption gates.

```sh
eim run "idf.py -C calibration/esp32s3-exception-rank-followup -B out/h2-exception-rank build" v6.1
eim run "python3 calibration/esp32s3-exception-rank-followup/verify_elf.py out/h2-exception-rank/esp32s3_exception_rank_followup.elf out/h2-exception-rank/elf-verification.json" v6.1
calibration/tools/dry-run.sh calibration/esp32s3-exception-rank-followup out/h2-exception-rank
```

Only after all three commands pass, the shared capture runner may flash the
image for exactly two clean boots. The queue reservation is one flash plus 30
seconds of board time, with no product-restore stage. The shared runner clears
serial input while reset is held, archives the exact ELF verification, and
pins the full source commit in the receipt.

```sh
calibration/tools/capture.py --image calibration/esp32s3-exception-rank-followup \
  --build out/h2-exception-rank --boots 2 --port PORT --timeout-s 15
```

No candidate is adopted by this image or by its emulator dry-run. Adoption
requires a committed two-boot H2 receipt that passes the unused H1 validation
targets pinned in `design-proof.json`. Mask-ROM fetch remains refused because
the minimal safe-window predicate failed closed in the emulator.
