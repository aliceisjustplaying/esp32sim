# Lane B design spike: browser emulator measured execution and adapter

Date: 2026-08-31

Status: ready for maintainer review. This is a throwaway design-spike
deliverable. It specifies interfaces against esp32sim commit
`aa851249341e8cd122e7f4852d4c0f002e46d887`, whose upstream code is pinned at
`2114ffc92039b4605264d2cfb4ee5543acbf98c1`. It contains no product
implementation and authorizes no merge.

The companion
[decision-record draft](browser-emulator-adapter-scheduler-decision-draft.md)
is explicitly unaccepted. The adapter protocol, event schema, timing
vocabulary, and scheduler semantics are cross-lane interfaces. They require
maintainer approval before implementation or merge.

## Result

The browser-hosted cycle-accurate ESP32-S3 emulator is feasible with two
separate seams in our esp32sim fork:

1. A versioned Rust backend adapter is the only machine-facing product surface.
   It owns virtual time, bounded typed events, quotas, artifacts, primary guest
   output validation, adopted chip identity, explicit boot modes, cumulative
   ledger chaining, capabilities, browser marshalling, and stable errors.
2. A measured interpreter path in the CPU backend owns instruction and memory
   observation. It prices an instruction before its next architectural
   boundary, advances CCOUNT and device time from the same cycle ledger, and
   emits the normalized ledger the adapter returns.

Measured execution cannot be implemented as a `Bus` wrapper or as a price pass
over a completed trace. The current JIT bypasses `Bus` for fast memory, and the
current scheduler advances device time after execution quanta. The measured
path must therefore be a separate interpreter backend selected outside the
existing fast inner loops. Fast mode remains unchanged. The esp32sim fork owns
the web UI shell. Puck is the donor, evidence, differential, and decision
repository for UI, recorder and replay, differential harness, timing model,
receipts, and reusable browser pieces. It is not the cycle-accurate product or
the home of a second execution engine.

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
| `wasm/src/lib.rs:176-204` | `esp32sim_run(cycles, unix_ms)` sets `max_cycles` and then calls the instruction-budgeted machine loop. Host Unix time is accepted directly. | The fork-owned browser adapter accepts an absolute virtual deadline and an explicit deterministic input transcript. It does not accept host time as simulated time. |
| `cli/src/main.rs:81-102,116` | Networking is created only when `--wifi` is supplied. `--no-jit` disables both native block caches. | The spike configuration requires interpreter execution and `NetworkPolicy::None`; incompatible configuration is rejected during `create`. |
| `cli/src/main.rs:140-179` and `esp32s3/src/periph.rs:798-823,1010-1028` | The CLI overlays text-parsed efuse and reset-register dumps, then assigns strap and reset-cause values. `Machine::new` otherwise synthesizes MAC and revision defaults. | The adapter requires hash-pinned canonical raw efuse and reset-register artifacts plus the exact strap word. It replaces efuse state, validates every reset-state record, and reports one identity-set hash. |
| `cli/src/main.rs:120-139` and `esp32s3/src/machine.rs:85-173` | ROM boot loads an ELF and starts at reset, while app boot installs synthetic second-stage state and jumps through `boot_app`. Individually supplied bootloader, partition, and app files are copied into flash. | Product boot requires the exact real mask-ROM ELF and one exact complete flash image. Direct application is a separately advertised diagnostic capability and cannot satisfy a product-boot claim. |

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
It also confirms only the HLE direct-application path. It is not a real-ROM
product-boot, timing, identity, or firmware-correctness claim.

`cargo test --workspace` also exited zero under Rust and Cargo 1.98.0. Existing
warnings are unchanged from the pinned base.

## Proposed internal layering

