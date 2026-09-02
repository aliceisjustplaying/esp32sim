# Brief 4 derive and boot morning report

## Commits

`0f12e94` derive reset clock configuration from XTAL
`4d0ee81` merge brief 4 reset configuration
`491d4e6` scope timing prices by hardware dependency
`329047b` merge brief 4 timing scope correction
`78d7b63` record conflicting mask ROM fetch residuals
`2ec960b` merge brief 4 ROM fetch finding
`37cd9e0` test wasm JIT window and ledger exits
`9c132a0` emit windowed and classified memory blocks
`50448dc` document wasm JIT window and memory coverage
`6f94e08` merge brief 4 JIT register windows and memory
`7c96315` record blocked exception timing derivation
`296915e` merge brief 4 exception timing finding
`38518e1` stop measured boot at unpriced reset vector
`7bf0d93` merge brief 4 measured boot stop
`28fe6a5` report brief 4 derivation stop
`7dafed1` fit brief 4 report README limit
`4269b0f` merge brief 4 Task 1 report
`42752b4` gate wasm JIT external pricing by chip config
`e0a8fd3` merge brief 4 JIT configuration gate
`21d2cc6` test calibration exception ladder contract
`b6d95f4` document blocked bare-rfe syscall cell
`2fe680a` prepare corrected exception ladder capture
`7f37ed7` require wide syscall epc adjustment
`71176d0` validate contracted console counters
`d5bb458` pin current TinyDraw gate harness
`b685077` remove exception ladder source whitespace
`b89346e` document gate dry-run blocker
`3deb6e5` merge brief 4 calibration images
`42d30f7` analyze IDF 6.1 contention candidates
`a091bbb` record blocked native product boot speed run
`4cec22b` merge brief 4 contention and native-speed findings

## Gate result

PASS locally. Formatting, strict clippy, debug and release workspace tests,
native JIT conformance, release-profile checks, documentation, the complete
wasm JIT spike suite, 67 calibration validator tests, and both reproducible
derivation analyzers pass with the pinned TinyDraw and ROM fixtures. Remote
publication is blocked on explicit approval to export the remaining local
commits to `fork`.

## Task exits

Task 1: STOP RULE FIRED. Reset configuration is 40 MHz CPU and 40 MHz APB.
ROM `memset` gives incompatible fetch candidates: -7/32 cycles per fetch for
the zero-length cell and -11/13,292 for the 0x52e0 cell. Exception E and R are
not derivable because the real IDF paths reach interval-priced `l32r`; the two
window prefixes total 18 cycles. No row was adopted. Two measured boot runs
stop byte-identically at cycle 0, core 0, `_ResetVector` (`0x40000400`), with
`MaskRomInstructionFetch`. This refusal is neither an engine bug nor
R8-derivable.

Task 2: PART 1 PASS, EXIT BLOCKED. The corrected H1 image uses wide `addi` to
advance EPC1 by three before `rfe`; its IDF 6.1 build, verifier, and dry-run
complete 6/6 cells and 600/600 samples with zero refusals. The current-main
gate image and console contract are pinned, and all 67 validator tests pass.
The gate dry-run produces no `TINYDRAW_LIVE_*` lines after 50 billion
instructions because small panel-init SPI2 DMA never completes. No fast-mode
reference was invented.

Task 3: PASS. One hundred random non-overflow window blocks match interpreter
state and cycles; 100 forced overflows match after typed fallback and handler
resume; the committed SRAM kernel ledger is byte-identical. MMIO and cache
paths use typed `ChipConfig` and refuse unmatched configurations by name.
TinyDraw histogram fallback is 30.095 percent, and window modules emit 238.791
bytes per guest instruction.

Task 4: PART 1 PASS, PART 2 BLOCKED. All 103 paired contention identities are
classified. Five authorized product runs reached MaxInsns without READY,
ending at 16.047 billion retired instructions and 66.667 emulated seconds.
The bounded pre-READY median is 111.889 MIPS, 23.310 percent of the 480 MIPS
budget. This is not completed-boot throughput and makes no JIT claim.

## Contention family classifications

