# Status

Last updated 2026-09-02. This file is the current truth: what exists,
what is adopted, and what the hardware queue holds. The goal is
[`GOAL.md`](GOAL.md); the working rules are [`../AGENTS.md`](../AGENTS.md).

## What exists

- Branch `alice` (this branch): upstream esp32sim pinned at `2114ffc`
  plus provenance, strict Rust safeguards, and this documentation and
  evidence set. The safeguards pin Rust 1.98.0, apply workspace-wide
  deny-level lints, preserve release overflow checks and debug
  assertions, and split the fast commit gate from the full push battery.
  The Xtensa and RISC-V objdump-differential corpora are committed in-tree;
  either test fails if its corpus is absent. Upstream provides the dual-core
  Xtensa LX7 interpreter and native JIT, the ESP32-S3 SoC and
  peripheral models, real-ROM boot, a wasm build (interpreter-only),
  and the web shell.
- Task 3a's initial measured core is merged at `a142952`. Task 3b wires it
  into the product interpreter through `Esp32Backend` at `88ae4dd`, with the
  full adopted-cost replay for both fake and real backends at `808581f`.
  Both adapters use the same typed transaction engine and structurally
  dual-core scheduler state. The product path delivers board deadlines and
  timestamped GPIO edges before the next transaction, commits timing only
  after architectural success, and emits deterministic canonical ledgers.
  Level 1 and level 3 interrupt entry and matching architectural resume are
  priced, typed ledger transactions using the committed IDF 6.1 receipt.
  Accepted interrupt context is tracked per core and consumed only after the
  matching return instruction succeeds. RFUE, unmatched or mismatched returns,
  unsupported interrupt levels, unknown MMIO, first-line fills, and unadopted
  instruction classes fail closed.
- The TinyDraw V2 board harvest is merged. It includes GP-SPI2 DMA,
  CST820 touch, the CO5300 panel, timestamped GPIO edges, browser touch,
  and the paced-stroke workflow, with the defect dispositions below.
- The wasm JIT accounting spike is committed at
  [`evidence/wasm-jit-accounting-spike-2026-09-01/result.json`](evidence/wasm-jit-accounting-spike-2026-09-01/result.json).
  On an Apple M1 Pro in Chrome 151, the accounting-off median is 10,486.56
  MIPS and accounting-on is 4,478.24 MIPS, a 57.30 percent accounting cost.
  The accounting-on ceiling clears 480 MIPS by 3,998.24 MIPS (832.97
  percent). This is a ceiling measurement, not product-JIT throughput.
- Branch `main`: clean upstream mirror.
- Branches `salvage/*`: frozen earlier work, inventoried below.
- The TinyDraw repository holds probe and reference firmware and the
  capture tooling; it stays live and separate.
- An adversarial review of all salvage material exists in the archived
  predecessor repository (its final state, `reviews/` directory).
  Harvesting from salvage should consult it.

## Salvage inventory

Read-only inputs. Harvest under review; do not resume.

| Branch | Head | Contents | Known defects to review before harvest |
| --- | --- | --- | --- |
| `salvage/core-measured-phase1` | `516b1ad` | Reimplemented under review at `a142952` as the typed backend API, transaction engine, structurally dual-core scheduler, `FakeBackend` contract, and exact receipt tests | The salvage defects were not carried into Task 3a; interpreter integration, cache behavior, interrupt accounting, and the interpreter-versus-JIT gate remain later work |
| `salvage/board-tinydraw-v2` | `b7c9b87` | Harvested at `30b7c8e` and `8dee48d`. Taken: generic GP-SPI with MISO, GP-SPI2 DMA delivery, CST820 touch, CO5300 panel, TCA9554, timestamped GPIO 13 tear and GPIO 21 touch edges, browser touch, and the one-command TinyDraw paced-stroke workflow. Dropped: the retrospective `input_changes(cycles)` API, the AMOLED board's dead ST7701 coupling, and the separate example script. | Dispositioned: the DMA walker has a 1,024-descriptor step budget, visited set, and typed read, cycle, and budget faults; GPIO 21 drives an active-low interrupt edge; the 60 Hz TE model remains explicitly an approximate compatibility signal with no adopted timing claim; PMIC, RTC, and IMU devices are labeled register-RAM stubs. The paced stroke and wasm build pass. |
| `salvage/rust-safeguards` | `b138473` | `scripts/pre-commit.sh`: fmt, check, strict clippy, debug and release tests, rustdoc | Harvested under review; frozen source retained |
| `salvage/gp-spi-device-hook` | `246c699` | Upstream-shaped synchronous GP-SPI board-response hook | Candidate for an upstream PR |
| `salvage/ci-spec`, `salvage/upstream-ci` | `6ba6a6d`, `3b58cc6` | CI workflow material; decoder-conformance intent harvested at `4e5f47e` | The mandatory Xtensa and RISC-V corpora are in-tree and absence fails the tests; remaining CI material is not yet reviewed in place |
| `salvage/design-spike` | `e22f971` | Design-spike markdown, historical | Do not implement from it |
| `salvage/puck-base` | `3051793` | The base `alice` was cut from | Fully contained in `alice` |

