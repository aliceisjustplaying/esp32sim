# Draft decision: versioned Puck backend adapter and measured scheduler

Date: 2026-08-31

Status: **draft, unaccepted**. This record is a lane B review artifact. It does
not authorize implementation, product merge, a timing claim, or an accepted
cross-lane interface. If approved, it must be assigned a numbered decision
record in the Puck repository and updated with the maintainer's disposition.

## Context

Decision 0012 requires all product access to esp32sim to cross one Puck-owned
adapter and requires CPU-backend observation. Review finding F-033 also
requires virtual-time, deadline, event-order, and host-pacing semantics before
peripheral work.

The pinned code currently exposes `Machine` directly to the CLI and WebAssembly
wrappers. `Machine::run` is instruction-budgeted, device time is delivered
after fixed instruction quanta, CCOUNT advances once per completed block, and
the native JIT can access mapped memory through raw TLB pointers. Those are
valid fast-mode mechanisms but cannot define measured execution.

The [design-spike report](puck-measured-mode-adapter-spike.md) records the code,
test, fixture, and timing-evidence receipts behind this draft.

The proposal was written against esp32sim fork commit
`aa851249341e8cd122e7f4852d4c0f002e46d887`, upstream base
`2114ffc92039b4605264d2cfb4ee5543acbf98c1`, and Puck commit
`a91fddc9cb1629ee2de37d916468ee3eb8f681f7`. The normative cost-vocabulary
receipt is decision 0008 at SHA-256
`38f79a88675a59c43a887b6401571b133ee566438de26d5f6c332150e11d7214`.
The current profile hash is
`31a83ab4fe2253ef7ff5a0bcc944aa5c9ca38f90eef485f48f8f725fd790402a`.
The affine MMIO adoption hash is
`ac04584f3a05931795d65dc7246ae556202dd98bb7304cce06f50b5a29b0dc8a`.
The full consulted receipt inventory is in the spike report and is part of this
draft's review evidence.

## Proposed decision

Adopt a separately versioned Puck backend protocol and a separate measured
interpreter scheduler. Keep the existing fast `Machine::run` and JIT path
unchanged. Product code outside the esp32sim adapter imports neither `esp32s3`
nor `xtensa-lx7`.

The initial measured capability is single-core, interpreter-only, and
networking-off. Releasing core 1 returns `UnsupportedCapability` until lane C
defines a separately approved interleave and contention policy. JIT capability
for measured or observed execution remains false until the required
observation-conformance proof is present.

## Exact adapter interface proposed for version 1

The Rust spelling is normative for the first implementation. A WebAssembly C
ABI may represent the same values but must preserve their semantics and version
checks.

