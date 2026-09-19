# Overnight experiments, September 19, 2026

Two sessions worked through the night on the TinyDraw browser battery: a coordinator (Claude) that owned benchmarking, the board and this log, and a peer (GPT-6 Astra, launched separately by the user) that owned isolated patches, differential tests and code review. No subagents were spawned, no pull request was opened. Branches are on the `fork` remote (`aliceisjustplaying/esp32sim`). A `git push` also carried upstream's existing `v0.2.0` tag to the fork.

## Results

The reference column gives the revision or configuration each figure comes from; rows marked historical are earlier receipts, not runs of main d5446b4a made tonight.

| | Reference | This night | Where |
| --- | --- | --- | --- |
| TinyDraw battery, exact instruction-count clock | main d5446b4a: 83.6 s wall, 0.57× | **42.5 s wall, 1.12×**; three alternating pairs on 1b28b34e and three on the previous head 75382bcc; identical 9,819,885,134 instructions and console hash | `night/combined-0919` at 1b28b34e, default build, pushed |
| pocket-tank, 30 guest s (both cores busy) | main d5446b4a: 80.9 s, 0.37× | 65.4 s, 0.46×, exact 10,073,833,775 (one pair) | same |
| Atech synth, SID jukebox, LCD-4B panel goldens | committed hashes | identical WAV hashes, consoles and per-core instruction totals with virtual quanta on (native, `--no-jit`, one run each) | `night/fast-entry-0919` |
| Production page, boot to READY | historical: 120.5 s (Sep 5 receipt, 8df2f0ad) | 54.7–56.5 s; strokes correct, movement→canvas median 36–38 ms (single runs) | `night/fast-entry-0919`, `night/timed-0919` |
| Approximate-timing model, EX066 configuration | historical: 0.478× (Sep 7, d13b7f93) | 1.05× with measurement-informed instruction, alignment, data-cache and fetch-cache prices; 1.14× without the fetch cache (single untimed runs) | `night/timed-0919` |
| Timed model vs an erased-start board, 15 paced cold tests | EX066 configuration on tonight's head: compute 0.749, wall 0.787, document load 0.47 | medians compute 0.983 (all 15 cases 0.977–0.989), HARD 1.004, wall 1.013, export 0.992, document load 0.933, present 1.072; staging microbenchmarks 0.93–1.04; whole battery 73.8 guest s vs 77.4 s (single run) | same |
| pocket-tank under the timed model | instruction clock: 24.3 tok/s, 62.5 fps | 7.4–8.0 tok/s, 32–33 fps with the final export set (9.5 and 35 before the fetch cache and the packed-PIE pricing fix); `docs/speed-plan.md` gives the board as 12 and 25–30. The model uses TinyDraw's cache geometry and one price for PSRAM and flash lines, so inference is now priced too high | same |
| Production page under the timed model (`?timing=hw`) | strokes dead (board-swap bug, found tonight) | boots to READY in 79–88 s wall (88 s with the fetch cache on), strokes correct, 39–43 ms median. The board needs 77.4 s from its startup serial line to the verdict; the endpoints differ | same |

What made the difference, in order of size:

1. **EX133 virtual quanta** (new). While the other core idles, core 0 runs many 64-instruction quanta in one budget. The run is bounded so that nothing can fall due inside it, a device-register access stops in front of its instruction, and the spanned rounds are closed exactly as before, so results are bit-identical. EX133 alone (K≤4) took 85.3 s to 70.5 s; with **EX134** in its first, unsafe constant form 82.2 s to 53.9 s. The guarded form of EX134 (tick deferral raised only while no cadence-driven device is active) costs about 1.4 s of that and is the one that is kept.
2. **EX137 larger regions.** The widening part of EX043, isolated, is worth 15% under virtual quanta. EX043 itself was a flat multi-change bundle, so longer budgets are the plausible enabler, not a proved sole cause.
3. **Dispatch diet**: EX038 boxed cache, EX065 negative coverage cache, EX106 direct cut index (5–10%), plus EX135 and EX136 (2–3% each).
4. **EX139 + EX138/EX140/EX141/EX146** for the timed model: solo batches that yield in front of device registers, then measured prices folded into generated code as constants. More cycles per instruction means less guest work per simulated second, so the priced model still runs at realtime; that ratio is under a different clock and is not an equal-work speedup.

What did not work: EX109 guarded RETW (inside noise), EX121 wasm-opt (about 1%), EX043 last-mapping TLB reuse isolated (4.2% slower), EX134 as a plain constant (breaks pocket-tank's pinned total), my first derived call/return prices (overcharged by 4.5 cycles per call; replaced by EX141's measured ones).

## What to trust, and how far

- The exact-clock numbers rest on the harness's own contract: equal instruction totals, equal console SHA-256, 36/36 firmware checks, zero JIT failures, alternating pairs. combined2, combined3 and combined4 each have three pairs, the mapping-cache screen two; the other exact-clock rows are single screening pairs, and the timed-model rows are single unpaired diagnostic runs. Each entry in the log states its own count.
- The peer ran the wasm differential suite (43,261 cases on combined4), selected native suites (Xtensa semantics, S3, shared SoC) and the CI clippy command on the integrated commits. The coordinator ran `cargo test --release --workspace` on `night/fast-entry-0919` and `night/timed-0919` and both differential configurations on the merged timed head.
- The timed model is approximate by design: a measurement-informed model with remaining assumptions. Its instruction prices come from earlier hardware ladders (EX067, EX068, EX079, EX080) and from EX081's control cells, captured on September 7 and analysed only tonight; its cache prices from window totals of tonight's Tier-B cohort, which are not isolated miss penalties. Assumed: CALLX = JX, SUB.S/MSUB.S = ADD.S, one-cycle latency for unmeasured FP/PIE writers, round-robin replacement, explicit-`msync` cost for automatic eviction, one fill price for PSRAM and flash. The fit is to one firmware on one board revision.
- Hardware: the board was fully erased (authorized), ran the frozen gate-1 images for the two reference captures, then the Tier-B calibration images. It was not restored; the last image flashed is the Tier-B XIP-PSRAM one.

## Open items, most valuable first

1. Review and upstream `night/combined-0919` in pieces: EX133 + EX134 guard, the diet, EX135, EX136/137, EX144. EX133's exactness rests on several invariants together (the run-length bounds on device flush, script, page push, peer wake-up and limits; deferral of device registers; the cadence guard) and on the tested contract (pinned totals, console hashes, goldens). `vq_violations` is only a backstop for register accesses that escape deferral, not a correctness certificate. It deserves a reviewer who did not write it.
2. Timed model: presentation is 7% slow with the fetch cache on and the fetch-line price (404, a window with two misses in its raw records) is provisional; ring-PIE staging 0.93; pocket-tank's inference is 35% slow and neither its 64 KB data cache nor a separate flash price fixes that (sequential data-cache autoload was checked and is off in both firmwares, so that gap is unexplained); the cache geometry should come from the EXTMEM registers; CALLX assumed; 96/160 are window totals, not isolated penalties. The Tier-B cohort captured tonight (`~/Archives/esp32s3/tier-b/`) has the msync and SPI2 decomposition cells still to analyse.
3. Calls and returns inside regions: 82% of region exits, about 4 s of the remaining 42 s; the peer's design note (`design-calls-in-regions.md`) puts the first step (direct CALL8 → ENTRY as an internal edge) at no more than 4%.
4. pocket-tank stays at 0.44×: both cores busy, so it needs raw throughput or the timed clock, not scheduling.

