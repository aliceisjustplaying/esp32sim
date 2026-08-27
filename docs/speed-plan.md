# Plan: making the emulator faster

Every number here was measured in this repo with `tools/bench.py` (interleaved rounds, best +
median wall time, guest instruction counts cross-checked) or `sample(1)` against a normal run.
The negative results are listed too, so nobody re-spends the time.

**Status:** Phase 1 (the basic-block interpreter) landed; Phase 0's NEON work and the JIT are open.

## Where we are (M-series Mac, `lto = "fat"`, `tools/bench.py`)

| workload | before blocks (`288a91e`) | with blocks | vs real time now |
| --- | --- | --- | --- |
| energy panel (LVGL, mostly idle) | 77 Minsn/s | **104** (1.34×) | ~2.9× |
| panel + WiFi + HTTPS (20 s) | 9.1 s wall | **8.6 s** | 2.3× |
| SID player (panel, tune playing) | 93 Minsn/s | **133** (1.42×) | 1.5× |
| autopling detector (PIE-heavy) | 63 Minsn/s | **78** (1.24×) | below |
| Atech synth (5 s scenario) | 113 Minsn/s | **153** | — |
| Atech ST7735 full redraw | needs ~240 Minsn/s | | still short |

Profile of the SID workload with blocks (shares of total run time):

| ~44 % | `exec_insn`: operand unpack + 245-way dispatch + semantics |
| ~19 % | loads/stores (through the software TLB) |
| ~14 % | block loop: window-overflow check, pc compare, entry copy, per-block accounting |
| ~0 %  | fetch, decode, cache validation — gone |
| ~7 %  | device time, IRQ derivation, DMA |

Before blocks the per-instruction scaffolding was ~35 % and **no single piece of it was
removable** — each ablated to ≈0 %. Executing blocks reclaimed it; what remains is the work of
the instructions themselves, which only code generation reduces further.

## Phase 0 — small, independent, do anytime

- **NEON for PIE** (`pie.rs`): every `ee.*` op runs as a scalar loop over `u128` lanes with
  shift/mask extraction and per-lane saturation; the detector spends ~20 % of its time there.
  `core::arch::aarch64` intrinsics behind a scalar fallback (the fallback stays the reference
  and the unit-test oracle). Expected +10–15 % on the detector only. ~2–3 days.
- **Shrink `CacheEntry`** (32 B × 64 K = 2 MiB copied by value on every hit). Measured: size
  32 K/64 K/128 K makes no throughput difference, so halving the table is free memory; packing
  `Insn` below 20 B may help cache locality but must be measured, not assumed.
- **Nothing else at this level.** Already measured as free or rejected: `ccompare` loop,
  per-instruction debug hooks (now bloom-gated), `target-cpu=native`, bigger icache, raw
  pointers in TLB entries, 2048-cycle idle steps (changed the Atech WAV — the regression bar
  is bit-identical output).

## Phase 1 — basic-block interpreter — DONE

**Measured: 1.24–1.42× across the workloads, every regression output bit-identical**, landed
in `xtensa-lx7/src/block.rs`. It is host-API-free like the rest of the core, so it carries over
to the WASM build unchanged. Two things the implementation taught that the design below did not
anticipate: IRAM/DRAM aliasing put `.dram0.data` in the same 4 KiB version page as the hottest
ISR code (version pages are now 256 bytes), and a block cut by the scheduling quantum has to
resume in place rather than start a new block at the cut, or the cache fills with fragments.

Design as built:

- **Block = straight-line run of decoded instructions** ending at a control transfer (`j`,
  `jx`, `call*`, `ret*`, branches, `loop*`), a `waiti`/`rsil`/`syscall`-class instruction, or
  a block-size cap (~32). Stored as a pre-resolved array: handler + unpacked operands per
  instruction (no re-decode, no operand extraction at run time).
