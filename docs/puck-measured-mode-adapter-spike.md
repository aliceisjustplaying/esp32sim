# Lane B design spike: measured execution and the Puck backend adapter

Date: 2026-08-31

Status: ready for maintainer review. This is a throwaway design-spike
deliverable. It specifies interfaces against esp32sim commit
`aa851249341e8cd122e7f4852d4c0f002e46d887`, whose upstream code is pinned at
`2114ffc92039b4605264d2cfb4ee5543acbf98c1`. It contains no product
implementation and authorizes no merge.

The companion [decision-record draft](puck-adapter-scheduler-decision-draft.md)
is explicitly unaccepted. The adapter protocol, event schema, timing
vocabulary, and scheduler semantics are cross-lane interfaces. They require
maintainer approval before implementation or merge.

## Result

The spike is feasible with two separate seams:

1. A Puck-owned, versioned backend adapter is the only product-facing surface.
   It owns virtual time, bounded typed events, quotas, artifacts, capabilities,
   and stable errors.
2. A measured interpreter path in the CPU backend owns instruction and memory
   observation. It prices an instruction before its next architectural
   boundary, advances CCOUNT and device time from the same cycle ledger, and
   emits the normalized ledger the adapter returns.

Measured execution cannot be implemented as a `Bus` wrapper or as a price pass
over a completed trace. The current JIT bypasses `Bus` for fast memory, and the
current scheduler advances device time after execution quanta. The measured
path must therefore be a separate interpreter backend selected outside the
existing fast inner loops. Fast mode remains unchanged.

## Code inspection receipt

| Current seam | Observed behavior at the pinned code | Required measured-mode seam |
| --- | --- | --- |
| `esp32s3/src/machine.rs:336-385` | `Machine::run(max_insns)` interleaves cores in 64-instruction quanta. Its stop budget is not a virtual-cycle deadline. | Add an absolute-cycle `run_until` path. Keep `Machine::run` as the fast path. |
| `esp32s3/src/machine.rs:387-432` | `after_round` calls `SocBus::tick` only after a scheduling round. Host pacing is mixed into `Machine`. | Measured scheduling advances devices before guest-visible deadlines. Host wall time is cancellation or pacing only and never changes virtual time. |
| `esp32s3/src/bus.rs:191-215,628-677` | MMIO flushes already pending ticks. Device delivery is otherwise lazy, bounded by `MAX_TICK_DEFER = 256`. Only systimer and timer-group deadlines are computed. | Give every active time-aware device an exact `At`, `None`, or `Unknown` deadline. `Unknown` blocks measured advancement. MMIO flushes to the current virtual cycle before access. |
| `xtensa-lx7/src/block.rs:157-215` | The block interpreter executes instructions, then advances `insn_count` and CCOUNT by the executed instruction count. CCOMPARE limits are also instruction-count limits. | A measured block carries exact cost references and prefix sums. It stops at the instruction boundary that crosses a CCOMPARE or device deadline, then commits the cycle delta. |
| `xtensa-lx7/src/exec.rs:117-124` | `advance_ccount(cycles)` already handles wrapping CCOUNT and three CCOMPARE registers for one scalar delta. | Reuse this state transition with measured cycle deltas. Record the exact match cycle and sample its interrupt at the next instruction boundary. |
| `xtensa-lx7/src/block.rs:143,187` and `xtensa-lx7/src/jit/mod.rs:468-472` | JIT compilation detects `fast_mem`, and generated code receives raw TLB and page-version pointers. RAM and mapped flash/PSRAM loads and stores can bypass `Bus`. | Observation is emitted by the CPU backend. Measured mode is interpreter-only until a JIT conformance proof covers RAM, flash, MMIO, faults, self-modifying code, and cross-page accesses. |
| `xtensa-lx7/src/bus.rs:33-63` | `Bus` exposes memory methods, fetch, page versions, `note_pc`, block breaking, fast memory, and tick. It has no complete observation contract. | The measured CPU backend emits normalized instruction, fetch, data, fault, trap, and invalidation observations. The bus supplies resolution facts but is not the authority for completeness. |
| `wasm/src/lib.rs:176-204` | `esp32sim_run(cycles, unix_ms)` sets `max_cycles` and then calls the instruction-budgeted machine loop. Host Unix time is accepted directly. | The Puck adapter accepts an absolute virtual deadline and an explicit deterministic input transcript. It does not accept host time as simulated time. |
| `cli/src/main.rs:81-102,116` | Networking is created only when `--wifi` is supplied. `--no-jit` disables both native block caches. | The spike configuration requires interpreter execution and `NetworkPolicy::None`; incompatible configuration is rejected during `create`. |

