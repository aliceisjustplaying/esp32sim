# ESP32-S3 mask-ROM instruction-fetch ladder

Status: design under independent challenge (astra, first pass 2026-09-06) before any
hardware capture. No price is adopted by this document.

What this ladder can and cannot establish: each pair measures the excess of a
complete windowed call into ROM over the same call into IRAM. Changing the call
target from IRAM to ROM may change the call and return pipeline effects as well
as the body fetches, so `D` is a matched whole-call ROM excess, not a
per-instruction fetch cost, and not the reset-vector fetch cost. Even `D = 0`
for every pair supports only "windowed-call bodies in ROM cost the same as in
IRAM". Boot pricing stays blocked until a second, non-window-call ladder
(straight-line ROM code reached without `entry`/`retw`) validates the same
model independently.

## Why

Measured boot stops at cycle 0 on `_ResetVector` because no receipt prices an
instruction fetched from the mask ROM. The two earlier attempts could not isolate
that cost: the H1 `mask_rom_fetch_straight_line` cell timed
`callx8; entry; retw.n` into ROM with no matched IRAM control, so the ROM excess
stayed behind an interval-priced call sequence (`docs/evidence/timing/h1-exception-ladders-2026-09-04/README.md`
on `codex/h1-hardware-batch-prep`), and the memset-derived candidates were
negative and non-integer (`derived-rom-fetch-idf61`).

## Design

Ten straight-line mask-ROM functions are timed through one driver, each twice:

- `rom_<name>`: the function at its ROM address.
- `iram_<name>`: a byte-identical copy assembled into IRAM at build time
  (`main/rom_twins.S`, generated from `rom-cells.json`), placed at the same
  address modulo 128 (hence modulo 64 and 32).

Both cells of a pair execute the same driver instructions, the same `callx8`,
the same register-window state, the same operands (`a2 = 0x123`, `a3 = 0x10`, both `movi` immediates so the timed region holds no load),
the same instruction bytes and the same alignment. Interrupts are masked at
level 15 around the timed call; a sample is accepted only if every instruction
and data cache counter delta is zero. Neither ROM nor IRAM fetches go through
the caches, so the counters guard against stray flash or PSRAM traffic only.

The per-pair difference `D = rom - iram` (all 100 samples per cell must agree,
in two independent boots) is the matched whole-call difference for that body:
the candidate ROM excess, scoped as stated at the top.

### Cells

Bodies were selected from the pinned ROM ELF
(`esp32s3_rev0_rom.elf`, SHA-256 `c0ce0f33...`) as every function of the form
`entry ... retw.n` whose body contains no branch, jump, loop, load, store,
call, special-register access, sync or division (division would trap on a
zero `a3`). `words` is the number of 32-bit-aligned words the body spans.

| cell | ROM address | bytes | instructions | words | addr mod 64 | body |
|---|---|---:|---:|---:|---:|---|
| p_none | 0x400559a4 | 5 | 2 | 2 | 36 | entry a1,16; retw.n |
| abs | 0x4002e314 | 8 | 3 | 2 | 20 | entry a1,32; abs a2,a2; retw.n |
| mulsi3 | 0x40056080 | 8 | 3 | 2 | 0 | entry a1,16; mull a2,a2,a3; retw.n |
| clzsi2 | 0x400560a8 | 8 | 3 | 2 | 40 | entry a1,16; nsau a2,a2; retw.n |
| temp_to_power | 0x40055c70 | 11 | 4 | 3 | 48 | entry a1,32; sub a2,a2,a3; extui a2,a2,2,8; retw.n |
| roundup2 | 0x40056064 | 15 | 6 | 4 | 36 | entry a1,32; addi.n a2,a2,-1; add.n a2,a2,a3; neg a3,a3; and a2,a2,a3; retw.n |
| ctzsi2 | 0x400560b0 | 20 | 7 | 5 | 48 | entry a1,16; neg a3,a2; and a3,a3,a2; nsau a2,a3; neg a2,a2; addi a2,a2,31; retw.n |
| ffssi2 | 0x400560c4 | 20 | 7 | 5 | 4 | entry a1,16; neg a3,a2; and a3,a3,a2; nsau a2,a3; neg a2,a2; addi a2,a2,32; retw.n |
| clzdi2 | 0x4005636c | 20 | 8 | 5 | 44 | entry a1,32; movnez a2,a3,a3; movi.n a9,0; movi.n a8,32; movnez a8,a9,a3; nsau a2,a2; add.n a2,a8,a2; retw.n |
| ctzdi2 | 0x40056380 | 30 | 11 | 8 | 0 | entry a1,32; movnez a3,a2,a2; movi.n a9,0; movi.n a8,32; movnez a8,a9,a2; neg a2,a3; and a2,a2,a3; nsau a2,a2; sub a2,a8,a2; addi a2,a2,31; retw.n |