## Files

- `summaries/*.json`: run-pairs summaries and single-run results named in the log.
- `hardware/`: gzipped serial logs of both erased-start captures, compact summaries, firmware-timer ratio tables.
- `tools/`: the small wrappers used (`pair.sh`, `battery.sh`, `prof.sh`, `resp.sh`, `cmp-hw.py`, `hash.py`).
- Raw captures, wasm artifacts, CPU profiles and module dumps stay local in `work/night-run/` (1.4 GB, ignored).

# Working log (chronological)

## Contract

User authorizes autonomous experiments, device flashing without restoration and pushing useful branches to fork. No PRs. No spawned subagents. User independently launched a peer in herdr w2:p5; peer owns EX109 RETW work in a separate checkout. Coordinator owns Chrome timing, board access and catalog updates. Keep implementations small; screen before polishing.

## History consulted

Read all 132 entries of `codex/experiment-catalog:docs/experiments.md`, `work/catalog-upstream-handoff.md` and archived fast-round-closeout, fast-dispatch and browser-performance-round-handoff reports plus review-spikes RESULTS.

Correction to earlier advice: approximate timing and hardware gate captures already exist. EX066 reaches 0.478x with incomplete combined timing. EX078 has two 78.70-second hardware captures but stored drawing state differs from browser. EX130 uniform CPI2 fails timing gates. Do not repeat these under new names. EX038 already has a later three-pair comparison: 3.80% lower median, one negative pair (`work/ex038-run-stable/summary.json`).

## Setup

- Main/source baseline d5446b4a; origin fetched and matches.
- Experimental checkout `work/night`, branch `night/speed-0919`.
- Catalog checkout `work/night-catalog`, branch `codex/experiment-catalog`.
- Inputs `work/night-run/assets.json`; hashes match EX042 published receipt.
- Baseline release artifact `work/night-run/wasm/base.wasm`; cpu-profile sibling `base-cpuprof.wasm`.
- Board enumerates `/dev/cu.usbmodem101`.

## Completed: baseline A/A

Receipt `work/night-run/aa-0/summary.json`: 87.934745 / 86.865130 wall seconds. Both advance 47.750049575 guest seconds and execute 9,819,885,134 instructions. Both pass the battery with identical console SHA-256 f4e7e4f3be2f988b8b52d2797d373208b49e86e70b9c2b6618910a90c7e36291. Same-artifact spread is about 1.2%; do not promote small screening differences as wins.

## Running

Fresh cpu-profile capture interrupted by harness restart after an early progress event; no complete result. Relaunched into `work/night-run/prof-base-retry`. This diagnostic build is not throughput evidence.

Peer investigates EX109 guarded common RETW. Coordinator is inspecting scheduler and region overhead for materially different experiments. No source optimization implemented yet.

## Step 2: profile, counters, EX109 screens

- Fresh CPU profile of d5446b4a (`prof-base-retry/cpu-summary.txt`, diagnostic build, 102 s sampled): generated blocks 36.8%, `run_block_inner` 19.6%, `jit::run` wrapper 12.3%, machine run 4.9%, `step_blocks` 4.9%, `exec_insn` 3.5%, `flush_ticks` 2.3%, `ready` 1.5%. Roughly 45% of host time is per-dispatch overhead, not guest work.
- jit-profile counters (`jitprof-base/events.json`, 118 s instrumented run): about 741M dispatches (629M compiled at 14.3 insns each, 112M interpreted at 2.1). Core-0 regions: 320M calls retire 6.21G instructions (19.4 per call), 99.9% exit by leaving the region.
- Inference (not yet measured): with 19-instruction region calls against a 64-instruction quantum, about 30% of region calls meet a quantum cut, and each cut costs extra dispatches (partial block module, then the resumed checked path, which cannot enter a region because `entry != 0`).
- EX109 guarded RETW (peer, commit c831cf69, 43,643 differential cases pass): one pair 89.43 → 87.72 s, 1.9% less, equal work and console. EX109 + singleton RETW admission (a29cad87, 44,155 cases): 86.46 → 87.53 s, 1.2% more. Base-to-base spread tonight is 86.5–89.4 s, so both are inside noise. Status: weak / inconclusive. Receipts `runs/ex109-s1`, `runs/ex109single-s1`.
- Peer now porting EX038 + EX065 (+ EX106 direct resume offset) as a "dispatch diet" branch.
- Coordinator starting EX133 "virtual quanta": multi-quantum core-0 budget while core 1 is idle, bit-exact by construction (bounded by next device deadline, any peripheral access aborts before the instruction and falls back to the legacy quantum). Differs from EX047 (q1024), which changed the instruction total.

## Step 3: first real wins

### Dispatch diet (peer; EX038 + EX065 + EX106), plain main base
- diet1 = EX038 boxed cache + EX065 negative region-coverage cache, commit 174ac1ee: 82.77 → 78.42 s (−5.3%), one pair.
- diet2 = diet1 + EX106 direct CUT resume index, commit 4b508e83: 86.55 → 77.36 s (−10.6%), one pair. Equal instructions and console hash in all arms. 42,107 differential cases pass. Receipts `runs/diet2-s1`, `runs/diet1-s1`. Needs 3-pair confirmation.
- diet3 (7409dc76: lazy resume lookup, shared helper table, cold-split ready) and EX121 wasm-opt artifacts are built, unscreened.

### EX133 virtual quanta (coordinator; new idea, commit 2f… on night/speed-0919)
While every other core idles, core 0 runs up to K scheduling quanta in one budget. K is bounded so that no device flush, script event, page push, peer timer wake-up, cycle limit or instruction limit falls due at an interior boundary. A device-register word access stops in front of its instruction (bus `defer_access`), the spanned rounds are closed exactly as the loop would have closed them, and the current quantum finishes the old way. 64-instruction semantics are preserved bit for bit; only the needless cuts go away.
- Untimed correctness runs, all with 9,819,885,134 instructions, 47.750049575 guest s, 36 checks, console f4e7e4f3be2f988b:
  - K≤4 (MAX_TICK_DEFER 256 untouched): 76.3 s wall (peer was compiling during the run).
  - K≤64 with MAX_TICK_DEFER 4096 (EX134): 56.2 s wall → 0.85×.
  - K≤1000 with MAX_TICK_DEFER 65536: 54.2 s wall → 0.88×.
- EX134 caveat: the 256-cycle fallback exists for devices without an explicit deadline (I2S/LCD_CAM/camera DMA steppers, WiFi air). TinyDraw output is unchanged, but EX005 shows cadence changes can alter the Atech WAV. Needs an "only when no cadence-driven device is active" rule before it can be proposed upstream.

## Step 4: timed screens of EX133/EX134

| Arm (one pair each, vs d5446b4a base in the same pair) | base s | cand s | change | realtime |
| --- | ---: | ---: | ---: | ---: |
| EX133 K≤4, tick deferral untouched (`runs/vq4-s1`) | 85.26 | 70.47 | −17.3% | 0.68× |
| EX133 K≤1000 + EX134 MAX_TICK_DEFER 65536 (`runs/vq1000-s1`) | 82.18 | 53.89 | −34.4% | 0.886× |

All arms: 9,819,885,134 instructions, console f4e7e4f3…, 36/36. A STRICT build (panics on any device-register access that escaped deferral) completed the whole battery: no PIE/MAC16 path reaches a register in this workload (`check-vq1000-strict`).