```rust
pub const ADAPTER_VERSION: ProtocolVersion = ProtocolVersion::new(1, 0, 0);
pub const EVENT_SCHEMA_VERSION: u16 = 1;
pub const LEDGER_SCHEMA_VERSION: u16 = 1;

pub type VirtualCycle = u64;

pub struct ProtocolVersion {
    pub major: u16,
    pub minor: u16,
    pub patch: u16,
}

pub struct ArtifactId(pub BoundedString);
pub struct SchedulerPolicyId(pub BoundedString);

pub trait BackendFactory {
    fn create(
        &self,
        config: BackendConfig,
        services: Box<dyn DeterministicServices>,
    ) -> Result<Box<dyn Backend>, BackendError>;
}

pub trait Backend {
    fn load(&mut self, artifacts: ArtifactSet) -> Result<LoadReceipt, BackendError>;
    fn reset(&mut self, kind: ResetKind) -> Result<ResetReceipt, BackendError>;
    fn run_until(&mut self, request: RunRequest) -> Result<RunSlice, BackendError>;
    fn inject(&mut self, event: InputEvent) -> Result<(), BackendError>;
    fn drain_events(&mut self, limit: DrainLimit) -> Result<EventBatch, BackendError>;
    fn inspect(
        &mut self,
        permit: &DebugPermit,
        range: GuestRange,
        max_bytes: u32,
    ) -> Result<OwnedGuestBytes, BackendError>;
    fn capabilities(&self) -> Capabilities;
    fn close(&mut self) -> Result<(), BackendError>;
}

pub struct BackendConfig {
    pub requested_adapter: ProtocolVersion,
    pub requested_event_schema: u16,
    pub chip: ChipConfig,
    pub board: BoardConfig,
    pub identity: ChipIdentity,
    pub execution: ExecutionConfig,
    pub networking: NetworkPolicy,
    pub quotas: Quotas,
    pub deterministic_seed: [u8; 32],
    pub trust: TrustClass,
}

pub enum ExecutionConfig {
    Fast { engine: FastEngine },
    Measured {
        engine: MeasuredEngine,
        timing_manifest: ArtifactId,
        scheduler_policy: SchedulerPolicyId,
    },
}

pub enum FastEngine { Interpreter, JitIfProven }
pub enum MeasuredEngine { Interpreter }
pub enum NetworkPolicy { None, Transcript(ArtifactId), LiveOptIn }

pub struct ArtifactSet {
    pub artifacts: Vec<ImmutableArtifact>,
}

pub struct ImmutableArtifact {
    pub id: ArtifactId,
    pub kind: ArtifactKind,
    pub sha256: [u8; 32],
    pub bytes: Box<[u8]>,
}

pub enum ArtifactKind {
    MaskRomElf,
    Bootloader,
    PartitionTable,
    Application,
    SymbolsElf,
    FlashImage,
    InputTranscript,
    TimingManifest,
}

pub enum ResetKind { PowerOn, Software, Watchdog, External }

pub struct LoadReceipt {
    pub artifact_hashes: Vec<(ArtifactId, [u8; 32])>,
    pub artifact_set_hash: [u8; 32],
}

pub struct ResetReceipt {
    pub epoch: u64,
    pub cycle: VirtualCycle,
    pub kind: ResetKind,
}

pub struct RunRequest {
    pub deadline: VirtualCycle,
    pub budget: RunBudget,
}

pub struct RunBudget {
    pub max_cycles: u64,
    pub max_instructions: u64,
    pub max_wall_millis: u32,
    pub max_output_events: u32,
    pub max_output_bytes: u32,
    pub max_ledger_entries: u32,
}

pub struct RunSlice {
    pub epoch: u64,
    pub start_cycle: VirtualCycle,
    pub end_cycle: VirtualCycle,
    pub instructions: [u64; 2],
    pub stop: RunStop,
    pub ledger: LedgerDelta,
}

pub enum RunStop {
    DeadlineReached,
    DeadlineBoundary { next_completion: VirtualCycle },
    BudgetExhausted { budget: BudgetKind },
    Idle { next_event: Option<VirtualCycle> },
    ResetRequested { kind: ResetKind },
    GuestStop(GuestStop),
    TimingBlocked(TimingBlock),
    QuotaExceeded { quota: QuotaKind },
    UnsupportedCapability { capability: CapabilityId },
}

pub struct TimingBlock {
    pub event_sequence: u64,
    pub instruction_sequence: Option<u64>,
    pub tier_candidate: CostTierName,
    pub reason: BoundedString,
    pub source: BoundedString,
}

pub enum CostTierName { Exact, Affine, Interval, Distribution, Unexplained }

pub struct InputEvent {
    pub schema_version: u16,
    pub epoch: u64,
    pub cycle: VirtualCycle,
    pub sequence: u64,
    pub payload: InputPayload,
}

pub struct BackendEvent {
    pub schema_version: u16,
    pub epoch: u64,
    pub cycle: VirtualCycle,
    pub sequence: u64,
    pub payload: EventPayload,
}

pub struct DrainLimit {
    pub max_events: u32,
    pub max_bytes: u32,
}

pub struct EventBatch {
    pub events: Vec<BackendEvent>,
    pub remaining_events: u32,
    pub remaining_bytes: u32,
}
```

`InputPayload` initially permits timestamped GPIO, touch, serial, and reset
requests using owned bounded values. `EventPayload` initially permits validated
serial, framebuffer, audio, GPIO, diagnostic, reset, and guest-stop values.
Timing observations live in `LedgerDelta`, not in human-readable diagnostics.
Every payload variant has an explicit byte bound in `Quotas`. The shared Puck
guest-output validator runs before output is queued.

`DeterministicServices` supplies immutable artifact lookup and transcript input.
It does not expose host wall time, filesystem paths, environment variables,
sockets, or random generation to measured execution. The wall budget is a host
cancellation check only. Cancellation returns at an architectural boundary and
does not advance `end_cycle`.

