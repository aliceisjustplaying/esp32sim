# ESP-IDF 6.1 PSRAM long-window candidates

This receipt assembles the four cold PSRAM long-window cohorts already
present in the committed ESP-IDF 6.1 rebaseline. No new hardware capture was
used. Each cohort has at least two complete independent boot receipts, so no
recapture is needed.

| Kernel | Eligible boots | Pooled samples | Min | p50 | p90 | p99 | Max |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| `psram_cold_sequential_single_core` | 3 | 300 | 1432180 | 1432180 | 1432180 | 1432251 | 1433597 |
| `psram_cold_sequential_core1_contended` | 4 | 400 | 2806948 | 2807281 | 2807300 | 2807648 | 2809162 |
| `psram_cold_random_single_core` | 4 | 400 | 2652302 | 2658665 | 2661461 | 2662784 | 2662958 |
| `psram_cold_random_core1_contended` | 4 | 400 | 5435990 | 5448089 | 5453885 | 5457303 | 5458557 |

The summaries pool the 100 cycle samples from every complete eligible boot.
Percentiles use nearest rank on the ascending pooled values. Boot 3's raw log
contains an incomplete sequential single-core record, so its absent recovered
receipt is excluded.

These are distribution candidates only. They are not adopted costs or
acceptance bounds. `receipt.json` records the source hashes, eligibility rules,
exact summary values, and disposition.

Verify this directory with:

```text
shasum -a 256 -c SHA256SUMS
```
