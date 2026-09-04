# TinyDraw frame correlation

`tools/frame_correlation.py` consumes newline-delimited JSON from hardware and
esp32sim. Records may be bare JSON or follow `FRAME_CORRELATION_V1`,
`TINYDRAW_TRACE_V1`, `TINYDRAW_FRAME_V1`, or `ESP32SIM_FRAME_V1`. Other serial
lines are ignored.

Normalize a raw USB serial capture against its immutable build manifest:

```text
python3 tools/frame_correlation.py normalize MANIFEST.json CONSOLE.log --source hardware > CAPTURE.ndjson
```

The normalizer verifies the manifest, runtime clock readback, complete fixed
replay, bundle identity, event count, and frame sequence. It derives the replay
and invariant-configuration hashes, then emits the strict records consumed by
the comparison commands. Use `--source emulator` for a dry-run receipt.

The normalized `metadata` record pins the artifact manifest, TinyDraw
commit, replay hash, invariant configuration hash, clock-specific receipt,
board, IDF version, PSRAM clock, and whether core-1 touch is enabled. Firmware cannot embed the hash of a
manifest that hashes the firmware itself, so verified capture tooling adds
the manifest and configuration-receipt hashes.

Each `frame` identifies `seq`, `kind`, and `event_seq`; carries total and
nullable non-PSRAM and PSRAM cycles; names unknown components; and records the
five hardware cache counters. `seq` must be contiguous. `run-complete` closes
the input with the observed frame count. The complete field contract is shown
by `tests/correlation/frame-v1/`.

Compare a genuinely partitioned hardware run and emulator ledger:

```text
python3 tools/frame_correlation.py compare HARDWARE.ndjson EMULATOR.ndjson
```

The command requires exact identity and frame alignment. It
checks each non-PSRAM frame against the inclusive 1 percent target using
integer arithmetic. PSRAM observations are summarized as distributions and
never reported as a scalar error or price.

Summarize the planned paired deterministic hardware replay:

```text
python3 tools/frame_correlation.py psram-candidate HARDWARE-40.ndjson HARDWARE-80.ndjson
```

This requires identical replay, distinct artifact and
configuration receipts, and core-1 touch stopped. It reports both total-cycle
distributions, their paired delta, and each run's cache-counter signature,
distribution, and paired delta. It
leaves the non-PSRAM partition empty and refuses the 1 percent claim. Shared
cache counters are covariates, not a PSRAM price. Emulator input is labeled as
a dry run and cannot become hardware evidence.

Exit status is 0 for a passing comparison or valid candidate, 1 for a missed
1 percent target, and 2 for a malformed, incomplete, unknown, or mismatched
input. Run the two acceptance behaviors with:

```text
python3 tools/test_frame_correlation.py
```
