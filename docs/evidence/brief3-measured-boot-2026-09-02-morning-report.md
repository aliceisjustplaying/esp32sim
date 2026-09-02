# Brief 3 measured boot morning report

## Commits

`1acbe63` test measured TinyDraw kernel exit
`5b3e262` remove sequence-total pricing state
`45322a9` derive timing configuration from machine registers
`d85a57c` price adopted instruction and memory rows
`c6e7926` run pinned TinyDraw SRAM kernel in measured mode
`40ea360` record real handler correlation gap
`4f2e433` wire measured pricing to shared cache model
`92d2f2d` pin measured acceptance paths to core zero
`6056b5c` commit measured correlation fixtures
`f6a1ac2` remove nonexecuting correlation attempts
`f53fba9` name unpriced exception timing classes
`fa77609` exercise real callx8 window exception
`3b421cd` correct measured window finding and queue H1
`7204550` scope SPI2 additive receipt to 32 KiB
`4bdf1d5` test measured cache burst and dirty eviction exit
`4f44ff6` refuse unpriced dirty cache evictions
`85a0494` route PSRAM fetches through shared cache
`9c0ef4a` test SPI2 DMA receipt deadline and affine refusal
`da5e01b` defer SPI2 GDMA completion to receipt deadline
`eb836ff` test wasm branch and loop conformance
`2203b80` emit wasm control flow with cycle accounting
`7ed4ff5` document wasm JIT coverage metrics
`5ccbe4f` test deterministic measured TinyDraw boot exit
`981bdc1` stop measured boot at typed first refusal
`a6a51d4` move measured backend fixtures out of mask ROM
`591ca01` fail closed on waiting-core deadline errors
`2c58623` park impossible bare-rfe capture proposal
`0ae3148` test calibration exception ladder contract, parked on `codex/brief3-task5`
`a3df529` document blocked bare-rfe syscall cell, parked on `codex/brief3-task5`

## Gate result

PASS. The final push safeguard completed formatting, strict clippy, debug and
release workspace tests, native JIT conformance, release-profile checks, and
documentation. Each workspace profile passed 72 tests with one expected ignored
window correlation. The real measured-boot test used the two required fixture
environment variables and passed twice with byte-identical output.

## Task exits

Task 0: PASS. Committed fixtures drive the SRAM kernel exit. The real callx8
window path reaches `_WindowOverflow8` and stays ignored with the required
receipt-blocked reason.

Task 1: PASS. Rebaseline cache bursts pass through `step_measured`; dirty
eviction refuses by typed class; the Task 0 SRAM ledger is unchanged.

Task 2: PASS. The real ROM, bootloader, and application stop deterministically
at the first typed refusal at cycle 0. Both runs are byte-identical.

Task 3: PASS. A 32,768-byte quad SPI2 DMA transfer at 40 MHz completes at
exactly T plus 401,589 cycles. A 64-byte payload refuses as an unpriced affine
candidate.

Task 4: PASS. One hundred random branch and loop blocks match interpreter
architectural state and measured ledger totals. TinyDraw fallback is 66.362
percent; emitted size is 146.915 bytes per guest instruction.

Task 5: PARKED. A bare level-1 `rfe` returns to the same `syscall`, so the
required cell cannot produce one sample. IDF 6.1 and emulator evidence are on
`codex/brief3-task5` at `a3df529`. Nothing was flashed. The amended capture is
marked as a proposal in `docs/STATUS.md`.

## Task 2 refusal histogram

```json
{
  "schema": 1,
  "image": {
    "elf_environment": "TINYDRAW_VECTOR_V2_BUILD",
    "elf_sha256": "7f598fd3580cf52078fb6aa04a5f6fe5179b0de9d89bb6468fdb06ed5e40e424",
    "rom_elf_sha256": "c0ce0f338d1de1bdc6efbef1591779a2a42c1ab7d759d3c6ae8ae63a7dd34cfd"
  },
  "outcome": "refusal",
  "boot_cycle": 0,
  "core_cycles": [0, 0],
  "ready": false,
  "deterministic_runs": 2,
  "refusals": [
    {
      "class": "MaskRomInstructionFetch",
      "chip_config": {
        "cpu_mhz": 0,
        "flash_mode": "Other",
        "flash_mhz": 160,
        "psram_mode": "Other",
        "psram_mhz": 160,
        "icache_size_bytes": 16384,
        "icache_ways": 4,
        "icache_line_bytes": 16,
        "dcache_size_bytes": 32768,
        "dcache_ways": 8,
        "dcache_line_bytes": 16
      },
      "count": 1,
      "first_core": "Core0",
      "first_pc": "0x40000400",
      "first_symbol": "_ResetVector"
    }
  ]
}
```