`salvage/core-measured-phase1` and `salvage/board-tinydraw-v2`
diverge from the same ancestor with a 30-file conflict surface; the
integration trunk milestone resolves that once, on `alice`.

## Board identity

- Touch controller: CST820, adopted for the exact V2 board from the
  on-device identity probe (I2C `0x15`, identity registers `0xA7/0xA8/0xA9`
  returned `0xB7/0x41/0x02`). Receipt:
  [`evidence/board-touch-identity-2026-09-01/`](evidence/board-touch-identity-2026-09-01/README.md).
- Panel controller: CO5300-class QSPI AMOLED.
- Chip: ESP32-S3 QFN56 revision v0.2.

## Adopted timing numbers

All from ESP32-S3 rev 0.2 silicon on the physical board, via CCOUNT
probes with hardware cache counters, two-boot cohorts, fail-closed
parsing. Receipts under [`evidence/timing/`](evidence/timing/); the
IDF 6.1 rebaseline ledger is
[`evidence/timing/idf61-rebaseline-3db3985/`](evidence/timing/idf61-rebaseline-3db3985/README.md)
(802 passing receipts, 210 identities, 204 at the strict
two-independent-receipt bar).

| Claim | Value | Receipt |
| --- | --- | --- |
| Straight-line SRAM issue | 1.000 cycles per instruction | `evidence/timing/esp32s3-rev02-tinydraw-bf169bc-counters-candidate.json` (candidate status; derivation in `evidence/timing/README.md`) |
| Window overflow plus underflow pair | 35 cycles | `evidence/timing/idf61-rebaseline-3db3985/toolchain-delta.json` |
| Loop alignment penalty | +1 cycle per iteration at body residue +3 mod 4 | same delta file |
| Taken/not-taken `beqz` | 3 / 1 cycles | `evidence/timing/esp32s3-rev02-tinydraw-2bf3ffd-beqz-adoption.json` |
| MMIO write cost (same-value run of n) | affine, 3n minus 8 cycles | `evidence/timing/esp32s3-rev02-tinydraw-e8a9f0e-mmio-write-adoption.json` |
| Cache line fill, subsequent lines (I-flash / D-flash / D-PSRAM) | 266 / 473 / 170 cycles | `evidence/timing/esp32s3-rev02-tinydraw-a91d1d7-cache-burst-adoption.json` |
| Cache line fill, first line | blocked, not adopted | one-cycle probe shift between IDF 6.0.2 and 6.1 is undiagnosed; `evidence/timing/idf61-rebaseline-3db3985/toolchain-delta.json` (`adopted: false`) |
| Interrupt entry / resume, level 1 | 227 / 143 cycles (IDF 6.1) | same delta file |
| Interrupt entry / resume, level 3 | 222 / 139 cycles (IDF 6.1) | same delta file |
| Boot to first output | 0.472 s median (IDF 6.1) | same delta file |

Toolchain rule: every receipt pins its ESP-IDF version, sdkconfig
hash, and compiler. The current baseline is ESP-IDF v6.1 with
xtensa-esp-elf 15.2.0. Silicon-architectural numbers must not move
across toolchains; anything IDF-owned (interrupt paths, boot) must be
re-measured on a bump. No mixing versions within a cohort.