| Family | Classification | Identity counts |
| --- | --- | --- |
| Branch | exact | 3 exact |
| Cache burst | exact | 28 exact |
| Cache hit | distribution | 7 exact, 1 interval, 3 distribution |
| Dependent load | interval | 1 exact, 2 interval |
| MMIO read | distribution | 9 exact, 4 distribution |
| MMIO write | exact | 10 exact |
| PSRAM pattern | distribution | 1 exact, 3 distribution |
| ROM routine | distribution | 12 exact, 1 interval, 1 distribution |
| Oracle | distribution | 14 exact, 3 distribution |

## Auditor reports

```text
(•̀ᴗ•́) `alice` 4d0ee81 and `codex/brief4-task1` 0f12e94: GOAL M2, brief Task 1 Part 0.
`codex/brief4-task2` b6d95f4: GOAL M2, brief Task 2 Part 1.
New `PARKED.md` and `test_verify_elf.py`: GOAL M2, brief Task 2 Part 1 H1 inputs.
`codex/brief4-task3` 3311540: GOAL M3, brief Task 3; no commit or new file yet.
`codex/brief4-task4` 3311540: GOAL M5 Part 1 and M3 Part 2, brief Task 4; no commit or new file yet.
TinyDraw has no in-scope branch activity or new files; its detached Task 2 worktree is clean.
Archives contain no in-scope additions within two hours; `briefs/` is excluded.
Drift: none.
R2 violation: “Concurrent SPI2 DMA effect on a 32 KiB PSRAM-to-SRAM CPU copy” is a sequence total in the price table.
R3/R9 violation: CPU/internal rows are hard-scoped to `C240-Q80-O80-I32-D64`, and `price_operation` globally gates them on `ChipConfig::RECEIPT_SCOPE`.
```

```text
(•̀ᴗ•́) `alice` and `codex/brief4-task1`: GOAL M2, brief Task 1 Part 0/R9; reset XTAL derivation and hardware-dependent price scoping. No new files.
`codex/brief4-task3`: GOAL M3, brief Task 3; new `register_windows.rs` and `sram_kernel_ledger.rs` are the window/fallback and ledger-equality exit tests.
`codex/brief4-task4`: GOAL M5, brief Task 4 Part 1; new contention-candidate `README.md`, `analyze.py`, and `summary.json`.
`codex/brief4-task2`: drift; new `PARKED.md` and `test_verify_elf.py` preserve the superseded bare-`rfe` blocker and do not implement brief Task 2’s approved EPC1+3 H1 fix.
TinyDraw has no in-scope commit in the two-hour window; untracked `this-is-a-story-of-project-management-v7.md` is drift. The ignored `blogpost` branch and `scripts/blog-dist` were excluded.
Archive changes are confined to `briefs/`, excluded by instruction; no other recent archive file advances a milestone.
Current `alice` price table has no R2 sequence total and no R3 mis-scoped cost.
Stale Task 2/3/4 branch tables still price the 32 KiB concurrent SPI2 DMA copy delta, an R2 sequence total, and key internal CPU/SRAM rows to `C240-Q80-O80-I32-D64`, an R3 violation.
```

```text
alice and codex/brief4-task1: GOAL M2, prerequisite to M4; brief Task 1. New derived-rom-fetch-idf61/{README.md,analyze.py,summary.json}: M2/T1.
codex/brief4-task2: GOAL M2 and M6; brief Task 2. New exception-ladders/{CMakeLists.txt,README.md,probe-cells.json,sdkconfig.defaults,verify_elf.py,test_verify_elf.py,main/CMakeLists.txt,main/exception_ladders.S,main/exception_ladders.c}: M2/T2.
Drift: exception-ladders/PARKED.md restates the obsolete bare-rfe blocker that brief Task 2 explicitly resolves; it is currently deleted only in the dirty worktree, not yet removed from branch HEAD.
codex/brief4-task3: GOAL M3; brief Task 3. New wasm-jit-spike/tests/{register_windows.rs,sram_kernel_ledger.rs} and src/system.rs: M3/T3.
codex/brief4-task4: GOAL M5; brief Task 4. New contention-candidates-idf61/{README.md,analyze.py,summary.json}: M5/T4.
TinyDraw: no commits in the two-hour window after ignoring blogpost; scripts/blog-dist is ignored. Drift: untracked this-is-a-story-of-project-management-v7.md has no GOAL milestone or brief task.
Archive: no new non-brief entries in the two-hour window; the only recent entry is ignored briefs/.
R2: none; all sequence totals are confined to Correlation targets. R3: none; configuration-dependent price rows are ChipConfig-scoped and the product pricing path refreshes ChipConfig from registers before each measured step.
```