## Boot correlation

Not reached. READY was not reached, so no delta from the 2.7968-second median
is claimed.

## Auditor reports

```text
(¬‿¬) `alice` @ e3dc340: GOAL M2, brief T0 precondition/base; clean.
`codex/overnight-task2` @ 3b421cd: M2/T0; fixtures, refusals, real callx8 correlation, and H1 status; clean.
Recent refs: `overnight-task5` M2/T1; `overnight-task4` M3/T4; `overnight-task3` M2/T4; `overnight-task1` M2/T1,3,4; findings/status M2/T0.
TinyDraw `blogpost`, plus untracked `scripts/blog-dist` and `this-is-a-story-of-project-management-v7.md`: drift, no GOAL milestone or brief task.
Archive opcode-ladder captures: M2/T0 and M3/T4; register-block captures: M2/T2; DMA capture: M2/T3.
Both new brief files are orchestration inputs for M2/T0,1,2,3,5 and M3/T4.
Committed new fixtures: M2/T0; cache model files: M2/T1; wasm JIT files: M3/T4; timing evidence: M2/T1,2,3.
R2 violations: none; SPI2 5,755/401,589 and window-pair 35 remain correlation targets.
R3 violations: none; configuration-scoped prices use `ChipConfig` derived from programmed registers.
```

```text
( •̀ᴗ•́ ) `alice` at 3b421cd: GOAL M2, brief Task 0.
`codex/brief3-task1`: M2, Task 1.
`codex/brief3-task3`: M2, Task 3.
`codex/brief3-task4`: M3, Task 4.
`codex/brief3-task5`: M2, Task 5.
New fixtures/correlation files: M2/T0; cache files: M2/T1; JIT files: M3/T4; exception verifier: M2/T5.
New timing evidence directories are the brief’s adopted M2 inputs; the two new brief files are orchestration records.
Drift: TinyDraw `blogpost`, untracked `scripts/blog-dist`, and `this-is-a-story-of-project-management-v7.md`.
R2: none; sequence totals remain under correlation targets.
R3: `Concurrent SPI2 DMA effect…` lacks its 32 KiB, quad, 40 MHz scope in the pricing key, so its +0 cost is configuration-hardcoded.
```

```text
(｀・ω・´) `alice` and `codex/overnight-task2`: GOAL M2, brief T0; `7204550` also scopes T3 evidence.
`codex/brief3-task1`: GOAL M2, brief T1.
`codex/brief3-task3`: GOAL M2, brief T3.
`codex/brief3-task4`: GOAL M3, brief T4; new `branch_loop.rs` and `literal_fallback.rs`.
`codex/brief3-task5`: GOAL M2, brief T5; all new exception-ladder source, build, manifest, verifier, and test files.
New Task 0 correlation tests, fixtures, provenance, ledger, and attempted-correlations files: GOAL M2, brief T0.
Drift: `codex/brief3-audit-r3`; TinyDraw `blogpost`, `scripts/blog-dist`, and `this-is-a-story-of-project-management-v7.md`.
Drift: both recently modified archive brief files, which have no brief task number.
Price table: no R2 sequence totals and no R3 hardcoded configuration constants found.
```

