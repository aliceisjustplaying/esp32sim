# ESP32-S3 register-block adoption

Two clean ESP-IDF v6.1 boots completed all 69 cells with 100 samples per
cell and zero refusals. The capture covers register reads, same-value writes,
and 1, 2, 4, 8, 16, and 256-operation run ladders.

Read costs are exact at 9 cycles for SYSTEM, SENSITIVE, EXTMEM, and
ASSIST_DEBUG, 15 for the APB peripheral blocks, and 18 for NRX. RTC and
eFuse reads remain distributions: 80.203125 to 80.96484375 cycles for RTC
and 80.34375 to 80.82421875 for eFuse in these captures.

The posted-write buffer accepts eight writes with an exact `n + 1` total.
From the 16 through 256 run slopes, the steady drain is exact at 4 cycles
per write for the fast tier and 15 for APB. NRX is interval 17 to 18. RTC
is a distribution from 69.7265625 to 70.62890625 cycles per write. Block
membership and every cell distribution are recorded in `summary.json`.

All costs have the `ChipConfig` scope in `receipt.json`. Pricing must derive
that configuration from programmed registers and refuse unmatched values.

Verify with:

```text
python3 analyze.py
git diff --exit-code summary.json
shasum -a 256 -c SHA256SUMS
```