```text
(•̀ᴗ•́) Audit at 11:16 BST after fetch: `git log --since=2h` was empty in both repos; the equivalent `--since='2 hours ago'` exposed the recent esp32sim work below.
`alice` (active trunk): GOAL M2/M3/M4, Brief 4 Tasks 1 and 3; it merges reset/derivation work and the JIT window/memory work.
`codex/brief4-task1`: GOAL M2/M4, Task 1; new: `derived-rom-fetch-idf61/{README.md,analyze.py,summary.json}`, `derived-exception-idf61/{README.md,analyze.py,summary.json}`, `esp32s3/tests/exception_derivation.rs`.
`codex/brief4-task2`: GOAL M2, Task 2 Part 1; new: exception-ladders `CMakeLists.txt`, `README.md`, `main/CMakeLists.txt`, `main/exception_ladders.S`, `main/exception_ladders.c`, `probe-cells.json`, `sdkconfig.defaults`, `test_verify_elf.py`, `verify_elf.py`.
`codex/brief4-task3` (recent, merged): GOAL M3, Task 3; new: `wasm-jit-spike/src/system.rs`, `tests/memory_classes.rs`, `tests/register_windows.rs`, `tests/sram_kernel_ledger.rs`.
`codex/brief4-task4`: GOAL M5, Task 4 Part 1; new: `contention-candidates-idf61/{README.md,analyze.py,summary.json}`; Part 2/M3 native baseline has no files yet.
TinyDraw has no recent commits or in-scope new files; its detached Task 2 worktree is Brief 4 Task 2 supporting GOAL M4/M6. Archive additions are only ignored `briefs/`.
Drift: none after the requested exclusions.
Price table on `alice`: no R2 sequence-total prices and no R3 hardcoded configuration constants; the former SPI2-DMA copy total has been moved to correlation targets.
```

```text
(._.) `alice`: GOAL M2 / Brief Task 1 Parts 0-2; GOAL M3 / Task 3 Parts 1-3.
`codex/brief4-task1`: M2 / Task 1 Parts 0-2; M4 / Task 1 Part 3.
Task 1 new files: `derived-rom-fetch-idf61/{README.md,analyze.py,summary.json}`, `derived-exception-idf61/{README.md,analyze.py,summary.json}`, `exception_derivation.rs`.
`codex/brief4-task2`: M2 / Task 2 Part 1; new exception-ladder CMake, README, source, contract, sdkconfig, and verifier files. Modified `ndjson.py`: M6 / Task 2 Part 2.
`codex/brief4-task3`: M3 / Task 3 Parts 1-3; new `system.rs` and `memory_classes.rs`, `register_windows.rs`, `sram_kernel_ledger.rs`.
`codex/brief4-task4`: M5 / Task 4 Part 1; new contention README/analyzer/summary. Untracked native-speed README/result: M3 / Task 4 Part 2.
TinyDraw has no recent non-ignored commits or files; Archives has no new non-brief entry.
R2: none remains in the price table; the DMA sequence total was moved to correlation targets.
R3: JIT hardcodes scoped MMIO read 9/15/18, enqueue 1 with depth 8, and drain 4/15 in `wasm-jit-spike/src/system.rs` without a register-derived `ChipConfig`.
Drift: none beyond that R3 violation.
```

