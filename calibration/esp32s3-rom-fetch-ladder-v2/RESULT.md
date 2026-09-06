# ROM fetch ladder v2: diagnostic session result, 2026-09-06

Status: evidence only. No price adopted. Declared rule (README) applied by
`analyze_v2.py` with zero input problems. Verdict: **not attributed: the declared
pattern was not observed**.

## Session

18:13:31 to 18:15:22 UTC, one start and one finish notification, board exclusive.
Five images, two boots each, every boot complete with zero refusals. The eight v2
boots carry boot identities (all distinct) and per-cell statistics (zero rejected
samples in every cell). The two v1 baseline boots have neither; for them only
completeness and zero refusals are known.

| image | app ELF | archive |
|---|---|---|
| v1 baseline (see integrity note below) | archived `476116ea...`, flashed `b622cde9...` | `~/Archives/esp32s3/esp32s3-rom-fetch-ladder-20260906-191331` |
| v2 dual-idle | `4e73525f...` | `~/Archives/esp32s3/esp32s3-rom-fetch-ladder-v2-20260906-191352` |
| v2 unicore (generated config: 1 core) | `7234274e...` | `...-v2-20260906-191414` |
| v2 dual-rom-hammer | `bb6275ee...` | `...-v2-20260906-191436` |
| v2 dual-iram-hammer | `9a0ba3c4...` | `...-v2-20260906-191459` |

Baseline image integrity (found after the session, from a hash mismatch astra
raised): the capture tool archived the verified v1 artifacts and then ran
`idf.py flash`, which rebuilt the application because a source comment had been
edited, so the board ran the rebuilt binary (ELF `b622cde9...`, bin
`9dd0fd9f...`), not the archived files (`476116ea...`, `362d2e2d...`).
Disassemblies of the two ELFs are identical apart from the file-name header; the
two images differ in 65 bytes, all in the app descriptor's embedded ELF hash and
the trailing image checksum. So the baseline ran the same code as the first
capture, but the archived hashes do not name the exact file flashed. The flashed
files, the flash log and a note are added beside that archive
(`flashed-*`, `SHA256SUMS.flashed`, `FLASHED-BINARY-NOTE.md`). The four v2
sessions did not rebuild (their archives match the flashed builds). The tool now
flashes the archived artifacts with esptool directly.

Capture preamble anomaly (dual-iram-hammer, boot 1, `...-191459/boot-1.log`): line 1
is a truncated `BOOT_IDENTITY counter=3121440697 ... rtc_us=100098` from the
post-flash boot, joined to the single ROM reset banner of the capture reset; the
complete identity `counter=3121440698` follows at line 11 and all 24 cells and
their statistics come after it. No measured cell crosses the boundary; the
capture validator did not flag the preamble and the analyzer kept only the
complete identity. It is recorded here rather than treated as a clean single-boot
file. The other nine logs each contain exactly one reset banner.

Hammer proof: ROM hammer 823,793 calls per 100 ms before cells, 17.34 M calls total
per boot; IRAM hammer 823,498 and 823,642 per 100 ms, 17.33 M total. Both tasks are
18 identical instructions at identical addresses, differing only in the target.

## Results

Cycles per timed call; a single number means all 100 samples identical in both
boots; a range means the samples varied (both boots agree unless shown as a/b).

| body | v2 dual-idle rom / iram | v2 unicore rom / iram | v2 rom-hammer rom / iram | v2 iram-hammer rom / iram |
|---|---|---|---|---|
| p_none | 10 / 10 | 10 / 10 | 11-14 / 10-16 | 11-14 (11-13) / 10-16 |
| abs | 11 / 11 | 11 / 11 | 12-14 / 15-17 | 12-15 / 15-18 |
| mulsi3 | 11 / 11 | 11 / 11 | 12-14 / 15-17 | 12-15 / 15-18 |
| clzsi2 | 11 / 11 | 11 / 11 | 12-14 / 15-17 | 12-15 / 15-18 |
| temp_to_power | 12 / 12 | 12 / 12 | 13-15 / 12-17 | 14-16 / 13-19 (12-19) |
| roundup2 | 14 / 14 | 14 / 14 | 16-18 / 18-21 | 17-18 / 18-21 |
| ctzsi2 | 15 / 15 | 15 / 15 | 18-19 / 20-22 | 16-19 / 21-24 |
| ffssi2 | 15 / 15 | 15 / 15 | 18-19 / 19-23 | 16-19 / 21-24 |
| clzdi2 (first, middle, last) | 16 / 16 at all three positions | 16 / 16 at all three | 18-20 / 18-20 | 19-20 / 19-20 (iram first, boot 1: 18 once) |
| ctzdi2 | 19 / 19 | 19 / 19 | 19-22 / 19-22 | 21-23 / 21-23 |

