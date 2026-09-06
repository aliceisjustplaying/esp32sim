# ROM fetch ladder v2: diagnosing the `clzdi2` variation

Status: diagnostic design under independent challenge (astra). Adopts no price.
Reviewed findings A to F from the first challenge are incorporated below.

## Question

In the v1 capture (`../esp32s3-rom-fetch-ladder/RESULT.md`), nine of ten matched
windowed-call bodies cost the same from mask ROM as from IRAM in every sample; the
ROM cell of the 20-byte `clzdi2` body was 16 cycles in 96 samples and 17 to 19 in
four, at the same sample positions in both boot logs, while its IRAM twin was 16
throughout. Why?

## What v2 changes, and what each change can and cannot show

Same ten ROM bodies, same build-time IRAM twins, same five-instruction timed region
(`rsr.ccount; movi.n; movi; callx8; rsr.ccount`, verified from the ELF).

1. **Boot identity.** Each boot prints `BOOT_IDENTITY counter=<RTC-noinit counter>
   random=<esp_random> rtc_us=<esp_timer> variant=<name>`. This identifies distinct
   captures and the running variant; it does not show that a disturbance is
   independent between boots. The analyzer requires distinct identities and the
   expected variant per archive.
2. **Variants of one source, one build directory each:**
   - `dual-idle`: core 1 runs only the IDF idle task and tick, as in v1, but the
     image layout differs from v1 (extra cells, records, statistics), so it is
     not an exact repeat. The archived v1 image (app ELF `476116ea...`) is rerun
     in the same session as the exact-layout, recording-off baseline. If neither
     reproduces the variation, the answer is "not reproduced" and nothing is
     attributed.
   - `unicore`: `CONFIG_FREERTOS_UNICORE=y`; core 1 is never started.
   - `dual-rom-hammer`: a core-1-pinned task loops calling the mask-ROM `abs`
     body (`0x4002e314`) for the whole run.
   - `dual-iram-hammer`: the same task, loop, priority and memory traffic, calling
     the byte-identical IRAM twin of `abs`. Only the call target differs. This is
     the matched control for the ROM hammer; without it, heavier traffic alone
     would not implicate the ROM.
   The hammer loop is `IRAM_ATTR`; the verifier checks that the task is entirely in
   IRAM and that its literals lie in IRAM or DRAM. Task creation is checked and
   progress is proved before cells run (`HAMMER_PROGRESS calls_per_100ms=N`; zero
   progress prints `CALIBRATION_FAILED`). The final call count is printed.
3. **Per-cell statistics.** `CELL_STATS name=... attempts=... accepted=...
   rejected_counters=... rejected_zero=...` after every cell, so cache-counter
   gating cannot silently censor affected samples. The analyzer reports them.
4. **Position diagnostic.** The `clzdi2` pair also runs first and last in the run
   (`rom/iram_clzdi2_first`, `rom/iram_clzdi2_last`; 24 cells). Position changes
   time since boot, cache, window and task phase together, so this is a
   diagnostic, not an isolation of any one factor.
5. **Sample start times.** `SAMPLE_STARTS` lines record the first `rsr.ccount`
   value of each accepted sample for the three `clzdi2` ROM cells and their twins
   (stored after the second read; the timed region is unchanged), so outlier
   spacing can be compared with periodic activity such as the 1 ms tick.
   Diagnostic only.
6. **Boundary hypothesis.** `ctzsi2` crosses the 64-byte boundary at `0x400560c0`
   inside its body and was constant; `clzdi2` ends one byte before `0x40056380`.
   These are different opcodes at different addresses, so this comparison can
   neither isolate nor dismiss a boundary effect. It is recorded as a diagnostic.

## Declared analysis rule (before capture)

`analyze_v2.py` requires, per archive, the expected variant, 24 cells, 100 samples
per cell and two boots with distinct identities, and does no fitting. It reports the
variation as consistent with shared-ROM contention from core 1 only if all of the
following hold (this does not establish that the original idle-run outliers had the
same mechanism): the `unicore` variant shows no ROM-cell variation; `dual-rom-hammer` shows
more ROM-cell variation than `dual-idle`; `dual-iram-hammer` does not show that
increase; IRAM twins are constant in every variant; and the `clzdi2` variation is not
confined to a single position in any boot. Missing variants, cells, statistics,
start times or identities are input problems and yield no verdict. Any other pattern is reported as
"not attributed" with the raw per-variant data. No price is adopted in any case.

## Build, verify, dry-run, capture

```sh
eim run "idf.py -C calibration/esp32s3-rom-fetch-ladder-v2 -B out/v2-dual build" v6.1
rm -rf out/v2-unicore && UNICORE=1 eim run "idf.py -C calibration/esp32s3-rom-fetch-ladder-v2 -B out/v2-unicore build" v6.1   # fresh dir: defaults only apply when no sdkconfig exists
ROM_HAMMER=1 eim run "idf.py -C calibration/esp32s3-rom-fetch-ladder-v2 -B out/v2-rom-hammer build" v6.1
IRAM_HAMMER=1 eim run "idf.py -C calibration/esp32s3-rom-fetch-ladder-v2 -B out/v2-iram-hammer build" v6.1
# verify each with --expect-variant <name>, dry-run each, then per variant:
uv run --with pyserial==3.5 python calibration/tools/capture.py --image calibration/esp32s3-rom-fetch-ladder-v2 --build out/<variant> --boots 2 --port <serial>
```

Capture resets through the RTC watchdog (see `../esp32s3-rom-fetch-ladder/RESULT.md`
for why). Start and finish notifications are sent for the board session; the board
is used exclusively during it.