```text
(｀･ω･´)ゞ Audit snapshot 2026-09-02 11:21 BST; recent esp32sim activity is Brief 4 only, and tinydraw has no recent commits.
`alice`: GOAL M2/M3, Brief Tasks 1/3; integrates the Task 1 and Task 3 files below.
`codex/brief4-task1`: GOAL M2/M4, Task 1; new `derived-{rom-fetch,exception}-idf61/{README.md,analyze.py,summary.json}`, `exception_derivation.rs`, and `derived-brief4-task1-report/README.md`.
`codex/brief4-task2`: GOAL M2/M6, Task 2; new `esp32s3-exception-ladders/{CMakeLists.txt,README.md,probe-cells.json,sdkconfig.defaults,test_verify_elf.py,verify_elf.py,main/{CMakeLists.txt,exception_ladders.S,exception_ladders.c}}`; current ndjson edits advance Part 2.
`codex/brief4-task3-r3`: GOAL M3, Task 3 review; no unique files. Task 3 added `system.rs` and tests `{memory_classes,register_windows,sram_kernel_ledger}.rs`.
`codex/brief4-task4`: GOAL M5/M3, Task 4; new contention `{README.md,analyze.py,summary.json}` and native-speed `{README.md,result.json}`.
Archive: no new non-brief entry within two hours; `briefs/` excluded.
Price-table R2/R3 violations: none; the SPI2 32 KiB sequence was moved to correlation targets, and all external/MMIO prices remain runtime-`ChipConfig` scoped.
Drift: tinydraw’s untracked `this-is-a-story-of-project-management-v7.md`; `blogpost` and `scripts/blog-dist` were excluded as directed.
```

```text
(•̀ᴗ•́) Snapshot 11:24 BST: `alice` is clean; its new Task 1 derivation/report files advance GOAL M2, brief 4 T1, and its new JIT files advance M3, T3.
`codex/brief4-task2`: M2/M6, T2; new exception-ladder image/contract files and untracked `calibration/esp32s3-gate-harness/{README.md,probe-cells.json,verify_elf.py,test_verify_elf.py}`.
`codex/brief4-task3-r3`: M3, T3; no new untracked files, with dirty JIT window/MMIO follow-up edits.
`codex/brief4-task4`: M5 contention and M3 native-speed evidence, T4; new `contention-candidates-idf61/{README.md,analyze.py,summary.json}` and `native-speed-2026-09-02/{README.md,result.json}`.
DRIFT: active `upstream/toolchain-and-ci`, `upstream/rustfmt-sweep`, `upstream/clippy-and-lints`, and `upstream/release-profile-checks` have no brief 4 task number; new files are `rust-toolchain.toml`, `rustfmt.toml`, and `xtensa-lx7/tests/release_profile.rs`.
DRIFT: untracked on `upstream/toolchain-and-ci`: `.githooks/{pre-commit,pre-push}` and `scripts/pre-commit.sh`; `main` worktree has no two-hour activity.
TinyDraw has no non-ignored two-hour activity; the archive listing has no new non-brief files.
R2 price-table violations: none; sequence totals remain under Correlation targets.
R3 price-table violations: none; configuration-dependent rows are keyed by register-derived `ChipConfig` and unmatched scopes refuse.
```

```text
( •̀ᴗ•́ )ゞ Audit report, 2026-09-02 11:30 BST.
`alice`: GOAL M2/M3; brief Tasks 1/3.
`codex/brief4-task2`: GOAL M2/M6; brief Task 2.
`codex/brief4-task4`: GOAL M3/M5; brief Task 4.
Drift branches: `upstream/toolchain-and-ci`, `upstream/rustfmt-sweep`, `upstream/clippy-and-lints`, `upstream/release-profile-checks`, and active `main`; no brief task authorizes them.
Task 1 new files: `docs/evidence/timing/derived-*`, `esp32s3/tests/exception_derivation.rs`; GOAL M2.
Task 3 new files: `wasm-jit-spike/src/system.rs` and its new window/memory/ledger tests; GOAL M3.
Task 2/4 new directories map to GOAL M2/M6 and M3/M5 respectively.
Drift new files: CI/hooks/toolchain/rustfmt/clippy/release-test files on upstream branches, plus TinyDraw `this-is-a-story-of-project-management-v7.md`.
Archive: no non-brief additions; price-table R2 sequence totals: none; R3 hardcoded configuration costs: none.
```