The current fast implementation also establishes useful mechanisms that the
measured path can reuse without exposing them through the adapter: decoded
blocks, page-version invalidation, MMU invalidation, `advance_ccount`, and the
interpreter's exact instruction semantics.

## Interpreter-only, networking-off run receipt

The spike exercised the existing selection points with a committed fixture:

```text
cargo build -p esp32sim
./target/debug/esp32sim \
  --board none --boot app \
  --app examples/wifi-station/build/wifi_station.bin \
  --elf examples/wifi-station/build/wifi_station.elf \
  --flash-mb 4 --psram-mb 2 \
  --no-jit --net none --max-insns 100000 \
  --console none --no-dump
```

Fixture hashes:

- `wifi_station.bin`:
  `fcb459809eb6ece09d6f03bb7d964361ead918ed176af4b2423883ed46a81f94`
- `wifi_station.elf`:
  `2f88bd958bcf22cd02e99d9ed8d61e6387fa4c3845e973e4f701680dcd4c0a5a`

The bounded direct-app run executed 41,775 core-0 instructions and reported
`jit: 0 compiled`. No `--wifi` option was supplied, so the code at
`cli/src/main.rs:81-102` created neither a virtual AP nor a network backend.
The guest reached its existing software-reset stop. This receipt proves only
that the spike used the required interpreter and networking-off configuration.
It is not a timing or firmware-correctness claim.

`cargo test --workspace` also exited zero under Rust and Cargo 1.98.0. Existing
warnings are unchanged from the pinned base.

## Proposed internal layering

| Layer | Owns | May import |
| --- | --- | --- |
| `puck-backend-api` | Adapter types, protocol and event versions, stable errors, fake backend contract suite | No esp32sim crate |
| `puck-esp32sim-backend` | Translation between adapter values and esp32sim `Machine` | `puck-backend-api`, `esp32s3`; it is the only Puck-owned crate allowed to import esp32sim internals |
| `esp32s3` measured scheduler | Virtual-cycle advancement, CPU selection, device deadlines, interrupt sampling | `xtensa-lx7` measured CPU interface and SoC devices |
| `xtensa-lx7` measured interpreter | Complete CPU observations, instruction pricing boundary, CCOUNT batching | CPU decode and interpreter internals, a timing-source interface supplied by the SoC |
| Existing fast machine and JIT | Upstream fast behavior | Existing internals only |

The dependency rule is mechanical: all Puck product code imports
`puck-backend-api`; only `puck-esp32sim-backend` may import `esp32s3` or
`xtensa-lx7`. The fake backend depends only on `puck-backend-api`.

## Measured CPU transaction

One measured instruction is one architectural transaction:

1. Decode from the existing block cache and emit `InstructionStart` with core,
   PC, exact encoding, and width.
2. Ask the timing source for the instruction's base claim and any statically
   classifiable additions, including branch route, loop alignment, and a
   pending dependent load-use hazard.
3. Stage instruction fetch and data accesses through the timing memory model.
   The stage resolves address class, cache state, MMIO class, line fills, and
   faults before committing cache-model mutations.
4. If every required duration resolves to a non-negative deterministic cycle
   delta, execute the interpreter instruction, commit staged timing state, and
   append its ledger entries.
5. If any cost or access shape is unknown, append the blocking attempt and stop
   before the instruction. No CPU, bus, cache-model, device, CCOUNT, or virtual
   time state changes. The loaded artifact set is immutable, so reset or a new
   backend with a newly approved timing manifest is required before progress.