| Layer | Owns | May import |
| --- | --- | --- |
| Fork Rust `backend-api` crate | Adapter types, protocol and event versions, stable errors, quota types, fake-backend contract suite | No machine internals |
| Fork Rust `browser-backend` crate | Deep adapter implementation, bounded artifact loader, primary guest-output validation, event queues, WebAssembly browser interface | `backend-api`, `esp32s3`, `timing-model` |
| Fork Rust `timing-model` crate | Receipt-pinned manifest importer, cost claims, ledger, normalized trace comparison | `backend-api`; no Puck TypeScript |
| `esp32s3` measured scheduler module | Virtual-cycle advancement, CPU selection, device deadlines, interrupt sampling | `xtensa-lx7` measured CPU interface, SoC devices, `timing-model` |
| `xtensa-lx7` measured interpreter module | Complete CPU observations, instruction pricing seam, pending instruction state, CCOUNT batching | CPU decode and interpreter internals, a timing-source interface supplied by the SoC |
| Existing fast machine and JIT | Upstream fast behavior | Existing internals only |
| Fork-owned thin web UI shell | UI and transport over the versioned Wasm interface; selected Puck browser pieces may be ported with provenance | No Rust machine internals and no execution engine |

The dependency rule is mechanical: browser TypeScript imports only generated
wire types and the versioned Wasm interface. Only the fork's `browser-backend`
crate may construct product-facing values from `esp32s3` internals. The primary
quota and guest-output validation occurs in Rust before `BackendEvent`
construction. Browser TypeScript rechecks already-owned bounded events only as
thin-client defense. The fake backend depends only on `backend-api`.

## Measured CPU transaction

One measured instruction is one architectural transaction:

1. Decode from the separate measured block cache and plan the instruction's
   exact encoding, width, fetch, every possible data-access shape, receipt
   matches, and nonnegative deterministic cost.
2. If any cost, access, or state-sensitive match key cannot be resolved, append
   the blocking attempt and stop before the instruction. CPU, bus, timing,
   device, CCOUNT, and virtual-time state do not change.
3. Persist a `PendingInstruction` containing the decoded operation, start and
   completion cycles, access dependencies, staged timing mutations, and receipt
   references. Fetch is observed at its priced phase. Version 1 captures
   effective address and CPU operands at start, but performs no data access or
   preview then.
4. Advance toward completion in scheduler segments. Each segment advances
   virtual time and CCOUNT and delivers any CCOMPARE match, device deadline, and
   injected event at its exact cycle. A run deadline or cycle budget may leave
   the instruction pending without an architectural commit.
5. At completion, process reset, device and DMA, and input events before the CPU
   boundary. Reset discards the pending instruction. Otherwise perform the data
   access exactly once against the resulting state, then commit architectural
   effects, timing state, and ledger entries atomically and increment the
   completed-instruction count once.
6. Consider an asserted interrupt for acceptance only after the completing CPU
   boundary and before the next instruction starts.

The persistent transaction keeps timing-driven execution from running ahead and
pricing a finished trace while preserving state across arbitrary `run_until`
partitions. The exact data-access phase is
`CompletionAfterSameCycleExternalEvents`: a load observes an earlier or
same-cycle DMA write, a store becomes visible after those external events, and
an atomic access is indivisible at the CPU boundary. No load value, fault, MMIO
return, match field, or side effect may be cached from preflight.

The plan declares guest-range, MMU, cache, bus-route, device-state, fault,
receipt-match, value, and side-effect dependencies. Device, DMA, and input
events declare impacts in the same vocabulary. If the backend cannot defer the
real access to completion, or an intervening impact can alter timing without a
receipt-backed invariant resolver, the instruction blocks before starting.
Injection into an existing pending interval is rejected when its impact is
unknown or incompatible. A previously unknowable conflicting device impact
blocks at its exact event prefix, leaves the instruction terminally pending,
and requires reset or a stronger reviewed model before progress.

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
    Unknown {
        device: DeviceId,
        reason: DeviceDeadlineUnknown,
    },
}

struct DeviceDeadlineUnknown {
    code: DeviceDeadlineUnknownCode,
    detail: DiagnosticString,
}

