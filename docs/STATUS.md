# Status

Last updated 2026-09-02. This file is the current truth: what exists,
what is adopted, and what the hardware queue holds. The goal is
[`GOAL.md`](GOAL.md); the working rules are [`../AGENTS.md`](../AGENTS.md).

## What exists

- Branch `alice` (this branch): upstream esp32sim pinned at `2114ffc`
  plus provenance and a mechanical rustfmt pass, plus this
  documentation and evidence set. Upstream provides the dual-core
  Xtensa LX7 interpreter and native JIT, the ESP32-S3 SoC and
  peripheral models, real-ROM boot, a wasm build (interpreter-only),
  and the web shell. No measured mode exists on this branch yet.
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
| `salvage/core-measured-phase1` | `516b1ad` | `backend-api` crate with fake backend, measured interpreter scheduler, schema-2 profile importer, ledger; 58 tests passing in isolation | Single-core; cache model is an unbounded lifetime set (no eviction, no per-core state); timing mutations are strings in the hot path; timing commits on trapped instructions; interrupt acceptance is unledgered; differential-gate test reverted at HEAD |
| `salvage/board-tinydraw-v2` | `b7c9b87` | Waveshare AMOLED 1.8 board model: GP-SPI DMA, CO5300 command parsing, TCA9554, tear line on GPIO 13, touch state; wasm builds; passes the safeguards script | Touch interrupt (GPIO 21) not driven; DMA descriptor walker unbounded (guest can hang the host); dead ST7701 state wired into the IO expander; PMIC/RTC/IMU are register-RAM stubs (acceptable per GOAL, label them) |
| `salvage/rust-safeguards` | `b138473` | `scripts/pre-commit.sh`: fmt, check, strict clippy, debug and release tests, rustdoc | None known; harvest first |
| `salvage/gp-spi-device-hook` | `246c699` | Upstream-shaped synchronous GP-SPI board-response hook | Candidate for an upstream PR |
| `salvage/ci-spec`, `salvage/upstream-ci` | `6ba6a6d`, `3b58cc6` | CI workflow material | Not yet reviewed in place |
| `salvage/design-spike` | `e22f971` | Design-spike markdown, historical | Do not implement from it |
| `salvage/puck-base` | `3051793` | The base `alice` was cut from | Fully contained in `alice` |

`salvage/core-measured-phase1` and `salvage/board-tinydraw-v2`
diverge from the same ancestor with a 30-file conflict surface; the
integration trunk milestone resolves that once, on `alice`.

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

Known gap: TinyDraw commit `3db39856` (the IDF 6.1 probe fix) is on a
side branch, not TinyDraw main; merging it is part of hardware-batch
preparation.

## Hardware queue

The board has one owner at a time. Everything below needs only the
USB-C cable (CCOUNT probes, GPIO interrupt timestamps, performance
counters, USB Serial/JTAG capture) and should be powered through as
one early batch:

1. Touch controller identity probe: read the I2C ID registers, adopt
   the name (unnamed until then).
2. Tear line timing: GPIO 13 interrupt CCOUNT timestamps for panel
   refresh period and phase.
3. Touch interrupt and transaction timing: GPIO 21 edges plus CCOUNT
   around I2C reads.
4. Panel flush cost: CCOUNT around QSPI flushes of swept sizes, for a
   cycles-per-byte display-path model.
5. GDMA and SPI2 transfer timing sweeps.
6. PSRAM and flash bandwidth under contention: memcpy sweeps with the
   second core active versus idle.
7. Cache maintenance costs: `esp_cache_msync` writeback and invalidate
   by size.
8. Re-capture the six identities below the strict two-receipt bar
   (repeated USB truncation; add capture-side per-line validation
   first).
9. PSRAM long-window cells to strict two-boot status.
10. First-line cache pooling diagnosis (unblocks the first-line cost
    class).
11. Arbitration and cache store/writeback probes (probe code needs
    review first).
12. CCOUNT lock-step against measured mode (needs milestone 2).

Equipment-gated, deferred indefinitely: wire-level QSPI, tear, and I2C
capture (a DSLogic Plus class logic analyzer, roughly 105 to 190 USD,
is the identified buy if panel-side validation is ever needed).

## Next steps

1. Harvest `salvage/rust-safeguards` into `alice`.
2. Integration trunk: harvest the board model and the measured-mode
   material that survives review.
3. Receipt-correlation tests against the adopted numbers above.
4. Wasm JIT cost-accounting spike (GOAL milestone 3).
5. Hardware batch prep: merge TinyDraw `3db39856`, add capture-side
   line validation, write the new probe families (queue items 1 to 7).