6. Advance CCOUNT, device time, and interrupt assertions to the committed
   completion boundary. Sample pending interrupts before the next instruction.

The staging rule keeps timing-driven execution from running ahead and pricing a
finished trace. A multi-access instruction is supported only when the planner
can resolve every possible access without causing a guest-visible side effect.
For example, an atomic RAM operation may use a read-only preview, while the
same shape against side-effecting MMIO blocks before execution.

The first measured implementation supports only instruction shapes it can
stage completely. Any fallback instruction whose internal memory behavior is
not represented by the planner has tier candidate `unexplained` and blocks.
Support grows by adding an exact planner and a receipt-backed cost mapping, not
by assigning a default.

## Lazy device-time delivery

Measured mode replaces the current implicit 256-cycle delivery bound with an
explicit device deadline interface:

```rust
enum DeviceDeadline {
    At(u64),
    None,
    Unknown { device: DeviceId, reason: String },
}
```

`None` means the device proves it cannot change state without a guest access or
an injected event. It does not mean that no deadline implementation exists.
`Unknown` blocks measured time advancement and is recorded as an unexplained
tier candidate.

The measured scheduler advances from `virtual_now` to the earliest run,
injection, device, or CCOMPARE boundary. Devices may receive a batch only when
their declared next deadline is not crossed. A peripheral read or write first
delivers all device time through `virtual_now`; a write that arms a deadline is
then visible when the scheduler recomputes the minimum. Run partitioning must
be inert: one `run_until(1000)` call and ten calls ending at 100, 200, through
1000 produce the same state, events, and ledger hash.

DMA, LCD, audio, USB, WiFi, and board devices currently rely in part on the
coarse defer bound. They remain unsupported in a cycle claim until their active
paths return exact deadlines. Lane A owns board-device deadlines. Lane C owns
shared MSPI and dual-core contention deadlines. Live networking is outside the
measured contract.

## Block-batched CCOUNT

Measured mode uses a separate measured block cache so fast `Entry` and
`BlockInsn` layouts and the JIT path do not gain timing fields or branches. A
measured entry carries decoded instructions plus receipt-backed base-cost
references and exact prefix sums for the costs known before execution.

The interpreter may aggregate several completed instruction deltas and call
`advance_ccount(total)` once when all of these are true:

- no instruction reads or writes CCOUNT, CCOMPARE, PS, INTERRUPT, or INTENABLE;
- no prefix crosses a CCOMPARE match or declared device deadline;
- no memory, MMIO, trap, reset, or injected event requires an earlier flush;
- no cost in the aggregate is unknown.

At the first crossed CCOMPARE, the batch ends at the completing instruction.
The ledger records the exact match cycle even when the match occurred during
that instruction, and the timer interrupt is sampled before the next
instruction. Reads and writes of CCOUNT and CCOMPARE remain block-first and
flush any pending delta before execution.

Idle advancement uses the same virtual-cycle delta for the running core's
CCOUNT. Measured dual-core scheduling remains capability-disabled in lane B;
lane C must define how both core-local counters advance under its accepted
interleave and contention policy.

## CPU-backend observation contract

The normalized observation stream is produced at the CPU backend, not inferred
from the bus after execution. Every event has adapter event-schema version,
reset epoch, monotonic sequence, core, instruction sequence, and virtual-cycle
position. The required variants are:

- instruction start and commit, with PC, exact bytes, width, next PC, and trap
  outcome;
- instruction fetch, literal load, load, store, and atomic access, with address,
  width, resolved memory class, cache emissions, and success or fault;
- MMIO read or write with peripheral class and the exact match key used for a
  receipt;
- window exception, ordinary exception, and interrupt assertion and acceptance;
- code-page write-version change and MMU invalidation;
- idle advance and device deadline delivery;
- one or more tier-carrying cost entries, or one explicit blocking unknown.

The interpreter emits these facts as it executes. A bus method may return
resolution and cache facts to the CPU backend, but the bus does not decide
whether the observation set is complete.