- **Once per block instead of once per instruction**: the interrupt check, the decode-cache
  probe, `ccount` advance (add `block.len`), `insn_count`, and the stub/probe bloom test.
  The window-overflow check stays per-instruction where a `max_ar` demands it — it is the one
  check that measurably matters (removing it broke the guest, ablation B).
- **Timer precision is preserved** by bounding a block's length to the cycles remaining until
  the next `cycles_until_timer()` deadline — same trick the lazy device tick already uses, so
  `ccompare`/systimer alarms still land on the exact instruction.
- **Invalidation is already built**: a block caches the `page_ver` values of the (at most two)
  256-byte pages it spans; validation is two indexed loads. MMU remaps bump all flash/PSRAM
  versions. Self-modifying code, the SPI flash
  controller and the image loaders all bump versions today (`note_written`), and MMU changes
  invalidate the TLB. Zero-overhead loops (`lbeg`/`lend`/`lcount`) fall out naturally: the
  loop body is a block, the loop-back edge re-enters it.
- **Interpreter first, no codegen.** The block cache, discovery, guards and invalidation are
  exactly the infrastructure a later JIT reuses; only the "execute" half differs.
- Acceptance: Atech WAV bit-identical, SID capture sample-identical after alignment, full
  regression sweep, and `tools/bench.py` on the three standard workloads.

Landed at SID 133 Minsn/s, detector 78, panel 104 — the estimate held for SID and the panel;
the detector gained less because its time is in PIE lanes and loads/stores, not scaffolding.

## Phase 2 — JIT (next; probably WASM-first)

**Goal: 3–4× over today.** One block IR, two backends:

- **wasm backend**: emit a wasm module per batch of hot blocks. Measured with a spike:
  compile+instantiate costs **~0.3 µs per block** when batched (64+ blocks/module) and
  **2.5 ns per call** into a generated function under V8 — so translation pays for itself
  within a handful of executions. Generated code is straight-line ALU + word memory, the
  shape where wasm runs at 52–68 % of native; `return_call` (shipped tail calls) gives the
  dispatch chain Rust cannot express, and SIMD128 maps onto the PIE lanes.
- **aarch64 backend**: a few hundred lines of direct emission (no Cranelift — a large
  dependency, and useless for the wasm side). Native SID is already at real time, which is
  why the browser backend goes first: at ~0.45× real time today it is where a JIT actually
  changes what is possible.
- New floor after a JIT: device ticks, bounds-checked memory, and the window-overflow check.

Effort: ~a month total, roughly half per backend once the IR exists.

## Phase 3 — perception, not emulation

The browser session runs ~0.82× real time while headless runs 0.99×, because 460 KB frames are
pushed 50×/s whether or not anything changed. Push at 25 Hz or only dirty rows (the RGB engine
can track lines). Half a day, and it is the cheapest "the SID page stopped stuttering" per
hour spent of anything on this list.

## Rejected, with the numbers

- **One host thread per core**: core 1 is 7–8 % of executed instructions on the workloads that
  matter, so the ceiling is ~1.1× — and it forfeits deterministic, bit-identical output, which
  is the regression bar. Not worth it.
- **Skipping guest busy-loops by pattern**: the mbedTLS accelerator polls are real firmware
  behaviour; a faster interpreter runs them faster, and special-casing them risks the
  plausible-wrong-answer failures the crypto section of decisions.md documents.
- Everything in the Phase 0 "already measured" list above.

## Method rules (learned the expensive way)

1. Benchmark **interleaved** (`tools/bench.py`), never A-then-B — background load drifts ~10 %
   over minutes and sequential comparisons harvested it as fake wins twice this session.
2. `--profile` reports **guest** PCs and disables idle-skipping; for emulator-side cost use
   `sample <pid>` and confirm with an ablation build.
3. `pgrep esp32sim` before benchmarking — leftover runs at 100 % CPU look like regressions.
4. The bar for landing anything: Atech WAV and TFT bit-identical, panel PNG identical,
   decoder-vs-objdump, hello_world, autopling, WPA2 join, HTTPS fetch, unit tests.