Exact encodings per instruction are in `rom-cells.json`, generated from the
pinned ROM disassembly; `verify_elf.py` re-derives them from the ROM ELF and
fails on any difference.

### Equations

For pair k, with identical driver cost C and identical body issue cost B_k on
both sides:

    rom_k  = C + B_k + F_k
    iram_k = C + B_k
    D_k    = rom_k - iram_k = F_k

F_k is the matched whole-call ROM difference. Candidate models, tested in this
order and only accepted as candidates when every D_k is reproduced exactly
with integer parameters in both boots:

1. F_k = 0 for all k (windowed-call bodies cost the same in ROM as in IRAM).
   A candidate only; no boot price is adopted from it.
2. F_k = f * words_k (a per-fetch-word cost).
3. F_k = f0 + f * words_k (first-fetch cost plus per-word cost).
4. F_k = f * instructions_k, and the same with an offset.

Anything else is recorded as interval or unexplained. No model from this
ladder is adopted as a price; see the scope statement at the top. The
ladder has ten pairs over five distinct word counts {2, 3, 4, 5, 8} and seven
distinct alignments modulo 64 {0, 4, 20, 36, 40, 44, 48}.

### Held-out validation (declared before capture, per rule R8)

Models are fitted only on boot 1 of the six fitting pairs: p_none, abs, mulsi3,
temp_to_power, ctzsi2, clzdi2. The four held-out pairs (clzsi2, roundup2,
ffssi2, ctzdi2) and all of boot 2 are validation only: a fitted model must
reproduce every held-out `D` exactly. Fitting on all ten and reproducing the
same ten is not validation.

### Scope

A validated model describes matched windowed-call bodies in ROM relative to
IRAM. It does not price instruction fetch in general, does not price the
reset-vector path, and says nothing about taken branches, loops, `l32r`
literal loads from ROM, or calls whose target is in ROM. Boot pricing stays
blocked until a non-window-call ladder validates independently; each of those
patterns needs its own matched-control cell.

## Disproof checklist for the independent reviewer

1. The ten instruction lists above match the ROM ELF byte for byte, and each
   IRAM twin has the same bytes at the same address modulo 128 in IRAM
   (`verify_elf.py` checks both from the ELFs).
2. The driver is one function, `measure_probe_samples`. Between the two
   `rsr.ccount` the verifier allows only operand moves and one `callx8`: no
   load, store, branch or volatile sink traffic (the result is accumulated
   after the second read). Both cells of a pair call it with the same operands;
   only the function pointer differs. The verification receipt lists the exact
   timed-region encodings.
3. The firmware re-checks each twin against ROM word for word at run time
   (reads only) and prints `ROM_TWIN` lines with both addresses and the words;
   any mismatch prints `CALIBRATION_FAILED`, which the validator rejects.
4. Twin address modulo 128 equals ROM address modulo 128 (`ROM_TWIN` lines).
5. Nothing executed in a body can trap on the chosen operands.
6. Cache counters are required zero for every accepted sample. They do not
   exclude IRAM window-overflow handlers. Assumption, not proof: the
   register-window state at the `callx8` is the same for both cells of a pair
   (same caller chain `app_main -> run_cell -> measure_probe_samples`, same
   depth), so the paired comparison is made at equal entry state; a handler, if
   any, is part of both sides. Cells whose samples vary are rejected as
   distributions rather than explained.
7. Cohort: CPU 240 MHz, main task pinned to core 0
   (`CONFIG_ESP_MAIN_TASK_AFFINITY_CPU0`), interrupts masked at level 15 around
   the timed call, core 1 idle in the IDF idle task, no Wi-Fi, no PSRAM traffic
   during cells.
8. Equal ROM and IRAM values support model 1 as a candidate for windowed-call
   bodies only; a non-integer or alignment-dependent residual under models 2 to
   4 is a refusal, not a fit.

## Build, verify, dry-run, capture

```sh
eim run "idf.py -C calibration/esp32s3-rom-fetch-ladder -B out/rom-fetch-ladder build" v6.1
eim run "python3 calibration/esp32s3-rom-fetch-ladder/verify_elf.py out/rom-fetch-ladder/esp32s3_rom_fetch_ladder_calibration.elf out/rom-fetch-ladder/elf-verification.json --objdump \$(command -v xtensa-esp32s3-elf-objdump)" v6.1
calibration/tools/dry-run.sh calibration/esp32s3-rom-fetch-ladder out/rom-fetch-ladder
calibration/tools/capture.py --image calibration/esp32s3-rom-fetch-ladder --build out/rom-fetch-ladder --boots 2 --port <serial>
```

The dry run executes the image in the emulator with the real ROM; its cycle
values are the emulator's instruction-count clock and carry no timing claim.
Hardware capture flashes the board (authorized by the owner on 2026-09-06,
contents of flash not preserved) and records two boots.