JIT capability for measured or traced execution stays false until one committed
conformance program covers RAM, mapped flash, MMIO, successful and faulting
accesses, self-modifying code, and cross-page accesses. Interpreter, JIT slow
memory, and JIT fast memory must produce identical normalized observations,
architectural state, memory state, and invalidation state. A proof is identified
by corpus hash and result hash in `capabilities()`. Absence or mismatch disables
the JIT; it never skips the proof.

## Adapter mapping

The exact proposed adapter is in the unaccepted decision draft. Its key
semantics are:

- `create` accepts validated deterministic config. The spike accepts measured
  mode, interpreter engine, and `NetworkPolicy::None` only.
- `load` accepts bounded, immutable, SHA-256-verified ROM, bootloader,
  partition, app, ELF, and timing-profile artifacts.
- `reset` names power-on, software, watchdog, or external reset and starts a new
  epoch. Partial snapshot support is reported absent.
- `run_until` accepts an absolute virtual deadline and independent instruction,
  wall-cancellation, memory, output, and ledger budgets. The wall budget never
  changes simulated time.
- `inject` accepts a timestamped owned event no earlier than `virtual_now`.
- `drain_events` returns owned, typed, bounded events with no guest pointers.
- `inspect` requires an explicit debug capability and a maximum byte count.
- `capabilities` reports adapter and event versions, backend commit, engine,
  networking, snapshot, board, measured-dual-core, receipt-manifest, and JIT
  observation-proof support.
- `close` is deterministic and leaves no worker, socket, timer, or executable
  mapping.

The adapter event queue is lossless in the initial contract. Reaching its byte
or count quota stops execution with `QuotaExceeded`; it does not drop or
coalesce silently. The output validator shared with lane H runs before an event
enters this queue.

## Cost and receipt boundary

The measured engine accepts a cost only through a hash-pinned timing manifest.
Each claim carries its decision-0008 tier and a source containing repository,
commit, path, SHA-256, firmware identity, toolchain identity, board revision,
and adoption status. An unreviewed candidate is not executable cost.

The current profile is not yet a valid measured-mode manifest:

| Surface | Current committed state | Measured-mode treatment |
| --- | --- | --- |
| Steady instruction issue and independent SRAM access | `timing.json` has scalar values and evidence links | Import only after the tiered manifest identifies their accepted tier and lane 0 records the ESP-IDF 6.1 rebaseline. |
| Branch route | The exact `beqz` adoption manifest is committed | Match its exact encoding and route only. All other conditional branches remain unknown. |
| Cache line fills and hot hits | Adopted manifests are committed, with instruction-PSRAM, store-hit, and writeback gaps | Use only the adopted address, cache, and burst shapes. Missing classes block. |
| Dependent load-use | The hot-hit adoption records an observed additional cycle but labels it `unmodeled`; the flexe classifier is reference-only | Lane B may add a decoded-register classifier after review. Every unsupported producer or consumer form blocks. |
| Window pair and loop alignment | The lane brief names 35 cycles and +1 at `+3 mod 4`; `STATUS.md` labels the core-timing results unreviewed candidates | Lane 0 must commit the ESP-IDF 6.1 rebaseline and adoption disposition before lane B uses either number. Until then both are unknown. |
| MMIO reads | Exact address, operation, and width adoptions exist for a small set | Match the entire receipt key. Any other address, width, operation, or state blocks. |
| Same-value MMIO writes | The adoption evidence proves the affine cohort `3n - 8`, but schema-1 `timing.json` exposes only a 3-cycle entry | Reject schema 1 for measured totals. Preserve slope, intercept, cohort bounds, and receipt. No per-event timeline is inferred from this aggregate claim. |

The affine MMIO gap is a fail-closed blocker, not a rounding issue. Lane B owns
the tiered profile importer and must preserve the `-8` intercept. Because the
receipt does not establish where that intercept lands in an online instruction
timeline, the first matching same-value write returns `TimingBlocked` until a
reviewed event-scoped resolver exists. If additional silicon discrimination is
needed, lane B files a request to lane E; no hardware access is performed by
lane B.

