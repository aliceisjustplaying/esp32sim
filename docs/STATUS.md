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
- The shared cache model is merged at `4608137`. One I-cache and one D-cache
  are shared by both live cores, with no per-core cache state. Geometry comes
  from typed `ChipConfig`; unmatched geometry is refused by name. The model
  provides explicit invalidation and writeback, carries no timing, and keeps
  the TRM-silent LRU policy behind `ReplacementPolicy`. All available IDF 6.1
  cold, hot, and hot-hit cache receipts replay deterministically. The missing
  16-line I-cache receipt is tested as model-only behavior, not a measured
  claim.
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

All numbers come from ESP32-S3 rev 0.2 silicon on the physical board via
CCOUNT probes, hardware cache counters where applicable, independent boot
cohorts, and fail-closed parsing. Internal CPU, IRAM, SRAM, and mask-ROM rows
carry no configuration key. `C240-Q80-O80-I32-D64` below scopes external
memory and MMIO rows to a 240 MHz CPU, QIO flash at 80 MHz, octal DTR PSRAM at
80 MHz, 32-byte I-cache lines, and 64-byte D-cache lines. Pricing derives this
`ChipConfig` from the registers firmware programs. Any mismatch on a scoped
row is a typed refusal naming the configuration.

### Price table

Only per-instruction costs and additive delays belong here. A distribution
does not provide a scalar total; measured mode records it at its named tier.

| Cost class | Tier | Price | ChipConfig scope | Receipt |
| --- | --- | --- | --- | --- |
| Straight-line instruction issue | exact | 1 cycle per instruction | none (internal CPU domain) | `evidence/timing/idf61-rebaseline-3db3985/toolchain-delta.json` |
| `mull`, `mulsh`, `muluh`, `nsa`, `nsau`, `sext`, `memw`, `extw`, `rsr`, `wsr`, `xsr`, `rsync`, `movsp`, `min`, `max`, `minu`, `maxu` | exact | 1 cycle per instruction | none (internal CPU domain) | `evidence/timing/esp32s3-opcode-ladders-2026-09-02/summary.json` |
| Conditional branches, wide and narrow | exact | 3 cycles taken, 1 not taken | none (internal CPU domain) | `evidence/timing/esp32s3-opcode-ladders-2026-09-02/summary.json` |
| `j` | exact | 3 cycles | none (internal CPU domain) | `evidence/timing/esp32s3-opcode-ladders-2026-09-02/summary.json` |
| `jx` | exact | 6 cycles | none (internal CPU domain) | `evidence/timing/esp32s3-opcode-ladders-2026-09-02/summary.json` |
| `loop`, `loopnez`, `loopgtz` setup | exact | 5 cycles | none (internal CPU domain) | `evidence/timing/esp32s3-opcode-ladders-2026-09-02/summary.json` |
| `quos`, `quou` | exact | 4 cycles | none (internal CPU domain) | `evidence/timing/esp32s3-opcode-ladders-2026-09-02/summary.json` |
| `rems`, `remu` | exact | 5 cycles | none (internal CPU domain) | `evidence/timing/esp32s3-opcode-ladders-2026-09-02/summary.json` |
| `l32r` | interval | 1 to 2 cycles | none (internal CPU domain) | `evidence/timing/esp32s3-opcode-ladders-2026-09-02/summary.json` |
| `s32c1i` | exact | 6 cycles | none (internal CPU domain) | `evidence/timing/esp32s3-opcode-ladders-2026-09-02/summary.json` |
| `isync` | interval | 6 to 7 cycles | none (internal CPU domain) | `evidence/timing/esp32s3-opcode-ladders-2026-09-02/summary.json` |
| Load-use dependency at distance 1 | exact | +1 additive cycle; distance 2 is +0 | none (internal CPU domain) | `evidence/timing/esp32s3-opcode-ladders-2026-09-02/summary.json` |
| Independent aligned SRAM load or store | exact | +0 additive cycles | none (internal CPU domain) | `evidence/timing/idf61-rebaseline-3db3985/receipts/boot-1-recovered.tar.gz` |
| Hot I-cache hit from flash | exact | +0 additive cycles | `C240-Q80-O80-I32-D64` | `evidence/timing/esp32s3-rev02-tinydraw-1ddd64b-4a2c659-hot-hit-adoption.json` |
| Hot D-cache load hit from flash or PSRAM | exact | +0 additive cycles | `C240-Q80-O80-I32-D64` | `evidence/timing/esp32s3-rev02-tinydraw-1ddd64b-4a2c659-hot-hit-adoption.json` |
| Loop body alignment at residue +3 mod 4 | exact | +1 additive cycle per iteration | none (internal CPU domain) | `evidence/timing/idf61-rebaseline-3db3985/toolchain-delta.json` |
| First cache-line fill, I-flash / D-flash / D-PSRAM | exact | 203 / 114 / 81 cycles | `C240-Q80-O80-I32-D64` | `evidence/timing/idf61-rebaseline-3db3985/toolchain-delta.json` |
| Subsequent cache-line fill, I-flash / D-flash / D-PSRAM | interval | adopted centers 266 / 473 / 170 cycles, ladder residuals ±1 / ±2 / ±2 | `C240-Q80-O80-I32-D64` | `evidence/timing/esp32s3-rev02-tinydraw-a91d1d7-cache-burst-adoption.json` |
| MMIO read, SYSTEM / SENSITIVE / EXTMEM / ASSIST_DEBUG | exact | 9 cycles | `C240-Q80-O80-I32-D64` | `evidence/timing/esp32s3-register-blocks-2026-09-02/summary.json` |
| MMIO read, APB peripheral blocks | exact | 15 cycles | `C240-Q80-O80-I32-D64` | `evidence/timing/esp32s3-register-blocks-2026-09-02/summary.json` |
| MMIO read, NRX | exact | 18 cycles | `C240-Q80-O80-I32-D64` | `evidence/timing/esp32s3-register-blocks-2026-09-02/summary.json` |
| MMIO read, RTC | distribution | 80.203125 to 80.96484375 cycles observed | `C240-Q80-O80-I32-D64` | `evidence/timing/esp32s3-register-blocks-2026-09-02/summary.json` |
| MMIO read, eFuse | distribution | 80.34375 to 80.82421875 cycles observed | `C240-Q80-O80-I32-D64` | `evidence/timing/esp32s3-register-blocks-2026-09-02/summary.json` |
| MMIO write enqueue while posted buffer has room | exact | 1 cycle per write, depth 8 | `C240-Q80-O80-I32-D64` | `evidence/timing/esp32s3-register-blocks-2026-09-02/summary.json` |
| MMIO write steady drain, SYSTEM / SENSITIVE / EXTMEM / ASSIST_DEBUG | exact | 4 cycles per write | `C240-Q80-O80-I32-D64` | `evidence/timing/esp32s3-register-blocks-2026-09-02/summary.json` |
| MMIO write steady drain, APB peripheral blocks | exact | 15 cycles per write | `C240-Q80-O80-I32-D64` | `evidence/timing/esp32s3-register-blocks-2026-09-02/summary.json` |
| MMIO write steady drain, NRX | interval | 17 to 18 cycles per write | `C240-Q80-O80-I32-D64` | `evidence/timing/esp32s3-register-blocks-2026-09-02/summary.json` |
| MMIO write steady drain, RTC | distribution | 69.7265625 to 70.62890625 cycles observed | `C240-Q80-O80-I32-D64` | `evidence/timing/esp32s3-register-blocks-2026-09-02/summary.json` |

