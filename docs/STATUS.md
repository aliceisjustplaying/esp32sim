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
Tier B probe drafts remain ready for review. Their fail-closed NDJSON path
covers 28 Tier B cells across six families. A
committed objdump gate verifies the probe issue-block encodings and all
four loop-body residues modulo 4 before capture; mismatch exits nonzero
without a result file. Task 5-prep did not flash the board or open serial.

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

Tier B, probe drafts are committed and ready for review (one unified
timing image where practical, two clean independent boots each):

6. Arbitration aggressors (internal, flash, PSRAM) with a start
   barrier and attributable cache counters.
7. Hot external-cache store-hit probe.
8. Clean-versus-dirty writeback ladders (1, 2, 4, 8, 16 lines).
9. Instruction-PSRAM hot and cold fetch probes.
10. First-line cache pooling probe (diagnoses the one-cycle IDF 6.1
    shift; unblocks the first-line cost class).
11. Selective cohort rerun mode (so USB truncation recovery stops
    costing full-suite boots).
12. Display-path and DMA cost families for GOAL's cost classes:
    panel QSPI flush sweeps (cycles per byte), GDMA and SPI2
    transfer sweeps, touch I2C transaction timing, GPIO 21 edge
    timing, `esp_cache_msync` writeback and invalidate by size, and
    PSRAM and flash bandwidth under cross-core contention.
13. Optional DMA descriptor marker hook to correlate GP-SPI2/GDMA
    activity with a later electrical capture.

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

1. Extend measured interpreter coverage as additional cost classes are adopted.
2. Execute the maintainer-owned hardware batch from TinyDraw `5f38ca5`: tier A,
   then the prepared Tier B cells.
3. Attach the architectural interpreter-versus-JIT conformance gate to the
   first costed-JIT task before product-JIT work begins.
