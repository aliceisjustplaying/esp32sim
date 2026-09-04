# TinyDraw frame correlation candidate

Two hardware boots at each PSRAM clock ran the same fixed 20-event TinyDraw
stroke and emitted 21 aligned frame records. The source commit, board, IDF,
CPU and flash clocks, workload hash, and invariant configuration hash match.
The 40 and 80 MHz clock selectors and readbacks differ as intended. Core-1
touch was stopped during each replay.

## Capture limits

These are not four clean boots. Both boot-1 raw logs begin with a stale,
partial post-flash serial line because the capture runner did not clear its
input buffer before reset. The raw bytes remain in the receipt. Analysis uses
only the accepted normalized frame window from the unique trace header through
the replay terminal.

The 80 MHz boot-2 log also records a recovered panel configuration failure:
the first configure attempt returned `ESP_ERR_INVALID_RESPONSE`, then one bus
reset succeeded. This happened before the accepted trace window, but it means
that boot did not enter the workload through the same clean initialization
path as the other three.

## Result

This is raw two-boot, distribution-tier candidate evidence. It provides paired
total-cycle distributions and shared cache-counter covariates. It does not
establish distribution agreement, a scalar PSRAM price, or a non-PSRAM
partition, so the frame-scale 1 percent claim is refused.

The 40-minus-80 MHz total-cycle distribution has a negative median in both
boot pairs: -9,840 and -12,096 cycles. This does not show that 40 MHz PSRAM is
faster. The total includes the presentation transfer wait, whose paired sign
matches the total-cycle sign on all 42 frames. The 40 MHz capture had a
shorter transfer wait on 33 frames and a longer wait on 9, which accounts for
the negative median and the mixed signs.

The wide positive tail is also phase-sensitive. Frame keys 3, 9, and 10
repeat above +200,000 cycles in both boots. Their transfer-wait deltas are
+467 to +486 microseconds, and their shared I-bus access deltas are +109,908
to +128,704. Boot 2 also has an isolated frame-19 outlier of +227,388 cycles
with a +931-microsecond transfer-wait delta. The same 40 MHz frame differs by
+235,878 cycles, or 53.989 percent, between its two boots. The other 20 differ
by -2,964 to -78 cycles.

The D-bus covariates show stable replay shape: PSRAM miss counts match on all
42 pairs, flash misses are zero, and access-count deltas are -2 to +2. The
I-bus counters differ, and all hardware cache counters are shared across both
cores. They describe concurrent activity but cannot attribute or subtract a
PSRAM cost. Presentation wait phase and shared activity therefore remain
confounders in the 40/80 delta.

## Receipt

The committed evidence includes the four raw serial logs, their normalized
records, both paired reports, both immutable build manifests, flash logs, and
the complete session contract and state. `receipt.json` records the original
raw hashes, build identities, capture-tool hashes, and archive index hash.

Recompute and verify with:

```text
python3 analyze.py
git diff --exit-code summary.json
shasum -a 256 -c SHA256SUMS
```