## Fixtures

Built from TinyDraw at `3db39856` under ESP-IDF v6.1. The ELFs are
machine-local to the maintainer (not committed); hashes pin them:

- Panel-probe ELF SHA-256
  `143e9f5185d010a8b5344ee5ed2c82a99928dba6839a84d746219d9045de468f`
- Vector demo ELF SHA-256
  `1b0475db6ab30e1e6b6ee07ae77ae46b21c874cac64a736e5ba86604a68234ce`
- Gate-harness ELF SHA-256
  `4e121a3642a6f18766cfe96c2be6adc8a0017fba4afa82105d642168ea40e2c8`

The fixture source is published on TinyDraw branch
`codex/lane-0-idf61-probes` at `632c966`. TinyDraw pull request 4
(branch `maintenance/idf61-probes` at `0835e5b`) carries the IDF 6.1
probe and receipt commits plus review fixes and is pending maintainer
test and merge; normal-product validation used TinyDraw `2643aa7`.

## Hardware queue

The board has one owner at a time. Front-load everything USB-C can
reach (CCOUNT probes, GPIO interrupt timestamps, hardware cache
counters, USB Serial/JTAG capture) as one early batch, in tiers:

The Tier A batch captured product firmware at TinyDraw `5f38ca5` with capture
tooling at `fe0ee64`; both are contained in current TinyDraw `main` at
`6ecc1fd`. The accepted receipt is
[`evidence/timing/tier-a-2026-09-01/`](evidence/timing/tier-a-2026-09-01/README.md).
The Tier B batch is complete from TinyDraw
`fc6d9347549730a0e57aa926f8f6935e12636844`. Its candidate receipt is
[`evidence/timing/tier-b-2026-09-01/`](evidence/timing/tier-b-2026-09-01/README.md).
Two normal boots each completed 25 cells and 198 samples; two XIP PSRAM boots
each completed 26 cells and 211 samples. All four have distinct boot identities
and zero refusals. Cross-variant analysis classifies all 24 shared cells and
their families. The dirty-writeback, SPI2, and cache-msync writeback totals are
affine, but their CPU, cache, and device components are mathematically
underdetermined. These captures adopt no measured-mode costs.

The Tier B decomposition follow-up is complete from clean TinyDraw
`7a157d44a9da3312b1ecda2b45b116af2de28e63`. Its candidate receipt is
[`evidence/timing/tier-b-decomposition-2026-09-01/`](evidence/timing/tier-b-decomposition-2026-09-01/README.md).
Two normal boots each completed 43 cells and 360 samples; two XIP PSRAM boots
each completed 44 cells and 373 samples. All four have distinct boot identities
and zero refusals. These captures adopt no measured-mode costs.

Tier A dispositions:

1. Parked: the IDF 6.1 rebaseline receipt pins TinyDraw commit
   `3db39856f0a04266a42aef8cd5ead1be6fc8eca4`, but that object is absent
   after fetching every TinyDraw remote. The maintainer directed captures
   from current `main`; a current-main full-suite capture cannot be joined
   to the pinned cohort without an explicit source-equivalence receipt. The
   six receipt-gap identities remain open.
2. Complete: two accepted independent IDF 6.1 core-timing boots corroborate
   the existing exact values for the window pair, straight-line issue, loop
   alignment, and level 1 and level 3 interrupt entry and resume. Boot 1 is
   retained by archive hash as a rejected noncanonical diagnostic.
3. Complete: 30 contiguous successful product resets retain reset-to-ready
   as a distribution only. The median is 2.7968129584987764 seconds and the
   nearest-rank p90 is 2.8008790419989964 seconds; this is not an acceptance
   bound.
4. Complete: all 30 product resets retain internally measured TE telemetry
   as diagnostic only. The median period is 16,806 microseconds and median
   high time is 579 microseconds; interrupt latency means neither is adopted
   panel timing.