`create` rejects incompatible protocol majors or event schemas. The lane B
spike also rejects measured mode unless `MeasuredEngine::Interpreter`,
`NetworkPolicy::None`, and the policy ID `lane-b-single-core-v1` are requested.
`load` recomputes every artifact hash and enforces kind-specific and aggregate
size quotas before mutating the backend.

`close` is repeatable at the host-wrapper boundary. After its first successful
call, all methods except `capabilities` and `close` return `Closed`.

## Stable error contract

```rust
pub struct BackendError {
    pub code: ErrorCode,
    pub message: BoundedString,
}

pub enum ErrorCode {
    IncompatibleVersion,
    InvalidConfig,
    InvalidState,
    InvalidArtifact,
    ArtifactHashMismatch,
    QuotaExceeded,
    UnsupportedCapability,
    EventInPast,
    EventSequence,
    InspectionDenied,
    GuestFault,
    BackendFault,
    Closed,
}
```

Messages are diagnostics and are not used for control flow. Error codes,
`RunStop`, normalized events, and ledger entries are deterministic inputs to
tests.

## Capability contract

`capabilities()` returns owned values containing:

- adapter, event, and ledger schema versions;
- backend name, esp32sim commit, and Puck adapter commit;
- chip and board identifiers;
- interpreter, native JIT, and WebAssembly JIT availability;
- active engine and trust class;
- network policies, with live egress false for untrusted and measured modes;
- snapshot support, initially false;
- privileged inspection support;
- measured single-core and measured dual-core support, with dual-core false in
  lane B;
- timing-manifest hash and receipt-set hash;
- observation proof status and its corpus and result hashes.

A caller may request only an advertised capability. Capability absence is a
typed refusal and never selects a fallback silently.

## Proposed event order and scheduler contract

The measured scheduler owns one monotonic `u64` virtual-cycle counter per reset
epoch. `reset` increments the epoch and sets its virtual cycle to zero. CCOUNT
is a wrapping `u32` projection advanced by committed virtual-cycle deltas.

At one virtual cycle, events are ordered by this stable key:

1. reset request;
2. internal device completion and interrupt assertion;
3. injected input in caller-supplied sequence order;
4. CPU architectural boundary, core ID order;
5. validated output and diagnostic emission in producer sequence order.

The initial measured policy permits core 0 only. Core ID order is specified so
the schema and fake tests do not need revision when lane C adds an approved
dual-core policy. It is not approval of a dual-core interleave or contention
model.

`inject` rejects an epoch mismatch, a cycle earlier than `virtual_now`, and a
non-increasing caller sequence. Inputs are not applied until the scheduler
reaches their cycle.

`run_until` never changes its requested deadline. It returns with
`end_cycle <= deadline`. If the next instruction would complete after the
deadline, it returns `DeadlineBoundary` with that completion cycle and commits
neither architecture nor time for that instruction. A later request with a
sufficient deadline may execute it. If all CPUs are idle, time advances to the
minimum of the run deadline, next injection, and next known device deadline.

Before each instruction, the measured interpreter stages every possible access
and exact timing effect without a guest-visible side effect. It executes and
commits only when every required duration is deterministically resolved. An
access shape that cannot be staged returns `TimingBlocked` before the
instruction and changes no machine or timing state. The immutable artifact set
means reset or a new backend with a newly approved manifest is required before
progress.

Every active time-aware device returns `At(cycle)`, `None`, or `Unknown`.
`None` proves there is no autonomous change. `Unknown` stops advancement.
Device time is delivered through the current virtual cycle before MMIO. Batches
may not cross a declared device deadline. Scheduling output is invariant under
different `run_until` partitions.

Host pacing observes virtual time after a slice. It may delay the next host
call but cannot change event order, guest inputs, virtual cycles, or ledger
contents.

## Proposed ledger and timing-claim contract