### Correlation targets

These are measured sequence totals. They test the sum of independently priced
instructions and delays and are never themselves prices.

| Sequence | Tier | Receipt target | Receipt |
| --- | --- | --- | --- |
| 256 `call0` or `callx0` plus `ret` pairs | exact | 1,664 cycles | `evidence/timing/esp32s3-opcode-ladders-2026-09-02/summary.json` |
| 256 `call8` or `callx8` plus `retw` pairs | exact | 1,920 cycles | `evidence/timing/esp32s3-opcode-ladders-2026-09-02/summary.json` |
| Window overflow plus underflow pair past depth 6 | exact | 35 cycles | `evidence/timing/idf61-rebaseline-3db3985/toolchain-delta.json` |
| Interrupt level 1 entry / resume | exact | 227 / 143 cycles | `evidence/timing/idf61-rebaseline-3db3985/toolchain-delta.json` |
| Interrupt level 3 entry / resume | exact | 222 / 139 cycles | `evidence/timing/idf61-rebaseline-3db3985/toolchain-delta.json` |
| ROM `memset`, zero length / 0x52e0 bytes | exact | 31 / 6,659 matched cycles | `evidence/timing/idf61-rebaseline-3db3985/receipts/boot-1-recovered.tar.gz` |
| ROM BBPLL first / steady same-value write | exact | 836 / 835 matched cycles | `evidence/timing/rom-i2c-bbpll-0a41b6f/README.md` |
| ROM `_xtos_set_intlevel(0x00040c00)` restore | exact | 15 matched cycles | `evidence/timing/esp32s3-rev02-tinydraw-d42615b-xtos-intlevel-adoption.json` |
| `rgb565_stage_five_scalar_oracle_hot` | exact | 50 cycles | `evidence/timing/idf61-rebaseline-3db3985/receipts/boot-1-recovered.tar.gz` |
| Concurrent SPI2 DMA during a 32 KiB PSRAM-to-SRAM CPU copy | exact observation | active-minus-idle paired medians 3.5 / 0 cycles; no scalar price adopted | `evidence/timing/esp32s3-dma-sram-2026-09-02/summary.json` |
| SPI2 quad 40 MHz 32 KiB submit / submit-to-complete | exact | 5,755 / 401,589 cycles | `evidence/timing/esp32s3-dma-sram-2026-09-02/summary.json` |

