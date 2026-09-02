# IDF 6.1 contention candidates

This analysis compares every one of the 103 rebaseline identities that has
both a `_single_core` and `_core1_contended` hardware receipt. It computes the
raw CCOUNT sample median for each identity and boot, then subtracts the
single-core median from the same-boot contended median.

An exact candidate requires at least two equal integer same-boot deltas. A
one-cycle range is interval tier. Every wider range, fractional result, or
identity without two same-boot pairs is distribution tier. The two identities
whose variants survived only in different boot archives use every independent
cross-boot difference and remain distributions. `summary.json` records min,
median, nearest-rank p90, and max for every distribution.

| Family | Family classification | What the candidates show |
| --- | --- | --- |
| Branch | exact | Zero contention across all three identities |
| Cache burst | exact | A repeatable constant for every identity, varying by burst shape |
| Cache hit | distribution | Zero or constant hits, one interval, and three spreading flash-map workloads |
| Dependent load | interval | Zero for SRAM and 317 to 318 cycles for hot flash and PSRAM |
| MMIO read | distribution | Zero outside RTC; RTC cells spread |
| MMIO write | exact | Zero contention across all ten identities |
| PSRAM pattern | distribution | One constant sequential-hot cell; the other patterns spread |
| ROM routine | distribution | Most are constant; reset-reason routines include an interval and a spread |
| Oracle | distribution | SRAM controls are zero, RGB565 is constant, and RTC reset-state reads spread |

These are milestone 5 candidates only. No value here is adopted or priced.

Reproduce the committed output byte for byte:

```text
python3 analyze.py
git diff --exit-code summary.json
```