```rust
pub struct LedgerDelta {
    pub schema_version: u16,
    pub epoch: u64,
    pub start_cycle: VirtualCycle,
    pub end_cycle: Option<VirtualCycle>,
    pub entries: Vec<LedgerEntry>,
    pub status: LedgerStatus,
    pub hash: [u8; 32],
}

pub struct LedgerEntry {
    pub sequence: u64,
    pub core: Option<u8>,
    pub instruction_sequence: Option<u64>,
    pub kind: TimingEventKind,
    pub start_cycle: VirtualCycle,
    pub end_cycle: Option<VirtualCycle>,
    pub claim: CostClaim,
}

pub enum LedgerStatus {
    Complete,
    Blocked(TimingBlock),
}

pub struct CostClaim {
    pub claim_id: ClaimId,
    pub tier: CostTier,
    pub receipt: ReceiptRef,
    pub resolved_cycles: Option<u64>,
}

pub enum CostTier {
    Exact { cycles: u64 },
    Affine {
        slope: i64,
        intercept: i64,
        measured_counts: Vec<u64>,
        cohort: CohortKey,
    },
    Interval { min: u64, max: u64, cause: BoundedString },
    Distribution { quantiles: Quantiles, cause: BoundedString },
    Unexplained { reason: BoundedString },
}

pub struct ReceiptRef {
    pub repository: BoundedString,
    pub commit: CommitId,
    pub path: BoundedString,
    pub sha256: [u8; 32],
    pub firmware_sha256: [u8; 32],
    pub sdkconfig_sha256: [u8; 32],
    pub toolchain: ToolchainIdentity,
    pub board_revision: BoundedString,
    pub adoption: AdoptionStatus,
}
```

`resolved_cycles` is present only when a deterministic event-scoped resolver
maps the claim to this occurrence. An interval or distribution claim therefore
does not imply sampling. Its accepted deterministic phase model, if any, is
part of the hashed timing manifest. `Unexplained` never resolves.

Affine claims retain slope, intercept, measured cell sizes, and cohort scope.
The committed same-value MMIO evidence is `3n - 8`. The current schema-1
profile's scalar `3` is not an executable replacement for that claim. The
initial importer rejects it for measured totals, and the online scheduler
blocks until an event-scoped resolver is reviewed. It does not distribute or
discard the negative intercept.

Unknown cost blocks the ledger total. No ledger hash is described as complete
when `LedgerStatus::Blocked` is present. Same normalized trace plus the same
timing-manifest hash must produce the same ledger bytes and hash.

## CPU observation contract

The CPU backend emits the canonical stream. It includes instruction start and
commit, exact fetch, load, store, atomic and MMIO accesses, faults, exceptions,
interrupt assertions and acceptance, code invalidations, and timing claims.
The bus supplies resolution facts but cannot claim completeness.

The interpreter is mandatory and is the semantic oracle. A JIT observation
capability is enabled only when a mandatory committed corpus covers RAM,
flash, MMIO, faults, self-modifying code, and cross-page accesses in interpreter,
JIT slow-memory, and JIT fast-memory modes. The normalized observation stream,
architectural state, memory state, and invalidation state must match. Missing
corpus, zero cases, or hash mismatch disables the capability.

## Decision-0008 receipt rules

Every cost is loaded from a hash-pinned accepted manifest. Unknowns carry a
tier candidate and block. The timing importer rejects a claim when its receipt,
firmware, sdkconfig, toolchain, board revision, or adoption status does not
match the manifest.

Lane 0 owns the ESP-IDF 6.1 rebaseline and the no-mixing boundary. Lane B owns
the tiered importer and exact event mapping after this draft is approved. The
35-cycle window pair and `+1` loop-alignment result remain unknown until lane 0
lands their 6.1 adoption disposition. Lane B requests lane E evidence only if
an existing committed receipt cannot resolve an event mapping.

## Consequences if accepted

- Fast mode keeps its current run loop, block layouts, CCOUNT behavior, and JIT
  inner path with no measured bookkeeping.
- Measured mode starts as interpreter-only, single-core, networking-off, and
  fail-closed on unknown device deadlines or costs.
- The adapter API and event and ledger schemas become independently versioned
  Puck interfaces with shared fake-backend contract tests.
- Lane C must propose the measured dual-core policy before enabling that
  capability.
- Lane H supplies the shared validated-output boundary used before queueing
  adapter events.
- Snapshot capability stays absent until a complete versioned state design is
  approved.
- Schema-1 `timing.json` cannot support a measured total because it lacks
  decision-0008 tier structure and loses the affine MMIO intercept.

## Approval checklist

Approval must name the permanent repository and crate locations, assign the
numbered Puck decision record, accept or amend the exact scheduler ordering,
and accept or amend the single-core capability boundary. It must also record
the owner of timing-profile schema version 2 and the lane H validator handoff.

Until that happens, this document remains a design-spike artifact only.