```text
(｀・ω・´) `alice`, `codex/brief3-task1`, and `codex/overnight-task2`: GOAL M2, brief Tasks 0 and 1.
`codex/brief3-audit-r3`: GOAL M2, brief Task 3 receipt-scope correction.
`codex/brief3-task3`: GOAL M2, brief Task 3; no new files.
`codex/brief3-task4`: GOAL M3, brief Task 4; new `branch_loop.rs` and `literal_fallback.rs`.
`codex/brief3-task5`: GOAL M2, brief Task 5; all new exception-ladder source, manifest, verifier, and build files.
Other `codex/*` branches: prior brief GOAL M2 calibration and evidence tasks; `main` is the upstream mirror.
Archive opcode-ladder, register-block, and DMA captures: GOAL M2, brief Tasks 0, 2, and 3; brief files orchestrate M2/M3 Tasks 0 through 5.
Drift: TinyDraw `blogpost`, untracked `scripts/blog-dist`, and `this-is-a-story-of-project-management-v7.md`.
Price table: no R2 sequence totals and no R3 hardcoded configuration constants found.
```

```text
( •̀ᴗ•́ ) `alice` @ da5e01b: GOAL M2, brief T0, T1, T3.
`codex/brief3-task2` @ da5e01b: GOAL M2, brief T2; clean starting branch.
`codex/brief3-task4` @ 7ed4ff5: GOAL M3, brief T4; new `branch_loop.rs` and `literal_fallback.rs`.
`codex/brief3-task5` @ 0ae3148: GOAL M2, brief T5; new verifier test and untracked `PARKED.md`.
Merged brief refs `audit-r3`, `task1`, `task3`, and `overnight-task2`: GOAL M2, T0, T1, T3.
Archive opcode captures: M2/T0 and M3/T4; register-block captures: M2/T2; DMA capture: M2/T3.
Both 2026-09-02 brief files are orchestration inputs for M2/T0,1,2,3,5 and M3/T4.
Drift: TinyDraw `blogpost`, `scripts/blog-dist`, `this-is-a-story-of-project-management-v7.md`; Task 5 ignored `__pycache__`.
R2 violations: none; sequence totals remain in correlation targets.
R3 violations: none; SPI2 additive pricing is scoped to quad 40 MHz and 32 KiB.
```

```text
( •̀ᴗ•́ ) `alice` @ 7ed4ff5: GOAL M2 Tasks 0,1,3 and M3 Task 4.
`codex/brief3-task2` @ 591ca01: GOAL M2, brief Task 2; sole active worktree.
`codex/brief3-task5` @ a3df529: GOAL M2, brief Task 5; parked branch, unmerged.
Merged brief refs Task 0, audit R3, Task 1, Task 3, and Task 4 retain their assigned M2/M3 task numbers.
New fixtures, correlation tests, provenance, and ledger files on `alice`: GOAL M2, Task 0.
New `cli/tests/measured_boot.rs` and refusal JSON: GOAL M2, Task 2.
New wasm branch/loop and fallback tests: GOAL M3, Task 4; exception-ladder PARKED/test files: GOAL M2, Task 5.
Archive opcode, register-block, and DMA captures advance M2 Tasks 0,2,3 and M3 Task 4; both briefs are orchestration inputs.
Drift: TinyDraw `blogpost`, untracked `scripts/blog-dist`, and `this-is-a-story-of-project-management-v7.md`.
Price table: no R2 sequence totals and no R3 hardcoded configuration constants found.
```

```text
(•̀ᴗ•́) `alice` @ 591ca01: GOAL M2, brief Tasks 0, 1, 2, 3, 5; GOAL M3, Task 4.
`codex/brief3-status-proposal` @ 2c58623: GOAL M2, Task 5 proposal.
`codex/brief3-task1`, `task2`, `task3`, `task5`, `overnight-task2`, and `brief3-audit-r3`: GOAL M2, Tasks 1, 2, 3, 5, 0, and 3.
`codex/brief3-task4`: GOAL M3, Task 4.
New correlation fixtures/tests: GOAL M2, Tasks 0 and 2.
New wasm JIT tests: GOAL M3, Task 4; new exception-ladder parked/test files: GOAL M2, Task 5.
Drift: TinyDraw `blogpost`, `scripts/blog-dist`, and `this-is-a-story-of-project-management-v7.md`.
Drift: both modified archive brief files lack a single brief task number.
Price table: no R2 sequence totals and no R3 hardcoded configuration constants found.
```