5. PSRAM long-window (complete offline): the four cold PSRAM cohorts have
   3, 4, 4, and 4 eligible independent boots. They are retained as
   distribution candidates only, with no recapture needed. Receipt:
   [`evidence/timing/psram-long-window-idf61-3db3985/`](evidence/timing/psram-long-window-idf61-3db3985/README.md).

Tier B dispositions:

6. Complete: arbitration aggressors for internal, flash, and PSRAM sources
   were captured with a start barrier and attributable cache counters.
7. Complete: the hot external-cache store-hit probe was captured.
8. Complete: clean and dirty writeback ladders at 1, 2, 4, 8, and 16 lines
   were captured.
9. Complete: instruction-PSRAM hot and cold fetch probes were captured in the
   XIP PSRAM image.
10. Complete as candidate evidence: first-line cache pooling was captured. It
    does not yet unblock the adopted first-line cost class.
11. Complete: selective cohort rerun is implemented and validated. The
    canonical cohort used full clean boots.
12. Complete except GPIO 21: panel QSPI, GDMA, SPI2, touch I2C, cache msync,
    and cross-core PSRAM and flash bandwidth families were captured. GPIO 21
    edge timing remains open because the session excluded its open-refusal
    cell.
13. Deferred as optional: the DMA descriptor marker remains available for a
    later electrical capture and is not part of this canonical cohort.
14. Cache-msync decomposition complete as candidate evidence: the total remains
    unexplained because its matched-clean baseline fails the affine threshold.
    The dirty C2M delta is an affine candidate at 161.334240342 cycles per dirty
    line at 80 MHz plus 125.245246779 at 40 MHz. The 4 KiB, 64-miss service
    controls remain distributions, not a universal line cost. Nothing is
    product-adopted because the C2M interval is not yet a non-double-counted
    measured transaction. Receipt:
    [`evidence/timing/tier-b-decomposition-2026-09-01/`](evidence/timing/tier-b-decomposition-2026-09-01/README.md).
15. SPI2 decomposition complete as candidate evidence: the rank-8 phase design
    reconciles all 216 samples exactly. Submission and completion fixed costs
    remain distributions. Device serialization is exact at 96 cycles per byte
    at 20 MHz, and exact at 48 cycles per byte at 40 MHz only from 4 KiB through
    32 KiB. Phased totals do not replace the prior blocking receipt, and no
    product transaction adopts these candidates.

No further board sessions are scheduled. The hardware queue is a queue, not
active work. Hardware resumes only when a milestone 2 through 4 cost class
lacks a receipt. Display-path and DMA decomposition are parked until milestone
4 is complete.

Tier C, equipment-gated, deferred indefinitely: ten-signal electrical
capture (QSPI chip select, clock, four data lines, GPIO 13 TE, I2C
SDA/SCL, GPIO 21 touch interrupt) resolving the 40 MHz bus, cold
reset through one known frame, at least 120 TE edges, plus
human-held touch landmarks. A DSLogic Plus class analyzer (roughly
105 to 190 USD) is the identified buy. The full capture contract is
in the archived predecessor repository (request A-01).

Tier D, blocked on emulator work: CCOUNT lock-step against measured
mode (needs GOAL milestone 2).

## Review residuals

- Interpreter-versus-JIT architectural conformance test: required before any costed-JIT work; attach it to the first JIT task.
- Root `LICENSE` file on the fork: waiting on the upstream author; the maintainer owns the contact.
- `.github/workflows/pages.yml` fetches the mask ROM unpinned from `releases/latest`: dormant because it triggers only on `main`, the upstream mirror; pin or remove it when the workflow is next touched.
- `periph.rs` and `machine.rs` decomposition: deliberately deferred; extract only what Task 3b forces under the build-exactly-the-thing rule.

## Next steps

1. Milestone 2 is next: make `step_measured` run one real SRAM-resident kernel
   end to end, produce its typed cycle ledger, and pass a committed replay test
   against that ledger.
2. Extend measured interpreter coverage only as the end-to-end kernel needs
   additional receipt-backed cost classes. Unknown classes remain fail-closed.
3. Attach the architectural interpreter-versus-JIT conformance gate to the
   first costed-JIT task before product-JIT work begins.
4. Resume display-path and DMA decomposition after milestone 4 is complete.
