# H1 is blocked by syscall EPC1 semantics

The required `syscall` cell cannot collect 100 samples with a handler whose
only instruction is `rfe`. An LX7 syscall exception records the syscall's own
address in EPC1. A bare `rfe` therefore returns to the same syscall and repeats
the exception without reaching the following CCOUNT read.

ESP-IDF v6.1 confirms the required architectural adjustment in
`components/xtensa/xtensa_vectors.S` lines 907 through 928. Its syscall handler
reads EPC1, adds the three-byte syscall width, writes EPC1, and then executes
`rfe`.

The IDF 6.1 image with a private level-1 vector containing only encoding
`003000` (`rfe`) reproduced the loop in esp32sim. The first three call cells
completed, `syscall_rfe_pair` emitted no record, and the bounded run stopped at
20,000,000 instructions with 52,094 exceptions and no `CALIBRATION_DONE`.

Command:

```sh
calibration/tools/dry-run.sh calibration/esp32s3-exception-ladders out/exception-ladders
```

Adding the required EPC1 increment would measure exception entry, three handler
instructions, and `rfe`. That is a different cell from H1's requested bare
handler, so the failing verifier contract remains the exit test and the image
is parked. Nothing was flashed.