Lane 0 owns toolchain rebaseline and adoption inputs. Lane B must not combine
new IDF 6.1 evidence with the historical IDF 6.0.2 cohort in one manifest.

The inspection used Puck commit
`a91fddc9cb1629ee2de37d916468ee3eb8f681f7`. These are the exact committed
source hashes consulted by the spike:

| Source | SHA-256 |
| --- | --- |
| `docs/decisions/0008-tiered-cost-vocabulary-and-acceptance-bounds.md` | `38f79a88675a59c43a887b6401571b133ee566438de26d5f6c332150e11d7214` |
| `packs/esp32-s3-touch-amoled-18/timing.json` | `31a83ab4fe2253ef7ff5a0bcc944aa5c9ca38f90eef485f48f8f725fd790402a` |
| `timing/evidence/esp32s3-rev02-tinydraw-bf169bc-counters-candidate.json` | `6e5bd06e4a0081cefc2e9f7d5ed910b0a32518d7681d40c5e2f135f3eb21754b` |
| `timing/evidence/esp32s3-rev02-tinydraw-a91d1d7-cache-burst-adoption.json` | `c181adf14f60401efa974d3807aa1c5954294745455cb8520d205d269cfd487b` |
| `timing/evidence/esp32s3-rev02-tinydraw-1ddd64b-4a2c659-hot-hit-adoption.json` | `b8d872688aba5f7067a15bdfe7bec66beb6631155298fe38bf8a055f3cd4db57` |
| `timing/evidence/esp32s3-rev02-tinydraw-2bf3ffd-beqz-adoption.json` | `335326d061acb0fe7465cfaa596bd77eb064ebd8b08643e2339a7749af781095` |
| `timing/evidence/esp32s3-rev02-tinydraw-e8a9f0e-mmio-write-adoption.json` | `ac04584f3a05931795d65dc7246ae556202dd98bb7304cce06f50b5a29b0dc8a` |
| `experiments/esp32s3-flexe-wasm/STATUS.md` | `a05937352c587e2b3313af6503c83bd7c91198470090cc8128bd27af2c4fa8d4` |

Paths abbreviated with `timing/evidence` are below
`packs/esp32-s3-touch-amoled-18/`. `STATUS.md` is a ledger, not a timing
receipt. Its hash is included to make clear that the 35-cycle window result and
loop-alignment result were inspected only as unreviewed candidates.

## Contract tests required after approval

The fake backend and esp32sim backend must pass the same adapter cases:

- version negotiation, invalid config, artifact hash and size rejection;
- every reset kind and reset-epoch sequencing;
- absolute deadline behavior and run-partition invariance;
- same-cycle reset, device, injected input, interrupt, and CPU-boundary ordering;
- event and ledger quotas with stable typed errors;
- privileged and denied memory inspection;
- close idempotence at the host wrapper and resource cleanup;
- one known exact ledger and one unknown that blocks without a total;
- timing manifest and receipt hash mismatch;
- networking absent and JIT absent from measured capabilities.

Measured CPU tests then cover CCOUNT wrap, all three CCOMPARE registers,
deadline crossing inside a priced instruction, MMIO flush-before-access,
self-modifying invalidation, cache line-fill sequencing, loop alignment,
window traps, dependent load-use, and unsupported-shape refusal.

The TypeScript timing machine and measured interpreter consume the same
normalized trace fixture and must emit the same event order, tier labels,
receipt identities, blocked event, and known ledger total. Same trace and same
manifest must produce the same ledger hash in repeated runs.

## Review gates

Maintainer approval is required for the companion decision draft before any
product implementation. Approval must also assign permanent homes for the
adapter API crates and the schema source of truth.

Lane C must approve or replace the capability-disabled measured dual-core seam
before it adds contention. Lane H must approve the validated-output handoff.
Lane 0's receipt rebaseline must land before lane B adopts new execution costs.

The spike stops here for review as required by the lane brief.