The IDF 6.1 rebaseline has 210 identities: 105 single-core identities were
examined, 103 contended identities were excluded for milestone 5, and the two
missing identities are contended RTC cells. Its additive and per-instruction
results are all represented above: issue, independent SRAM access, dependent
load-use, hot instruction and load hits, branch direction, loop alignment,
MMIO, and cache fills. The unaligned-access, ROM, RGB565, flash-map, PSRAM,
reset-reason, and cache-ladder bodies are sequence or workload totals; R2
prevents treating those totals as prices. RTC and long-window PSRAM remain
distribution-tier observations.

Boot to first output is a non-ledger observation: median 0.472351875 seconds
under IDF 6.1, recorded in
`evidence/timing/idf61-rebaseline-3db3985/toolchain-delta.json`.

Toolchain rule: every receipt pins its ESP-IDF version, sdkconfig
hash, and compiler. The current baseline is ESP-IDF v6.1 with
xtensa-esp-elf 15.2.0. IDF 6.1 is authoritative: where two toolchains
disagree at probe level, the IDF 6.1 value is adopted. No receipt mixes
toolchain versions within a cohort.

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

H1. Capture the IDF 6.1 `esp32s3-exception-ladders` image: 100 samples each
for non-tail `call4`, `call8`, and `call12` recursion past the register-file
knee, `syscall` with a bare level-1 `rfe` handler, `rfe` alone with EPC1 set to
the next instruction, and `rfi 3` alone with EPC3 set to the next instruction.
Add a straight-line mask-ROM instruction-fetch cell. The two existing ROM
`memset` paths produce different, noninteger residuals and cannot adopt a ROM
fetch price under R8; receipt:
[`evidence/timing/derived-rom-fetch-idf61/`](evidence/timing/derived-rom-fetch-idf61/README.md).
Require zero cache-counter deltas, verified encodings, and a passing emulator
dry-run. The estimated board time is about two seconds. Nothing is flashed
until the maintainer starts the next capture session.

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

- The interpreter-versus-native-JIT architectural conformance gate is merged
  at `2d42032`. Its committed corpus and 128 deterministic randomized SRAM
  blocks compare registers, PC, touched memory, and the complete SRAM image.
- Milestone 2's committed TinyDraw SRAM kernel fixture and deterministic ledger
  pass on explicitly pinned core 0. Shared cache state is transactional across
  both live cores. The previous window attempt walked `_WindowOverflow12` and
  `_WindowUnderflow12` by assigning PC every three bytes, so it neither matched
  the receipt's `callx8` `_WindowOverflow8` and `_WindowUnderflow8` pair nor
  executed a real exception. The corrected recursion reaches the first real
  `_WindowOverflow8` entry and refuses. Two receipt classes are missing:
  exception-entry delay and the `rfwo` and `rfwu` instruction costs. The
  correlation stays ignored pending hardware queue item H1.
- ESP32-S3 TRM v1.8, section 4.3.3.2, page 405, specifies one
  dual-core-shared I-cache and one dual-core-shared D-cache, with each core
  on its own bus. GOAL and the merged Task 5 cache model adopt that topology.
  LRU is isolated behind a policy enum because the TRM does not specify a
  replacement policy.
- Root `LICENSE` file on the fork: waiting on the upstream author; the maintainer owns the contact.
- `.github/workflows/pages.yml` fetches the mask ROM unpinned from `releases/latest`: dormant because it triggers only on `main`, the upstream mirror; pin or remove it when the workflow is next touched.
- `periph.rs` and `machine.rs` decomposition: deliberately deferred; extract only what Task 3b forces under the build-exactly-the-thing rule.

## Proposed next steps

1. Proposal: amend H1 before capture so the syscall handler advances EPC1 by
   the three-byte syscall width before `rfe`, matching IDF 6.1. Treat the known
   handler instructions as terms in the timing equation, rebuild the verifier,
   and require a passing emulator dry-run after the maintainer approves this
   change to the capture contract. The requested bare-`rfe` cell is parked on
   `codex/brief3-task5` because it returns to the same syscall indefinitely.
2. Proposal: add per-frame firmware counters to the next maintainer capture so
   frame-scale correlation can begin after measured boot reaches READY.
3. Proposal: begin milestone 5 contention pricing only after the measured boot
   findings identify the first shared-cache or MSPI arbitration boundary.
