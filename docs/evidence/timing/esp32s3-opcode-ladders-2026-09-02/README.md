# ESP32-S3 opcode ladder adoption

Two clean ESP-IDF v6.1 boots completed all 88 cells with 100 samples per
cell and zero refusals. `receipt.json` pins the image, source, toolchain,
board, raw captures, and exact `ChipConfig` scope.

The standard 256-operation ladder has 15 cycles of entry, exit, and loop
overhead: the 271-cycle nop body minus 256 issued instructions. Subtracting
that matched overhead makes a 3.058594 raw branch observation exactly 3
cycles per operation. Specialized verified bodies and their overheads are
listed in `summary.json`.

All conditional branches are exact at 3 cycles taken and 1 not taken. `j`
is exact at 3, `jx` at 6, loop setup at 5, `quos` and `quou` at 4, `rems`
and `remu` at 5, and `s32c1i` at 6. `l32r` is interval 1 to 2 and `isync`
is interval 6 to 7. The distance-1 load-use delay is exact at +1 cycle;
distance 2 is exact at zero.

Call and return pairs remain correlation targets under price-table rule R2.
The exact 256-pair totals after wrapper subtraction are 1,664 cycles for
`call0` or `callx0` plus `ret`, and 1,920 for `call8` or `callx8` plus
`retw`.

Verify with:

```text
python3 analyze.py
git diff --exit-code summary.json
shasum -a 256 -c SHA256SUMS
```