```text
(•̀ᴗ•́) Two-hour audit: TinyDraw had no commits; esp32sim activity is mapped below.
`alice`: GOAL M2/M3/M4/M5/M6; brief Tasks 1-4, integrating all brief-4 outputs.
`codex/brief4-task1`: M2/T1; new `docs/evidence/timing/derived-{rom-fetch-idf61,exception-idf61,brief4-task1-report}/**` and `esp32s3/tests/exception_derivation.rs`.
`codex/brief4-task2`: M2/M4/M6, T2; new `calibration/esp32s3-{exception-ladders,gate-harness}/**`, including the subsequently deleted `PARKED.md`.
`codex/brief4-task3` and `codex/brief4-task3-r3`: M3/T3; new `wasm-jit-spike/src/system.rs` and `tests/{memory_classes,register_windows,sram_kernel_ledger}.rs`.
`codex/brief4-task4`: M5/T4 for contention and M3-support/T4 for native speed; new `docs/evidence/timing/contention-candidates-idf61/**` and `docs/evidence/native-speed-2026-09-02/**`.
Drift branches: `upstream/{toolchain-and-ci,rustfmt-sweep,clippy-and-lints,release-profile-checks}` have no brief-4 task; their new `.githooks/**`, `.github/workflows/ci.yml`, `clippy.toml`, `rust-{toolchain,fmt}.toml`, `scripts/pre-commit.sh`, and `xtensa-lx7/tests/release_profile.rs` are likewise drift.
Drift from recent upstream-main history: new `cli/src/{lib.rs,bin/esp32sim-c3.rs}`, `docs/adding.md`, `esp-soc/**`, `esp32c3/src/soc.rs`, `esp32s3/src/soc.rs`, and `tests/golden/hello-c3.console.actual`; no brief-4 task or product milestone.
TinyDraw drift: untracked `this-is-a-story-of-project-management-v7.md`; ignored as instructed: `blogpost`, `scripts/blog-dist`, and archive `briefs/**`; no other new archive files.
R2/R3: no violations in the price table; sequence totals are confined to correlation targets, and configuration-dependent rows are keyed by register-derived `ChipConfig`.
```

```text
( •̀ᴗ•́ ) `alice`/`fork/alice` aggregates GOAL M2/brief T1, M2+M6/T2, M3/T3, and M3+M5/T4.
`codex/brief4-task1` (+ fork ref): M2/T1; new `derived-{rom-fetch,exception}-idf61/*`, `derived-brief4-task1-report/README.md`, and `esp32s3/tests/exception_derivation.rs`.
`codex/brief4-task2`: M2+M6/T2; new `calibration/esp32s3-{exception-ladders,gate-harness}/*` (including transient/deleted `PARKED.md`).
`codex/brief4-task3` and `codex/brief4-task3-r3` (+ fork refs): M3/T3; new `wasm-jit-spike/src/system.rs` and tests `{memory_classes,register_windows,sram_kernel_ledger}.rs`.
`codex/brief4-task4`: M3+M5/T4; new `native-speed-2026-09-02/*` and `contention-candidates-idf61/*`.
DRIFT: `upstream/{main,clippy-and-lints,toolchain-and-ci,release-profile-checks,rustfmt-sweep}` and `origin/main` have no brief-4 task number.
DRIFT new files: `.githooks/*`, `.github/workflows/ci.yml`, `clippy.toml`, `rust-{toolchain,fmt}.toml`, `scripts/pre-commit.sh`, `xtensa-lx7/tests/release_profile.rs`.
DRIFT new files: `esp-soc/**`, `esp{32c3,32s3}/src/soc.rs`, `cli/src/{lib.rs,bin/esp32sim-c3.rs}`, `docs/adding.md`, `tests/golden/hello-c3.console.actual`.
TinyDraw has no non-ignored commits in the window; the archive has no new non-brief entries.
R2/R3 audit: no sequence total is in the price table, and no configuration-scoped price bypasses the register-derived `ChipConfig`; no offending cost found.
```
