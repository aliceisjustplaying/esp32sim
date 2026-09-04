# H1 exception-ladder hardware result

Two ESP-IDF 6.1 boots on the Waveshare ESP32-S3-Touch-AMOLED-1.8 v2
completed all seven H1 cells: 700 accepted samples per boot and zero
refusals. The receipt pins source commit `75778a4c`, the immutable ELF and
manifest, both raw logs, and the complete 22-file archive index.

Six cells are constant across both boots. `call4_window_pair` has a stable
352-cycle median and p90 in each boot, plus four larger samples per boot, so
that cell is a distribution and not a constant exact total.

The direct boundaries produce exact candidates of five cycles for `rfe` and
four for `rfi 3`, after subtracting the one-cycle leading `rsr.ccount`. The
syscall cell produces a seven-cycle exception-entry candidate. Subtracting the
two nine-cycle handler prefixes from the existing exact 35-cycle window-pair
target leaves 17 cycles for the four unknowns
`E_window_overflow8 + E_window_underflow8 + rfwo + rfwu`.

The seven-cycle syscall entry is a different typed class from both window
entries. Equating them would conditionally leave `rfwo + rfwu = 3`, but H1 and
the pinned sources do not establish that equality. Every completed recursion
also pairs one overflow with one underflow, so the `rfwo` and `rfwu` columns
are identical. The straight-line mask-ROM cell stays interval-tier because its
timed path includes the interval-priced `callx8; entry; retw.n` sequence and
has no matched IRAM control.

No price is adopted. R8(b) still requires an unused committed receipt to
validate the return and syscall-entry candidates through the measured engine.
The window returns and mask-ROM fetch are not independently identifiable.
All existing typed refusals therefore remain in place.

Reproduce with:

```text
ESP32S3_H1_ARCHIVE=/Users/sarah/Archives/esp32s3/hardware-batch-2026-09-04-20260904-102500 \
ESP32S3_H1_BUNDLE=/Users/sarah/Archives/esp32s3/pinned-builds/esp32sim-h1-75778a4c \
python3 analyze.py > ../../../../work/h1-summary.json
diff -u summary.json ../../../../work/h1-summary.json
```