v1 baseline, same session: all nine other pairs equal (ROM = IRAM, every sample);
`rom_clzdi2` varied again, boot 1 {16: 94, 17: 3, 18: 1, 20: 2} at sample indices
39, 40, 41, 48, 50, 54; boot 2 {16: 96, 17: 2, 18: 1, 19: 1} at 40, 41, 44, 46, and
boot 2's log is byte-identical to both logs of the first capture
(`e315d6a2...`); boot 1's log differs (`cd413399...`). The IRAM twin was 16 in every
sample of every boot.

## What this establishes

1. **The v1 phenomenon reproduces with the v1 image and is absent in the modified
   v2 image; mechanism unknown.** The v1 image (code-identical rebuild, see the
   integrity note) showed the ROM-only
   `clzdi2` variation again in both boots, one of them with a log identical to the
   earlier session and one differing. That supports the phenomenon; it cannot
   retrospectively prove the earlier identical logs were independent boots. The
   v2 image differs from v1 in cadence, driver call depth and register-window
   state, and layout, all at once, so its clean `clzdi2` cells (16 in ROM and IRAM
   at three positions) neither exclude a body-or-ROM interaction nor single out
   cadence.
2. **In the v2 image with core 1 idle or absent, ROM equals IRAM for all ten
   bodies**, every sample (dual-idle and unicore agree cell for cell). This is the
   paired-equality candidate (v1 README model 1) observed in full for that image;
   it remains a windowed-call whole-call equality, not a fetch price, and nothing
   is adopted from it.
3. **The hammer variants do not isolate the ROM.** Busy core 1 slows and scatters
   core 0's timed calls for both ROM and IRAM targets, and the IRAM twin is slowed
   more than the ROM body (for example `abs`: ROM 12-14, IRAM 15-17); the
   ROM-versus-IRAM hammer target makes little difference. Outlier spacing is
   periodic (gaps alternating about 166 and 658 cycles; sample period about 164
   cycles). This is consistent with contention on a shared resource; there was no
   memory-traffic-only control, so the resource is not identified as SRAM, and a
   ROM component is not excluded. It does not by itself explain the v1 phenomenon,
   whose IRAM twin never varied.
4. **Under the declared rule: not attributed.** The unicore variant shows no
   variation, but so does dual-idle; the ROM hammer increases variation, but so
   does the IRAM hammer; IRAM twins vary under both hammers. The rule's pattern
   for "shared-ROM contention from core 1" was not observed.

## What remains open

The v1 phenomenon is consistent with a periodic core-1 activity (the idle task
and 1 ms tick) coinciding with the ROM cell of `clzdi2` under the v1 conditions,
and equally consistent with any other periodic event or interaction that the v2
conditions removed. A candidate next step is the v1 source built with
`CONFIG_FREERTOS_UNICORE=y`; note that this changes IDF scheduling, image layout
and timing, so it is not a layout-identical control and cannot close the cause on
its own. If the outliers vanish, that is consistent with a core-1 or build-related
factor; if they persist, core 1 is not necessary under the new conditions, which
does not exclude it as a contributor to the original. Any further probe needs a
bounded design with matched controls; none is run here. Reviewer's guidance for
such a design (astra, 2026-09-06): keep one binary and layout fixed and vary a
single runtime factor (for example core-1 activity switched at run time), and
first reproduce the v1 phenomenon in that controllable image before any causal
comparison, rather than rebuilding with a different core configuration.

Boot pricing remains blocked; nothing here changes the scope statements of v1.