enum DeviceDeadlineUnknownCode {
    NoDeadlineModel,
    UnsupportedActivePath,
    UnresolvedExternalClock,
    UnresolvedDmaCompletion,
    UnresolvedSharedResource,
}
```

`None` means the device proves it cannot change state without a guest access or
an injected event. It does not mean that no deadline implementation exists.
`Unknown` blocks measured time advancement and is recorded as an unexplained
tier candidate.

The measured scheduler advances from `virtual_now` to the earliest run,
injection, device, CCOMPARE, or pending-instruction phase boundary. Devices may
receive a batch only when their declared next deadline is not crossed. A
peripheral read or write first delivers all device time through `virtual_now`;
a write that arms a deadline is then visible when the scheduler recomputes the
minimum. The code and bounded detail for `Unknown` are stable test values, and
the string is never used for control flow. Run partitioning must be inert: one
`run_until(1000)` call and ten calls ending at 100, 200, through 1000 produce
the same state, events, and cumulative ledger `chain_after` at every common
entry sequence, even when a deadline lies inside a priced instruction.

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

At the first crossed CCOMPARE, the batch splits at the exact matching virtual
cycle, including a match inside a pending instruction. CCOUNT and device time
advance to that cycle, the timer interrupt asserts, and the instruction remains
pending until its completion boundary. It cannot accept the interrupt before
that boundary. Reads and writes of CCOUNT and CCOMPARE remain block-first and
flush any pending aggregate before execution.

Idle advancement uses the same virtual-cycle delta for the running core's
CCOUNT. Measured dual-core scheduling remains capability-disabled in lane B;
lane C must define how both core-local counters advance under its accepted
interleave and contention policy.

## Cumulative ledger chain

`LedgerDelta.entries` contains only entries emitted by one `run_until` call, so
its `delta_entries_hash` is transport integrity and may change with run slicing.
The run identity is a separate cumulative chain. Its epoch seed hashes ledger
schema, epoch, artifact set, adopted identity set, boot set, timing manifest,
and receipt set. Each globally sequenced canonical entry extends the prior hash
with a domain tag and length-prefixed entry bytes. Slice start, end, and grouping
are excluded. Deltas return `chain_before`, `chain_after`, and cumulative entry
count. Empty deltas leave the chain unchanged.

A blocked ledger hashes the canonical `TimingBlock` with the known-prefix chain
as `ledger_state_hash` but does not claim a complete total. Therefore arbitrary
run partitions can have different delta hashes while producing the same ordered
entry stream and cumulative hash at every shared sequence. The decision draft
defines the exact domain strings and byte ordering.

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
  mode, interpreter engine, and `NetworkPolicy::None` only. It also names one
  hash-pinned adopted efuse image, strap word, reset-register state, and boot
  mode.
- `load` validates an artifact manifest and all declared and aggregate quotas
  before allocation, then streams bounded chunks and verifies actual size,
  explicit EOF, and SHA-256 before atomic commit. Product ROM-flash boot
  requires the real mask-ROM ELF and exact complete flash image. HLE direct-app
  boot is a distinct non-product capability.
- the first `reset` must be power-on and applies the verified raw identity and
  selected boot. Later software, watchdog, external, or power-on reset starts a
  new epoch under the specified persistence rules. Partial snapshot support is
  reported absent.
- `run_until` accepts an absolute virtual deadline and independent cycle,
  instruction, wall-cancellation, output, and ledger budgets. Cycle limits may
  pause a persistent instruction. Instruction limits withhold only the CPU
  commit and never partially commit architecture. The wall budget never changes
  simulated time.
- `inject` accepts a timestamped owned event no earlier than `virtual_now` and
  rejects an unknown or conflicting impact inside a pending instruction.
- `drain_events` returns owned, typed, bounded events with no guest pointers.
- `inspect` requires an explicit debug capability and a maximum byte count.
- `capabilities` reports adapter and event versions, backend commit, engine,
  networking, snapshot, board, ROM-flash and direct-app boot support, active
  identity and boot hashes, measured-dual-core, receipt manifest, and JIT
  observation-proof support.
- `close` is deterministic and leaves no worker, socket, timer, or executable
  mapping.

The adapter event queue is lossless in the initial contract. Reaching its byte
or count quota stops execution with `QuotaExceeded`; it does not drop or
coalesce silently. The Rust adapter checks declared sizes, guest ranges, queue
capacity, and exact canonical encoded length before allocating owned payload
bytes or constructing `BackendEvent`. Lane H reviews this primary seam. Browser
TypeScript only rechecks the already-owned bounded event.

The efuse image is exactly 128 little-endian words replacing addresses
`0x60007000` through `0x600071fc`. The reset-state artifact is a canonical,
sorted, duplicate-free list of address and value pairs, and every pair must be
accepted by `Peripherals::init_regs`; none is ignored. The adapter verifies the
base MAC decoded from raw efuses, pins the lane A/E adoption receipt, and hashes
revision metadata, MAC, strap, raw efuse, reset state, and receipt into one
identity-set hash. This complete tuple is the A-01/E identity handoff; an efuse
digest alone is not sufficient.

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

- version negotiation and every hard limit at maximum and maximum plus one;
- bounded UTF-8, NUL, canonical event-length, unknown-tag, and trailing-byte
  rejection;
- artifact count, per-kind size, aggregate size, hash, early EOF, late EOF,
  oversized chunk, non-sequential chunk, and Wasm pointer-overflow rejection,
  with an allocation spy proving rejection precedes allocation;
- exact 512-byte efuse replacement, decoded-MAC mismatch, strap application,
  reset-register ordering, duplicate, unaligned, disallowed-address, artifact
  hash, and identity-set hash cases;
- ROM-flash boot requiring the named real mask-ROM ELF and an exact flash-sized
  image at offset zero, absent ROM reset-vector rejection, and direct-app
  capability, fixed offset, and non-product receipt labeling;
- every reset kind and reset-epoch sequencing;
- absolute deadline behavior, zero budgets, cycle and instruction crossing, and
  run-partition invariance with a persistent pending instruction;
- same-cycle reset, device, injected input, interrupt, and CPU-boundary ordering;
- pending load, store, atomic, and MMIO access across DMA, device, and injected
  input changes, including completion-phase value sampling, impact-safe passage,
  injected conflict rejection, and terminal fail-closed unknown impact;
- CCOMPARE and device deadlines at instruction start, strictly inside, and at
  completion, including CCOUNT wrap;
- input, event, ledger, and runtime-memory quotas with stable typed errors;
- typed bounded device-deadline unknowns for every reason code;
- privileged and denied memory inspection, guest-range addition overflow,
  unmapped range, scope mismatch, byte-limit crossing, and use of a permit with
  the wrong backend instance;
- close idempotence at the host wrapper and resource cleanup;
- one known exact ledger and one unknown that blocks without a total;
- one large run and many partitions producing different delta groupings but the
  same canonical entry sequence and cumulative chain at every shared sequence;
- timing manifest and receipt hash mismatch;
- networking absent and JIT absent from measured capabilities.

Measured CPU tests then cover CCOUNT wrap, all three CCOMPARE registers,
deadline crossing inside a priced instruction, MMIO flush-before-access,
self-modifying invalidation, cache line-fill sequencing, loop alignment,
window traps, dependent load-use, and unsupported-shape refusal.

The fork's Rust timing model and the donor Puck TypeScript timing machine
consume the same normalized trace fixture and must emit the same event order,
tier labels, receipt identities, blocked event, and known ledger total. Same
trace and same manifest must produce the same ordered canonical entries and
cumulative ledger chain in repeated runs.

## Review gates

Maintainer approval is required for the companion decision draft before any
product implementation. The recommendation is that `backend-api`,
`browser-backend`, and `timing-model` are Rust crates in our esp32sim fork;
measured scheduler and interpreter observation remain modules in `esp32s3` and
`xtensa-lx7`; the Rust browser adapter is the schema source of truth, and the
fork owns the thin web UI shell. Approval must accept or amend those homes. No
deep adapter or execution module is recommended for Puck TypeScript. Puck
`docs/decisions` is the recommended home for the accepted cross-lane record
because Puck remains the evidence and decision repository.

Lane C must approve or replace the capability-disabled measured dual-core seam
before it adds contention. Lane H reviews the Rust validated-output seam. Lane
0's receipt rebaseline must land before lane B adopts new execution costs. The
unaccepted decision draft also carries exact proposed text for amending decision
0011's role of Puck, decision 0012's Puck-owned adapter wording, and the roadmap.

The spike stops here for review as required by the lane brief.