Review findings from the peer, all accepted:
- PIE/MAC16 word accesses are not decoded by `word_access`; bus-level backstop counts them (`vq_violations`), and L32r was added. Production claim must stay "exact unless the counter is non-zero".
- C3/C6 buses do not honour deferral: `SocBus::can_defer` gates EX133 to the S3 (commit 4e5b4519).
- EX134 as a constant is NOT generally safe. RMT, RTC watchdog and USB-Serial/JTAG SOF have no reported deadline; USB's accumulator subtracts one 60000-cycle period per tick, so a 65536-cycle batch would drift. TinyDraw equality does not prove other firmware. Peer is implementing EX134-safe: 256-cycle cap whenever any cadence-driven device is active, otherwise 32768.

## Step 5: combined stack: 0.92–0.95× realtime, bit-exact

Branch `night/combined-0919` (peer's worktree `work/night-combined`) = EX133 + EX134 knobs + dispatch diet (EX038, EX065, EX106, diet3).

| Arm (one pair each) | base s | cand s | change | realtime |
| --- | ---: | ---: | ---: | ---: |
| combined, K≤1000, MAX_TICK_DEFER 65536 constant (unsafe in general) | 84.26 | 50.32 | −40.3% | 0.949× |
| same + wasm-opt -O3 (EX121) | 83.66 | 49.87 | −40.4% | 0.957× |
| combined-safe, K≤4, deferral untouched | 83.44 | 65.49 | −21.5% | 0.729× |
| **combined-quiet** 5863a82d: K≤1000, EX134-safe (256 cap while any cadence-driven device is active, else 32768) | 82.68 | 51.71 | −37.5% | **0.923×** |

Every arm: 9,819,885,134 instructions, console f4e7e4f3…, 36/36, zero JIT failures. EX121 wasm-opt: about 1%, inside noise → not worth build plumbing.

EX133 round counters (`stats-vq1000`, diagnostic build): 1.67M multi-quantum runs cover 134.3M quanta (80 quanta ≈ 5,100 instructions per run); 1.48M runs end at a device-register access, 11K at waiti; 8.1M rounds have both cores busy and 1.5M only core 1 (legacy path). So 93% of busy quanta now run uncut. Core-0 dispatches fell from ~741M to ~437M; region calls now retire 32 instructions each (was 19).

Core 1's 613M instructions are FreeRTOS critical-section / spinlock code (`spinlock_acquire`, `vPortExitCritical`, ROM `_xtos_set_intlevel`), 3.5 instructions per dispatch. Both-busy rounds must keep the 64-instruction interleave (shared-memory compare-and-set), so they stay on the old path.

## Step 6: EX135, safety fixes, second workload

- **EX135 fewer forced block boundaries around special registers** (coordinator, commit 5942cd06 on `night/ex135-sr-blocks`; peer's integrated version f5fdd86a + loop-state fix): RSYNC/ESYNC/DSYNC no longer end a block and emit as no-ops; RSR/WSR of PS and INTENABLE no longer force a block start; the wasm backend emits RSR for registers whose `Cpu` field is exact mid-dispatch (PS, PRID, EXCSAVE, EPC, …); WSR/XSR/RSIL are terminal helpers. Motivation: core 1 is 28% of all dispatches for 6% of instructions, nearly all FreeRTOS critical sections from the touch sampler's I2C polling (`_xtos_set_intlevel` took 5 dispatches for 9 instructions). Bit-exact on the battery; compiled share 95.3% → 97.7%; one pair on top of EX133/134: 53.34 → 52.08 s (−2.4%, weak). Peer review found and fixed one real bug before the combined2 validation: a terminal WSR/XSR of LBEG/LEND/LCOUNT after a retained hardware-loop prefix breaks the retired-offset reconstruction. Such writes still compile; their blocks can no longer retain a loop prefix. 919 new directed cases, 43,257 total pass.
- **PIE/MAC16 deferral hole closed** (peer, 8e49f2f0): when armed, a PIE or MAC16 load form scans the 16 visible address registers against the device range (±128 bytes slack for `ld.qr/st.qr`), so no operand decoding; 231 new cases.
- **EX134 unsafe constant is wrong on pocket-tank, EX134-safe is right.** `runs/pt-ex135` (K≤1000, MAX_TICK_DEFER 65536 constant): 10,073,833,712 instructions instead of the pinned 10,073,833,775 → rejected by the harness. `runs/pt-combined2` (cadence guard, quiet cap 32768): exact total and console 9e8a66e4, 83.59 → 78.49 s (−6.1%). pocket-tank keeps both cores busy, so EX133 rarely applies there; the gain is the dispatch diet.
- Native check: `--no-jit` pocket-tank for 30 guest seconds gives identical pc/ccount/insns with ESP32SIM_VQ=1 and =1000 (K≤4 natively).

## Step 7: confirmation, new profile, past 1.0×

**combined2 (2f8093d8) confirmed with three alternating pairs** (`runs/combined2-x3`): base 83.95 / 84.92 / 83.18 s, candidate 49.75 / 50.20 / 51.07 s → median 83.95 → 50.20 s, **−40.2%, 0.951× modeled realtime**, every run 9,819,885,134 instructions, console f4e7e4f3…, 36/36.

Profile of combined2 (`prof-combined2`, 54 s sampled): generated blocks 37.1%, `jit::run` wrapper 15.2%, `run_block_inner` 14.4%, other 17.7%, `step_blocks` 6.0%, machine run 4.4%, `exec_insn` 3.7%. Counters (`stats-ex136`): 560M dispatches (core 0: 355M compiled at 26.5 instructions + 74M interpreted; core 1: 96M + 35M); 254M core-0 region calls retire 7.96G instructions, 98% of exits "left".

On top of combined2, branch `night/fast-entry-0919` (`work/night-fast`), one pair each, all bit-exact:

| Step | base s | cand s | change | realtime |
| --- | ---: | ---: | ---: | ---: |
| EX136 region-entry facts cached in the dispatched block (epoch-validated; no walk through owner block → region → 3 vectors) vs combined2 | 49.65 | 48.33 | −2.7% | 0.99× |
| EX137 region limits 8 chunks/64 insns/4 pages → 24/192/6, vs EX136 | 48.23 | 42.92 | −11.0% | **1.11×** |
| EX137b limits → 64/512/8, vs EX137 | 43.93 | 41.89 | −4.6% | **1.14×** |

EX137 is the widening part of EX043 retried in isolation under a materially different condition (virtual quanta). EX043 was a flat multi-change bundle, so its null result cannot be attributed to the quantum alone. Cumulative generated wasm grows 98 → 111 MB.

EX136 gave 2.7% in this screen, not the 10–15% I had guessed for the `jit::run` wrapper; it does not isolate every wrapper cost.

## Step 8: robustness of the fast-entry head (eaf23abc)

- `cargo test --release --workspace` on `night/fast-entry-0919`: every suite passes (goldens included), 0 failed.
- Production page (`resp-ex137b`, real worker + pacing, headless Chrome): boots the battery firmware to `READY` in 54.7 s wall (the 2026-09-05 receipt needed 120.5 s), replays 3 strokes, 3 commits, 24/24 movement points answered; movement → canvas median 37.6 ms, max 52.4 ms (2026-09-05: 44.8 ms median with one 492 ms outlier). Screenshot shows three correct strokes and the minimap.

## Step 9: the timed model on the fast head: 0.48× → 0.93×

Merged `codex/fast-integration` (EX066's approximate-timing stack: frontier scheduler, data-cache model 160/96 with contention, timed SPI2 completion, measured TE) into the fast head → branch `night/timed-0919` (`work/night-timed`, 16 conflict hunks, virtual quanta disabled in approximate mode). Same experiment exports as EX066's final run (`esp32sim_set_approximate_jit_timing 1 64`, frontiers, cache 160/96/2, contention, SPI2 timing, measured TE), supplied through a `tinydraw-timed` workload entry.

**EX139 solo batches in the frontier scheduler:** when every other core idles, the running core's batch may extend to the next device/script deadline instead of 64 instructions; the first dispatch of a batch runs undeferred (a register it reaches sees batch-start time, as any access did in the 64-instruction batches this replaces), later dispatches stop in front of a device register and yield, so those accesses happen at settled device time.

| Timed configuration (untimed single runs, all 36 checks pass, zero JIT failures) | guest s | wall s | realtime |
| --- | ---: | ---: | ---: |
| EX066 as archived (d13b7f93, 2026-09-07) | 60.158 | 125.79 | 0.478× |
| fast head, solo batching off (`check-timed-nosolo`) | 60.008 | 95.74 | 0.627× |
| fast head + EX139 (`check-timed-solo`) | 60.025 | 64.54 | **0.930×** |

Model fidelity is unchanged from EX066 (hardware needs 78.70 s for this battery per EX078, with the stored-drawing state caveat); what changed is that the timed configuration now costs about realtime instead of 2×. Instruction totals differ slightly between the three (10,039,507,230 / 10,038,971,428 / 10,045,820,831) because batch boundaries feed the penalty model; no exactness claim is made for approximate mode.

## Step 10: confirmations, exit histogram, matched hardware reference

**combined3 (75382bcc = combined2 + EX136 + EX137/137b) confirmed with three alternating pairs** (`runs/combined3-x3`): base 83.22 / 83.02 / 83.25 s, candidate 42.59 / 42.56 / 42.68 s → **−48.8%, 1.12× modeled realtime**, bit-exact in all six runs. Peer: 43,257 differential cases, native suites and CI clippy pass on the same commit.

pocket-tank on combined3 (`runs/pt-combined3`): exact (10,073,833,775, console 9e8a66e4), 80.99 → 68.84 s (−15.0%), i.e. 0.37× → 0.44× realtime. Both cores stay busy there, so EX133 cannot apply; this workload still needs raw throughput (PIE kernels) — it is the honest counter-example to "we are at realtime".

Region exit histogram (peer's jit-profile counters, `stats-combined3`), core 0: 133.8M region calls retire 8.38G instructions (62.6 per call). Exits by kind: RETW 51.7M, CALL 45.6M, CALLX 8.2M, outside edge 8.6M, slow memory 8.3M, budget 6.2M. Calls + returns are 82% of exits, but at ~38 ns per dispatch that is about 4 s of 42 s: a bounded (<10%) lever that needs windowed call/return emission inside regions. Parked.

**Hardware reference with matched start state (EX078 retry; board erased as authorized).** `esptool erase-flash`, frozen bootloader/ptable/app written and verified, captured with `tools/fast-hardware/capture-gate.py` twice from an erased chip: 36/36 gates, `TINYDRAW_AUTOSAVE_RESTORE generation=0` (the emulator's erased start; EX078 had restored 38 operations), startup-serial → verdict **77.392 s and 77.400 s**. Files `hw/erased-boot-{1,2}.*`, `hw/erased-summary-{1,2}.json`.

Firmware-timer ratios, emulator ÷ hardware (`bin/cmp-hw.py`, medians over the 15 paced cold tests unless noted):

| | compute_us | present_us | wall_us | HARD total_us (4) | load_us | export | ring PIE / scalar |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | --- |
| untimed fast head | 0.726 | 0.284 | 0.562 | 0.519 | 0.430 | 0.553 | 0.153 / 0.694 |
| timed model (EX066 config + EX139) | 0.749 | 0.900 | 0.787 | 0.820 | 0.472 | 0.784 | 1.015 / 1.165 |

Whole battery: hardware 77.4 s; timed model 60.0 guest s (0.78); untimed 47.75 guest s (0.62, an effective hardware CPI of 1.62 against the instruction clock). The ratios equal EX056's (0.749 / 0.899), so the stored-drawing mismatch was not what held them down. Largest remaining gaps: document load (0.47, FP-heavy → EX057 dependency latency) and compute (0.75).

## Step 11 EX043 mapping component: no gain. EX138 control-flow prices: accuracy 0.75 → 0.94 at 0.99× realtime

- **EX043 last-mapping TLB reuse, isolated** (peer, d6daa2cb on `night/mapping-cache-0919`, 43,621 cases pass): two pairs vs combined3, base 43.02 / 42.76 s, candidate 44.89 / 44.49 s → **4.2% slower**, exact work. Parked again, now with an isolated result under long virtual-quantum runs.
- **EX138 measured control-flow prices on the timed model** (`night/timed-0919`): on top of the EX066 configuration + EX139, every taken branch costs 3 cycles, J 3, JX 6, LOOP setup 5, QUO 4, REM 5 (EX068 ladders); CALLn priced as J, CALLXn/RET/RETW as JX and ENTRY as 3, which reproduces the measured 16-cycle `callx8/entry/add/retw.n` level but is not individually measured. Emitted at code-generation time as one add to `Cpu::timing_extra` on the taken path only; the interpreter and the helper charge the same table; the frontier scheduler spends the extras from the batch budget without ending the batch.

| Firmware timer ÷ hardware (erased-start board, boot 1) | timed (EX066 cfg + EX139) | + EX138 prices |
| --- | ---: | ---: |
| paced cold compute_us (median of 15) | 0.749 | **0.938** |
| paced cold present_us | 0.900 | 0.989 |
| paced cold wall_us | 0.787 | 0.947 |
| HARD total_us (4) | 0.820 | 0.938 |
| export elapsed_us | 0.784 | 0.911 |
| document load_us | 0.472 | 0.602 |
| panel staging linear PIE / scalar | 0.839 / 0.989 | 0.961 / 1.002 |
| panel staging ring PIE / scalar | 1.015 / 1.165 | 1.048 / 1.222 |
| whole battery guest seconds (hardware 77.4 s) | 60.0 (0.78) | **69.0 (0.89)** |
| host wall seconds → realtime | 64.5 → 0.93× | **69.9 → 0.987×** |

All 36 checks pass, zero JIT failures (`check-timed-priced`, full table `cmp-hw-priced.txt`). This is the thesis from the start of the night, now measured: pricing cycles moves the model toward the silicon *and* keeps realtime, because more cycles per instruction means less guest work per simulated second. (A higher guest-seconds-per-wall-second ratio under a different clock is not an equal-work host speedup; the exact-clock rows are the equal-work comparison.) Still open: load-use (+1, measured in EX067), FP dependency latency (EX057; explains the 0.60 document load), ring-scalar overshoot from the provisional cache parameters, L32R and instruction-fetch cache.

### EX138 load-use and EX140 static FP result readiness: timers within ~0.5% of the board (medians)

- Load-use (+1 cycle when an instruction reads the result of the load right before it; EX067) → compute 0.960, wall 0.961, HARD 0.960, export 0.972 (`check-timed-priced-lu`).
- **EX140 static FP readiness**: ADD.S/MUL.S/MADD.S results usable four cycles after a one-cycle issue, consumers stall (EX079, measured; SUB.S/MSUB.S assumed equal; all other FP writers one cycle). Computed once per decoded straight-line run together with load-use (`exec::static_extras`), so the emitter folds it into constants and the interpreter reads a byte per instruction. Dependencies across a block boundary are not charged.

| Firmware timer ÷ hardware (erased-start board, boot 1; boot 2 agrees to 3 decimals) | EX066 cfg + EX139 | + EX138 control | + load-use | + EX140 FP |
| --- | ---: | ---: | ---: | ---: |
| paced cold compute_us, median of 15 | 0.749 | 0.938 | 0.960 | **0.998** (min 0.937, max 1.013) |
| paced cold present_us | 0.900 | 0.989 | 1.007 | 0.996 (0.934–1.047) |
| paced cold wall_us | 0.787 | 0.947 | 0.961 | **0.995** (0.958–1.016) |
| HARD total_us (4) | 0.820 | 0.938 | 0.960 | 0.995 (0.962–1.016) |
| export elapsed_us | 0.784 | 0.911 | 0.972 | 0.975 |
| document load_us | 0.472 | 0.602 | 0.618 | 0.651 |
| ring PIE / ring scalar staging | 1.015 / 1.165 | 1.048 / 1.222 | 1.057 / 1.223 | 1.057 / 1.223 |
| whole battery guest s (hardware 77.4) | 60.0 | 69.0 | 70.4 | 72.3 (0.934) |
| host wall s → realtime | 64.5 → 0.93× | 69.9 → 0.99× | 71.7 → 0.98× | 74.9 → 0.97× (peer profiling concurrently) |

36/36 checks pass in every configuration. Known misfits, each with a named cause candidate: document load 0.65 (FP divide/sqrt sequences and conversions are unpriced; dependencies across loop backedges are not charged), ring-scalar staging 1.22 (provisional 160/96-cycle data-cache parameters from EX054), whole battery 0.93 (boot, flash/instruction-cache fetch EX055, L32R, call/return prices derived not measured). This is a fit to one firmware on one board revision; the prices are the measured ones, not tuned to this table.

### EX141: EX081's captured control cells finally analysed → measured call/return prices and a fetch-alignment cycle

EX081 ("captured / not fully analyzed") holds nine control cells, two boots, 9 samples each, identical medians in both boots (`fast-control-2026-09-07/boot-{1,2}.log`, disassembly alongside). Per bundle, against the matching straight-line control:

| Cell | extra cycles per bundle | reading |
| --- | ---: | --- |
| `nop256` − `empty` | 1.0000 | one cycle per instruction |
| `beqz` not taken | 0.0000 | not-taken branch costs its own cycle only |
| `beqz` taken over one instruction (9-byte stride) | **2.5000** | taken branch = 3 cycles, **plus one cycle whenever the target instruction straddles a 32-bit fetch word**: with a 9-byte stride the targets sweep all four alignments and a 3-byte instruction straddles at 2 and 3 mod 4 → +0.5 on average. The zero-overhead-loop ladder (core-timing README: +1 only at body start 3 mod 4 with 2-byte instructions) is the same rule, `(pc & 3) + length > 4`. |
| `call0` + `ret` | 4.5031 | call 3, return 3 (not 6 like JX), +0.5 alignment on the return address |
| `call8` + `entry` + `retw` | 4.5029 | same, ENTRY costs its own cycle only |

So my first EX138 call/return prices (call 3, ENTRY 3, return 6, derived from the 16-cycle `callx8` level) overcharged every call by about 4.5 cycles, and the near-perfect 0.998 compute fit was partly that error cancelling the missing alignment cycle. With the measured values (call 3, return 3, ENTRY 1, CALLX still assumed = JX 6, alignment cycle on every redirected fetch whose target straddles):

| Firmware timer ÷ hardware | EX140 (derived call prices) | EX141 (measured) |
| --- | ---: | ---: |
| paced cold compute_us, median of 15 | 0.998 | 0.964 (0.910–0.973) |
| paced cold present_us | 0.996 | 1.022 (0.968–1.065) |
| paced cold wall_us | 0.995 | 0.978 (0.958–0.990) |
| HARD total_us (4) | 0.995 | 0.982 (0.961–0.994) |
| export elapsed_us | 0.975 | 1.040 |
| document load_us | 0.651 | 0.639 |
| whole battery guest s (hardware 77.4) | 72.3 | 72.05 (0.931) |
| host wall s → realtime | 74.9 → 0.97× | 71.9 → **1.003×** |

The measured table is the one to keep: the compute median is 0.964 with cases from 0.910 to 0.973, instead of a median that was right partly by cancellation. Unpriced and plausible for the remainder: window overflow/underflow exception entry (35 cycles per spilled frame measured, the emulator only charges the handler's instructions), interrupt entry/resume (228/142 cycles measured), L32R, instruction-fetch cache misses (EX055), CALLX.

## Step 12: the timed model on a second firmware, and the both-busy quantum in approximate mode (EX143)

Peer review of the pricing commits found five real defects (double charge on divide fallbacks, +6 on skipped LOOPNEZ/LOOPGTZ bodies, priced hardware-loop backedges in the interpreter only, FP readiness not cleared by fast overwrites, timeline not aged by fixed opcode costs). All fixed in 3af6fd22; battery ratios unchanged (rare paths).

**pocket-tank under the timed + priced model** (`check-pt-timed*`, same exports as TinyDraw): the firmware reports **9.5 tok/s and 34–35 fps**; `docs/speed-plan.md` gives the real board as 12 tok/s and 25–30 fps; the instruction clock gives 24.3 tok/s and 62.5 fps. So the prices transfer to a firmware they were never fitted on: inference is now 20% slow (flash weight loads through the provisional 160-cycle cache fill) and rendering 25% fast (display path), instead of both being 2× fast. The harness's `model_decisions ≥ 10` check fails at this speed, as it must.

**EX143 both-busy quantum in approximate mode.** The frontier scheduler interleaves two busy cores every 64 instructions. In approximate mode there is no bit-exactness contract, only the fit to hardware, so the quantum is a free parameter (the EX066 API already allowed up to 4096). Single untimed runs:

| | quantum 64 | quantum 512 | quantum 4096 |
| --- | ---: | ---: | ---: |
| pocket-tank realtime (tok/s, fps unchanged at 9.5 / 35) | 0.47× | 0.61× | 0.61× |
| TinyDraw realtime | 0.98× | **1.11×** | — |
| TinyDraw compute / wall / HARD ratio to hardware | 0.964 / 0.978 / 0.985 | 0.963 / 0.978 / 0.985 | — |

36/36 in both TinyDraw runs. This is EX047's knob, acceptable here only because the mode is approximate and the hardware-timer fit did not move; it must not leak into the exact path.

## Step 13: EX136 hot-loop pair (no gain); the production page under the hardware-timing model, and a real bug it exposed

- **EX136 extension, cached admitted loop pair** (peer, 6b844713, 43,263 cases): pocket-tank 69.68 → 70.89 s, TinyDraw 43.00 → 43.10 s against combined3. No gain; not adopted.
- **`?timing=hw` on the page** (`night/timed-0919`): `web/wasm/worker.js` applies the timing-model exports after the loads and before boot; `web/emu.js` and the response harness pass them for `timing=hw` (the harness also takes `timing=hw-<n>-<m>` to drop exports when bisecting). Needs a wasm built with `--features cache-inline`.
- First run: the page booted the battery in 79.7 s wall (the board needs 77.4 s plus boot) but **no stroke registered**. Bisecting the exports found `esp32sim_set_measured_te`: it swaps in a new board before boot and re-attaches its I2C devices, and `I2c::attach` appended, so the old board's touch controller kept answering while input went to the new one. The battery never touches the screen, so EX056/EX066 could not see it. Fix: attach replaces a device at an occupied address.
- After the fix (`resp-timed-hw-r3`): boot to READY 80.1 s wall, 3/3 strokes committed, 24/24 movement points, movement → canvas median 38.3 ms (max 49.3), screenshot correct. So the page now keeps device time through the whole battery and stays interactive.

## Step 14: generality checks and two more negatives

- **Golden regression bar with virtual quanta, natively.** With `ESP32SIM_VQ_NATIVE=1 ESP32SIM_VQ=1000 --no-jit` (the interpreter path has the deferral guard; the AArch64 JIT does not, so the native default stays off), the three committed golden scenarios give the committed hashes: Atech synth WAV `c64c46c5…`, SID jukebox WAV `ca76a497…`, LCD-4B panel WAV `89880538…`, identical consoles and identical per-core instruction totals against `ESP32SIM_VQ=1` (319,618,108 / 338,474,266 / 396,469,561, equal to `tests/golden/*.insns`). These firmwares use I2S, RMT and LCD_CAM, i.e. the devices the EX134 cadence guard exists for.
- **EX144 any single busy core + backoff** (`night/fast-entry-0919` 1 commit): the Atech firmware runs on core 1 with core 0 idle, which EX133 as first written ignored; and it touches device registers every few instructions, so runs were cut short 3.17M times out of 3.59M (0.48 quanta per run, slower than not trying). Now whichever single core is busy qualifies, and a run cut short inside two quanta doubles a skip counter (max 255 rounds). Atech: 462K runs, 3.6 quanta each, no slowdown, still exact. TinyDraw wasm pair vs combined3: 43.20 → 43.29 s, exact. No gain on TinyDraw, removes a pathology elsewhere.
- **Cache parameter sensitivity (not adopted):** requested-data readiness 96 with 160-cycle service (EX054's split) fixes ring-scalar staging (1.217 → 1.036) but worsens present (1.022 → 0.952), HARD (0.985 → 0.942) and export (1.03 → 0.935). Left at 160/96.
- **EX145 instruction-fetch cache at dispatch granularity: no effect.** 64 sets × 8 ways × 32-byte lines for flash-mapped code, 140 cycles per missing line, touched for the entered block only: every ratio unchanged to three decimals, load_us stays 0.639. Removed again. Either the working set fits or the misses happen inside regions where this coarse model cannot see them.
- **Document load (peer's scoped native profile, `doc-load-native/`)**: the timed window is 57.5M instructions of eager rasterization (raster helpers 48%, MaterializedCanvas 22%, memmove/memcpy 6.5%, `__divsf3` 5.6%, `lroundf` 3.3%), 37% of it fetched from flash. The missing 200 ms is about 0.84 cycles per instruction, far more than the unpriced FP divide assist (100K divides) or LSI→use (3.4M) can supply, so the data side (cache geometry, PSRAM misses and dirty writebacks under scattered access) is the leading remaining hypothesis. Instruction counts alone do not rule out unknown per-operation latencies or path differences, and the coarse fetch-cache null result does not rule out fetch misses inside regions. Needs a hardware probe, not another parameter guess.

## Step 15: timed-model host cost, merge fix, Tier-B hardware session started

- Profile of the timed + priced model (`prof-timed`, 69 s sampled): generated 36.8%, `jit::run` 13.3%, `run_block_inner` 10.9%, and 1.4 s in `decode` — my alignment check decoded the target instruction on every redirected fetch through the helper (51M RETWs). Replaced by a length test on the first byte, skipped when `pc & 3 < 2`. Same ratios; TinyDraw timed + priced, both-busy quantum 512: **72.0 guest s in 60.6 s wall = 1.19× realtime** (`check-timed-priced-r4`, single untimed run).
- Merge fix: `pie::timing_tests::selective_pie_costs…` (from the timing branch) failed after the merge because main's packed PIE executor bypasses the table executor where the optional PIE cost hypotheses are charged. Packed execution is now skipped when such a hypothesis is selected (none is in our configuration). `cargo test --release --workspace`: no failures on `night/timed-0919` f16d4daf.
- **Tier-B hardware session** (tinydraw `calibration/esp32s3-tier-b` at 7a157d4, the probe whose README says its decomposition controls were never captured; `~/Archives/esp32s3/tier-b/` did not exist): normal image built and ELF-verified under IDF v6.1, then flash + `tools/tier-b-capture.py --cells all` for two boots, then the XIP-PSRAM image the same way. Runs in the background from `~/Archives/esp32s3/tier-b/logs/session.sh`; the tinydraw checkout was dirty (`.gitignore`, untracked site files) and the verifier records that. This is the data-side probe the document-load misfit needs; analysis is for the morning.

## Step 16: Tier-B cohort captured; measured cache prices replace the fitted ones; PIE Q readiness (EX146)

- **Tier-B hardware cohort** (`~/Archives/esp32s3/tier-b/`): normal image boots 1 and 2: 43/43 cells, 360 samples, 0 refusals each; XIP-PSRAM image boots 1 and 2: 44/44 cells, 373 samples, 0 refusals each; receipts and the archived ELFs written by `tools/tier-b-capture.py`. Both boots of the normal image agree exactly on these window totals: `first_line_d_psram` **96 cycles** and `first_line_d_flash` **128** (each times the probe's whole one-line read window, with no matched hit control subtracted, so they are not isolated miss penalties); `first_line_i_flash` 404 (one 724 outlier per boot); explicit dirty `msync` writeback 1016 / 1170 / 1506 / 2154 / 3442 cycles for 1 / 2 / 4 / 8 / 16 lines (**≈162 cycles per additional dirty 64-byte line** on top of 862 for the clean call; an explicit flush cost, not a proven price for automatic victim eviction); `store_hit_psram` 272 for its whole window of 256 stores (baseline 276). The firmware's data cache is 32 KB, 8-way, 64-byte lines (`sdkconfig`); the model has 4 ways (the inline wasm path is built for that geometry).
- **Measurement-informed cache prices** (fill 96, writeback 160, taken from the window totals above with the caveats stated there; one shared fill price also covers flash, whose window is 128) instead of EX054's fitted 160/96, on top of the instruction prices: ring-scalar staging 1.217 → **1.033**; every other timer moves down a little and the medians cluster near 0.96 (individual cases vary): compute 0.956 (0.883–0.969), present 0.981, wall 0.958, HARD 0.955, export 0.964, document load 0.630; whole battery 70.4 guest s (0.91). The fitted 160 had been absorbing CPU cost that was not priced yet. Ring-PIE staging drops to 0.71: it was only right before because the fill was inflated (EX059 already saw it 17% short).
- **EX146 static readiness of loaded PIE Q registers**: operand masks from the EX058 prototype (810f38c5), only the measured results delayed (VLD, LD.USAR, loaded Qu of SRC.Q.LD usable at issue + 2, EX080), folded into the same per-run table as load-use and FP readiness. Linear PIE staging 0.953 → **0.997**; ring PIE 0.704 → 0.714 (its shortfall is memory-side).
- Host cost unchanged: 70.4 guest s in 60.2 s wall = 1.17× realtime. `?timing=hw` on the page now uses the measured cache prices.

State of the timed model against the erased-start board: a measurement-informed model with remaining assumptions (CALLX = JX, SUB.S/MSUB.S = ADD.S, one-cycle latency for every unmeasured FP/PIE writer, round-robin replacement, explicit-`msync` cost applied to automatic eviction, one fill price for PSRAM and flash). Reported medians cluster near 0.96 on compute, wall, HARD and export, with individual cases from 0.88 to 0.98; document load 0.63 and ring-PIE staging 0.71 are the two open misfits, for which the data side is the leading hypothesis, not an established cause.

## Step 17: combined4, EX111, cache geometry

- **combined4** (1b28b34e on `night/combined-0919`: combined3 + EX144 + the 1024-quanta wasm default, built with no environment knobs; peer: 43,261 differential cases incl. new legacy-vs-virtual-quanta machine tests with only core 1 busy, native suites, CI clippy): TinyDraw 83.00 → 44.43 s in its own pair and 42.48 s as the control of the next pair (so equal to combined3 within noise), exact; pocket-tank 80.91 → **65.41 s (−19.2%)**, exact — EX144 helps there because one of its cores does idle at times.
- **EX111 tiny tails** (peer, 8850d6c6, 43,262 cases; only budget-1 continuations inside a decoded block stay interpreted): pocket-tank 65.54 → 65.41 s, TinyDraw 42.48 → 43.12 s against combined4. No gain. Not adopted.
- **Timed model cache geometry**: the model and the inline wasm probe now use the firmware's 8-way data cache (64 sets) instead of 4 ways. Ratios move by less than 0.015 (present 0.981 → 0.995, HARD 0.955 → 0.959); kept because it is the real geometry.
- **combined4 confirmed with three alternating pairs** (`runs/combined4-x3`, default build, no knobs): base 83.27 / 84.16 / 83.57 s, candidate 42.50 / 42.45 / 42.75 s → **−49.2%, 1.12× modeled realtime**, bit-exact in all six runs. `tools/wasm-test.mjs` manifests hello, c3-hello, c6-hello, c6-energy-scan, panel, atech and atech-sid pass on the same artifact.
- Peer's design note for calls inside regions: `design-calls-in-regions.md` (direct CALL8 → ENTRY-headed chunk as an internal edge, RETW and CALLX still leaving; optimistic ceiling 45.6M dispatches ≈ 1.7 s ≈ 4%).

## Step 18: one head with everything

`night/timed-0919` now contains combined4 as well (merge, 2 conflict files). On that head:
- wasm differential suite: 43,261 cases with default features, 43,279 with `cache-inline` (the inline-cache test now follows the 8-way geometry); `cargo test --release --workspace` has no failures.
- Exact mode (plain build, no timing exports): TinyDraw bit-exact, 43.70 / 43.67 s against combined4's 42.69 / 42.53 s in two pairs, i.e. **the timing hooks cost about 2.5% when unused** (1.09× instead of 1.12×). `night/combined-0919` stays the branch to take for the exact path alone.
- Timed mode (`cache-inline` build + exports): identical to the pre-merge run (10,101,385,928 instructions, same ratios).

## Step 19: one artifact for both modes; page checks on the unified head

- A `cache-inline` build used to emit the inline data-cache probe into every generated memory access, whether or not the timing model was on: exact mode 48.33 s against 43.52 s, 28% more generated code. The probe is now emitted only after `esp32sim_set_approximate_jit_cache(…, 2)` switches the model on, and `cache-inline` is a default feature of the wasm crate on `night/timed-0919`. Same artifact: exact mode 43.79 s (control 43.69 s, identical generated bytes and console hash), timed mode identical to before (10,101,385,928 instructions, 62.6 s wall for 70.6 guest s). 43,279 differential cases and the workspace tests pass.
- Production page with that default artifact (`resp-unified-*`): exact mode boots the battery firmware to READY in 56.5 s wall, `?timing=hw` in 78.9 s; both commit 3/3 strokes and answer 24/24 movement points (medians 36.2 and 38.6 ms). The hardware figure next to the 78.9 s is 77.4 s from the startup serial line to the verdict; the endpoints differ (the page time runs from worker start to the READY line), so read it as "same order", not as a matched comparison.
- Board state at the end of the night: last flashed with the Tier-B XIP-PSRAM calibration image (`flash-xip-psram-2.log`), not TinyDraw.

## Step 20: readiness vs service, from the same Tier-B cohort (peer's finding)

The peer noticed that the cohort separates first-word readiness from sustained line service: `first_line_d_psram` is a 96-cycle window, but the uncontended baseline of `arbitration_psram_victim_internal_aggressor` is 702,762 cycles for 4,096 cold 64-byte lines = **171.6 cycles per line** (loop overhead included), and `flash_bandwidth_cross_core` 1,946,208 / 4,096 = 475.1 per line. The model charged fill = service = 96. With EX054's split (requested word ready after 96, the line occupies the memory service for 160, writeback 160):

| Firmware timer ÷ hardware | fill = service = 96 | ready 96 / service 160 |
| --- | ---: | ---: |
| ring PIE staging | 0.714 | **0.933** |
| ring scalar staging | 1.032 | 1.039 |
| linear PIE / scalar staging | 0.997 / 0.998 | 0.998 / 0.998 |
| paced cold compute_us (15) | 0.957 | 0.962 |
| paced cold present_us / wall_us | 0.995 / 0.961 | 1.007 / 0.971 |
| HARD total_us (4) | 0.959 | 0.979 |
| export | 0.969 | 0.975 |
| document load | 0.627 | 0.628 |
| whole battery guest s (hardware 77.4) | 70.6 | 71.0 |

Single untimed run, 36/36 (`check-tp-r96s160w160`, 71.0 guest s in 62.4 s wall = 1.14× realtime). Adopted for `?timing=hw`. The 160 is the 171.6-cycle total less an unmeasured loop overhead, so it is measurement-informed, not measured; flash lines (475 per line sustained) still share the PSRAM price, which pocket-tank's flash-resident weights will expose. Document load does not move: whatever is missing there is not line fill cost.
- **Linux on the S3 in the wasm module** (`tools/wasm-test.mjs linux`, image fetched by `tools/fetch-demo-assets.sh`, pinned by SHA-256): reaches `buildroot login:` on both builds with the same 440.8 M instructions and 170 console lines; base 18.1 s wall, combined4 15.3 s. A second dual-core system (network adapter on core 0, Linux XIP on core 1, MMU remaps, a function stub) that virtual quanta leave intact.

## Step 21: the document-load misfit is the instruction-fetch cache (EX147), not the data side

EX145's null result came from its granularity: it touched only the entered block's lines, and with 512-instruction regions almost no fetch line was ever touched, so the working set looked tiny. EX147 touches, per region entry, the fetch lines of the region (64 sets × 8 ways × 32-byte lines as in the firmware's `sdkconfig`, flash-mapped code only), and charges the Tier-B `first_line_i_flash` window of **404 cycles** per missing line. Same configuration otherwise (ready 96 / service 160 / writeback 160), single untimed runs, 36/36:

| Firmware timer ÷ hardware | no fetch cache | whole region span touched | the region's chunk lines touched |
| --- | ---: | ---: | ---: |
| document load_us | 0.628 | **0.956** | 0.737 |
| paced cold compute_us (15) | 0.962 | 0.981 | 0.976 |
| HARD total_us (4) | 0.979 | 0.998 | 0.999 |
| paced cold wall_us | 0.971 | 1.007 | 1.003 |
| paced cold present_us | 1.007 | 1.076 | 1.063 |
| export | 0.975 | 0.985 | 0.978 |
| whole battery guest s (hardware 77.4) | 71.0 | 73.3 | 72.4 |

The load window fetches 37% of its instructions from flash (peer's scoped profile), and a fetch-line miss costs four times a data-line miss, so this is where its missing 200 ms are. Both variants are approximations in opposite directions (the span also fetches gaps and unexecuted chunks; the chunk list misses entries through the slow path and treats every chunk as executed), and both overshoot presentation by 6–8%, so EX147 stays a diagnostic switch (`esp32sim_set_icache_fill`, off in `?timing=hw`). A faithful version needs the touch where a chunk is actually entered. My earlier note that the data side was the leading hypothesis is withdrawn.

Refinement the same hour: generated regions now record the chunks they actually enter (a 64-entry ring in `Cpu`, one store and one increment per chunk head of flash-mapped regions, emitted only when the fetch cache is on), and `jit::run` replays them into the fetch cache after the call. With per-core caches that gave load_us 0.709. Making the cache **one for both cores, as on the chip**, gives:

| Firmware timer ÷ hardware | no fetch cache | EX147, shared cache, chunk replay, 404 per line |
| --- | ---: | ---: |
| document load_us | 0.628 | **0.933** |
| paced cold compute_us (15) | 0.962 | 0.983 |
| HARD total_us (4) | 0.979 | 1.004 |
| paced cold wall_us | 0.971 | 1.013 |
| export | 0.975 | 0.992 |
| paced cold present_us | 1.007 | 1.072 |
| whole battery guest s (hardware 77.4) | 71.0 | 73.8 (0.954) |

Ranges over the individual cases: compute 0.977–0.989 (all 15 within 1.2 points of each other), HARD 1.002–1.019, wall 0.986–1.042, present 1.025–1.106. 36/36, single untimed run (`check-tp-icache404-shared`), 73.8 guest s in 70.4 s wall = 1.05× realtime. Core 1's flash-resident code (the touch sampler and FreeRTOS paths) evicts core 0's lines: a cross-core effect through the shared fetch cache that no per-core model can show. Adopted for `?timing=hw`; presentation is now the one timer more than 2% off in the median (7% slow).

- **pocket-tank under the final `timing=hw` set** (`check-pt-hw`): 8.0–8.4 tok/s and 32–33 fps (board: 12 and 25–30), 30 guest s in 50.2 s wall. Inference got slower than with the fetch cache off (9.5 tok/s), i.e. further from the board. Two known reasons not to read much into it yet: the cache geometry in the model is TinyDraw's `sdkconfig` (64-byte data lines, 16 KB fetch cache), while pocket-tank is a different build whose bootloader programs its own geometry, which the model should take from the cache configuration registers instead of constants; and the peer found that packed PIE loads go through `Bus::read_bulk`, which bypasses the data-cache pricing (fix in progress).
- Peer's fix f12d9837 (packed PIE loads no longer bypass data-cache pricing through `read_bulk`; native regression went from 0 fills / 0 hits to 1 / 3) and lint cleanup a2e8b5af are on `night/timed-0919`. TinyDraw ratios unchanged (`check-hw-final2`: compute 0.982, HARD 1.004, wall 1.013, load 0.933, present 1.074; 74.0 guest s in 71.6 s wall). pocket-tank now 7.4–8.0 tok/s, 32–33 fps (`check-pt-hw2`): its weight loads are priced at last, and at TinyDraw's cache geometry and the shared PSRAM/flash price they are priced too high; the board does 12 tok/s.
- Production page with the final `timing=hw` set (`resp-hw-final`): READY after 88.1 s wall, 3/3 strokes, 24/24 movement points, 42.8 ms median (max 59.9).
- The timed model is deterministic: two runs of the final head and export set give the same 10,152,693,296 instructions, 73.975 guest seconds and console hash c8ec9334… (`check-hw-final2`, `check-hw-final2-repeat`; wall 71.6 and 75.4 s, so about 1.0× realtime with the fetch cache on).
- Instruction-side numbers still unused from the cohort: XIP-PSRAM `instruction_psram_cold` 993 against `instruction_psram_hot` 133 cycles for 256 bytes, i.e. about 107 cycles per sequential fetch line from PSRAM; for flash only the isolated first-line window (404) exists. A sequential flash fetch-line probe would settle whether the 7% presentation overshoot is a too-high sequential fetch price.

## Step 22: pocket-tank's cache configuration, flash price knob (peer), what the board must be doing

- The firmware's own cache geometry is readable from the emulated EXTMEM registers after boot (`--peek 0x600c4000,4 --peek 0x600c4060,4`): TinyDraw programs `DCACHE_CTRL = 0x11` (64-byte lines, 32 KB) and `ICACHE_CTRL = 0x0b` (32-byte lines, 16 KB, 8 ways); **pocket-tank programs `DCACHE_CTRL = 0x15`: a 64 KB data cache**. The model's geometry is a constant; it should follow these registers. Added a 64 KB option for now (`esp32sim_set_approximate_jit_cache(…, 3)`, 128 sets in the inline probe).
- pocket-tank, final export set (single untimed runs): 32 KB cache 7.4–8.0 tok/s; 64 KB cache 7.9–8.3; 64 KB plus the peer's separate flash price (26e82aa1, `esp32sim_set_approximate_flash_timing(128, 160)`) 7.4–8.0; fps 32–33 throughout; the board does 12 tok/s and 25–30 fps. Neither knob closes the inference gap, and a bigger flash price moves away from the board. The weights stream sequentially from flash, so the likely missing piece is the S3's data-cache autoload (sequential preload), which ESP-IDF enables and the model does not have: a line-at-a-time miss price cannot be right for a sequential stream. Not attempted tonight.

## Step 23: EX147 review fixes and the price's uncertainty

Peer review of EX147 found: the ring was replayed only on the hot region-entry path (regions entered through the slow path, e.g. with an active in-region hardware loop, were never priced); the fetch cache was a module-global that a second emulator in the same instance would inherit; more than 64 chunk entries per call lose their order (left as a labeled approximation); and the 404-cycle window's raw records show two fetch misses, so 404 is not a proven per-line price. Fixed the first two (both call paths replay; the cache resets when the price is set for a new machine). Ratios with 404 are unchanged. Sensitivity to the price, same head (`check-hw-final5*`, single runs, 36/36):

| Firmware timer ÷ hardware | 404 per line | 200 per line |
| --- | ---: | ---: |
| document load_us | 0.934 | 0.757 |
| paced cold compute_us | 0.982 | 0.971 |
| HARD total_us | 1.007 | 0.995 |
| paced cold wall_us | 1.013 | 0.990 |
| paced cold present_us | 1.074 | 1.030 |
| export | 0.992 | 0.983 |

Document load wants at least 404, presentation at most 200, so presentation's overshoot is not only a fetch price. `?timing=hw` keeps 404 as a provisional value; a sequential flash fetch-line probe is the measurement that would replace it.

## Step 24: two more nulls

- **Data-cache autoload is not the pocket-tank explanation** (peer, `autoload/*-peek.log`): after boot both firmwares leave `DCACHE_AUTOLOAD_CTRL` (0x600c404c) at 0x8, enable and section bits clear, section registers zero; IDF 6.1's `cache_hal_init` preserves that state and no S3 call site enables autoload. The 8-versus-12 tok/s gap stays unexplained.
- **EX149 solo memory stalls spend budget instead of ending the batch** (timed model): 69.8 s against 70.6 s wall, same ratios. No gain; reverted.
