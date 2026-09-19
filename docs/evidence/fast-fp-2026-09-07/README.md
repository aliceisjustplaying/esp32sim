# FP arithmetic dependency latency on ESP32-S3 revision 0.2

Two boots give the same decisive result: **dependent `add.s`, `mul.s` and
`madd.s` chains take approximately four cycles per operation; four independent
accumulators sustain approximately one operation per cycle.** All result bit
patterns match their finite expected values. [Both boot summaries](summary.json)
retain nine samples per cell, raw cycle counts and baseline subtraction.

| Operation | Dependent cycles/op, boot 1 | Four independent cycles/op, boot 1 |
| --- | ---: | ---: |
| `add.s` | 4.000256 | 1.000408 |
| `mul.s` | 4.000256 | 1.000328 |
| `madd.s` | 4.000057 | 1.000114 |

A useful next simulator experiment is **one-cycle FP issue with arithmetic
results ready four cycles after issue**, stalling only consumers. This is an
inference from these chains, not evidence for charging every FP instruction
four cycles or for assigning all unmeasured FP opcodes the same latency.

The small probe derives its IDF setup and warmed IRAM call experiment from
TinyDraw's existing core-timing calibration. Each sample calls a block 1024
times, each nonempty block has exactly 256 FP instructions and interrupts are
masked during timing. The blocks initialize all operands to finite 1.0 values
on each call, so the largest arithmetic result is 257.0. A shared empty block
removes call/initialization overhead; terminal result-read effects remain
small per-block overhead. The `lsi_add_gap` cell additionally executes 128 NOPs.

The conversion/load pair cells are diagnostic only: their independent
consumer still has a two-instruction recurrence, which is confounded by the
four-cycle arithmetic latency. Do not infer isolated conversion or load-use
latency from those pairs.

Evidence: [raw boot 1](boot-1.log), [raw boot 2](boot-2.log),
[actual assembly](blocks-disassembly.txt), [checked instruction counts](instruction-counts.json),
[IRAM symbol addresses](symbols.txt), [image metadata](image-info.txt)
and [ELF/image hashes](sha256.txt). CPU is 240 MHz, IDF v6.0.2, silicon revision 0.2.

Probe source is [fp-probe](../../../tools/fast-hardware/fp-probe/main/fp_timing.c).
The original full flash backup remains untouched. Only bootloader, partition
table and app image ranges were written; no full erase occurred.
