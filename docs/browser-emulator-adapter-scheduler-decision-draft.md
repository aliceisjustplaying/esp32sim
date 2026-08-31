# Draft decision: browser emulator adapter and measured scheduler

Date: 2026-08-31

Status: **draft, unaccepted**. This record is a lane B review artifact. It does
not authorize implementation, product merge, a timing claim, or an accepted
cross-lane interface. If approved, it must be assigned a numbered decision
record in Puck's `docs/decisions`, the recommended decision and evidence home,
and updated with the maintainer's disposition.

## Context

Decision 0012 currently requires all product access to esp32sim to cross one
Puck-owned adapter and requires CPU-backend observation. The maintainer has
since identified the product as a browser-hosted cycle-accurate ESP32-S3
emulator built from our esp32sim fork. The accepted decision's ownership phrase
therefore requires an explicit amendment before implementation. Review finding
F-033 also
requires virtual-time, deadline, event-order, and host-pacing semantics before
peripheral work.

The pinned code currently exposes `Machine` directly to the CLI and WebAssembly
wrappers. `Machine::run` is instruction-budgeted, device time is delivered
after fixed instruction quanta, CCOUNT advances once per completed block, and
the native JIT can access mapped memory through raw TLB pointers. Those are
valid fast-mode mechanisms but cannot define measured execution.

The [design-spike report](browser-emulator-measured-mode-adapter-spike.md)
records the code,
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

Adopt a separately versioned backend protocol and a separate measured
interpreter scheduler as Rust modules in the esp32sim fork. Keep the existing
fast `Machine::run` and JIT path unchanged. The esp32sim fork owns the web UI
shell and its thin TypeScript transport. Puck remains the donor, evidence,
differential, and decision repository. It does not own an execution engine or a
second substantial adapter implementation.

The proposed permanent homes are fork workspace crates `backend-api` for the
versioned interface and contract suite, `browser-backend` for the deep machine
adapter, bounded loader, browser interface, queue and primary validator, and
`timing-model` for the manifest importer and ledger. Measured scheduler and CPU
observation stay as modules in the existing `esp32s3` and `xtensa-lx7` crates.
The Rust `backend-api` schema is the source of truth for generated browser wire
types, and the fork's `web/` tree owns the UI shell.

The initial measured capability is single-core, interpreter-only, and
networking-off. Releasing core 1 returns `UnsupportedCapability` until lane C
defines a separately approved interleave and contention policy. JIT capability
for measured or observed execution remains false until the required
observation-conformance proof is present.

The product scope is the complete ESP32-S3 SoC plus the exact Waveshare
TinyDraw V2 board. It includes SRAM, flash, octal PSRAM, caches and MMU, DMA and
peripherals, and eventually the CO5300 panel with GRAM, TE and scan-out, CST820
touch, QMI8658, PCF85063A, and TCA9554. Radio and SoC blocks omitted from the
first milestone are deferred, not excluded from product scope. The first useful
milestone is real TinyDraw V2 firmware boot, draw, and touch in the browser.

## Exact adapter interface proposed for version 1

The Rust spelling is normative for the first implementation. A WebAssembly C
ABI may represent the same values but must preserve their semantics and version
checks.

```rust
pub const ADAPTER_VERSION: ProtocolVersion = ProtocolVersion::new(1, 0, 0);
pub const EVENT_SCHEMA_VERSION: u16 = 1;
pub const LEDGER_SCHEMA_VERSION: u16 = 1;

pub const MAX_IDENTIFIER_BYTES: usize = 128;
pub const MAX_DIAGNOSTIC_BYTES: usize = 1_024;
pub const MAX_RECEIPT_PATH_BYTES: usize = 1_024;
pub const MAX_DEBUG_RANGES: usize = 64;
pub const MAX_AFFINE_COUNTS: usize = 256;
pub const MAX_ARTIFACTS: usize = 16;
pub const MAX_ARTIFACT_MANIFEST_BYTES: usize = 64 * 1_024;
pub const MAX_ARTIFACT_CHUNK_BYTES: usize = 64 * 1_024;
pub const MAX_TOTAL_ARTIFACT_BYTES: u64 = 128 * 1_024 * 1_024;
pub const MAX_RUNTIME_BYTES: u64 = 512 * 1_024 * 1_024;
pub const MAX_QUEUED_INPUT_EVENTS: u32 = 4_096;
pub const MAX_QUEUED_INPUT_BYTES: u32 = 16 * 1_024 * 1_024;
pub const MAX_QUEUED_OUTPUT_EVENTS: usize = 4_096;
pub const MAX_QUEUED_OUTPUT_BYTES: u32 = 64 * 1_024 * 1_024;
pub const MAX_SINGLE_OUTPUT_BYTES: u32 = 2 * 1_024 * 1_024;
pub const MAX_SERIAL_PAYLOAD_BYTES: usize = 4 * 1_024;
pub const MAX_FRAME_PAYLOAD_BYTES: usize = 2 * 1_024 * 1_024;
pub const MAX_AUDIO_PAYLOAD_BYTES: usize = 2 * 1_024 * 1_024;
pub const MAX_LEDGER_ENTRIES_PER_RUN: usize = 65_536;
pub const MAX_INSPECT_BYTES: usize = 1 * 1_024 * 1_024;
pub const MAX_RUN_CYCLES: u64 = 1 << 32;
pub const MAX_RUN_INSTRUCTIONS: u64 = 10_000_000;
pub const MAX_RUN_WALL_MILLIS: u32 = 60_000;
pub const EFUSE_REGISTER_IMAGE_BYTES: u64 = 128 * 4;
pub const MAX_RAW_RESET_REGISTER_CAPTURE_BYTES: u64 = 16 * 1_024 * 1_024;
pub const MAX_RESET_REGISTER_RECORDS: usize = 4_096;
pub const MAX_RESET_REGISTER_STATE_BYTES: u64 = 64 * 1_024;
pub const RESET_REGISTER_FILTER_RECEIPT_BYTES: u64 = 152;
pub const DIRECT_APP_FLASH_OFFSET: u32 = 0x0001_0000;

pub type VirtualCycle = u64;

// The field is private. `new` rejects non-UTF-8, NUL, and a byte length over
// MAX. Length is always the UTF-8 byte length, not a character count.
pub struct BoundedString<const MAX: usize>(Box<str>);
pub type Identifier = BoundedString<MAX_IDENTIFIER_BYTES>;
pub type DiagnosticString = BoundedString<MAX_DIAGNOSTIC_BYTES>;
pub type ReceiptPath = BoundedString<MAX_RECEIPT_PATH_BYTES>;

// The field is private. `new` rejects a byte length over MAX.
pub struct BoundedBytes<const MAX: usize>(Box<[u8]>);

// The field is private. `new` rejects a count over MAX.
pub struct BoundedList<T, const MAX: usize>(Vec<T>);

pub enum BoundsErrorCode { TooLong, TooMany, InvalidUtf8, InteriorNul }
pub struct BoundsError {
    pub code: BoundsErrorCode,
    pub actual: u64,
    pub maximum: u64,
}

impl<const MAX: usize> BoundedString<MAX> {
    pub fn try_copy_from_utf8(bytes: &[u8]) -> Result<Self, BoundsError>;
    pub fn as_str(&self) -> &str;
}

impl<const MAX: usize> BoundedBytes<MAX> {
    pub fn try_copy_from(bytes: &[u8]) -> Result<Self, BoundsError>;
    pub fn as_bytes(&self) -> &[u8];
}

impl<T, const MAX: usize> BoundedList<T, MAX> {
    pub fn try_from_iter(items: impl IntoIterator<Item = T>)
        -> Result<Self, BoundsError>;
    pub fn as_slice(&self) -> &[T];
}

pub struct ProtocolVersion {
    pub major: u16,
    pub minor: u16,
    pub patch: u16,
}

pub struct ArtifactId(pub Identifier);
pub struct SchedulerPolicyId(pub Identifier);
pub struct CommitId(pub BoundedString<64>);
pub struct ClaimId(pub Identifier);
pub struct CohortKey(pub Identifier);

pub enum TrustClass { LocalTrusted, Untrusted }
pub enum ActiveEngine { FastInterpreter, FastJit, MeasuredInterpreter }
pub enum ChipModel { Esp32S3 }

pub struct ChipConfig {
    pub model: ChipModel,
    pub core_count: u8,
    pub cpu_frequency_hz: u32,
    pub flash_bytes: u32,
    pub psram_bytes: u32,
}

pub struct BoardConfig {
    pub id: Identifier,
    pub revision: Identifier,
}

pub struct ChipIdentity {
    pub revision: ChipRevision,
    pub expected_base_mac: [u8; 6],
    pub efuse_registers: HashPinnedArtifact,
    pub strap_word: u32,
    pub raw_reset_register_capture: HashPinnedArtifact,
    pub reset_register_applied_subset: HashPinnedArtifact,
    pub reset_register_filter_receipt: HashPinnedArtifact,
    pub adoption_receipt_sha256: [u8; 32],
}

pub struct ChipRevision { pub major: u8, pub minor: u8 }

pub struct HashPinnedArtifact {
    pub id: ArtifactId,
    pub sha256: [u8; 32],
}

pub trait BackendFactory {
    fn create(
        &self,
        config: BackendConfig,
        services: Box<dyn DeterministicServices>,
    ) -> Result<CreatedBackend, BackendError>;
}

pub struct CreatedBackend {
    pub backend: Box<dyn Backend>,
    pub debug_permit: Option<DebugPermit>,
}

pub trait Backend {
    fn load(&mut self, manifest: ArtifactManifest) -> Result<LoadReceipt, BackendError>;
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
    pub boot: BootMode,
    pub execution: ExecutionConfig,
    pub networking: NetworkPolicy,
    pub quotas: Quotas,
    pub deterministic_seed: [u8; 32],
    pub trust: TrustClass,
    pub inspection: InspectionPolicy,
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

pub enum BootMode {
    RomFlash {
        mask_rom: HashPinnedArtifact,
        flash_image: HashPinnedArtifact,
    },
    DirectApplication {
        application: HashPinnedArtifact,
        flash_offset: u32,
    },
}

pub struct Quotas {
    pub max_artifacts: u16,
    pub max_total_artifact_bytes: u64,
    pub max_runtime_bytes: u64,
    pub max_queued_input_events: u32,
    pub max_queued_input_bytes: u32,
    pub max_queued_output_events: u32,
    pub max_queued_output_bytes: u32,
    pub max_single_output_bytes: u32,
    pub max_frame_payload_bytes: u32,
    pub max_audio_payload_bytes: u32,
    pub max_ledger_entries_per_run: u32,
    pub max_inspect_bytes: u32,
    pub max_run_cycles: u64,
    pub max_run_instructions: u64,
    pub max_run_wall_millis: u32,
}

pub struct ArtifactManifest {
    pub artifacts: BoundedList<ArtifactDescriptor, MAX_ARTIFACTS>,
}

pub struct ArtifactDescriptor {
    pub id: ArtifactId,
    pub kind: ArtifactKind,
    pub sha256: [u8; 32],
    pub declared_size: u64,
}

pub enum ArtifactKind {
    MaskRomElf,
    FlashImage,
    Application,
    SymbolsElf,
    EfuseRegisterImage,
    RawResetRegisterCapture,
    ResetRegisterState,
    ResetRegisterFilterReceipt,
    InputTranscript,
    TimingManifest,
}

pub trait DeterministicServices {
    fn read_artifact_chunk(
        &mut self,
        id: &ArtifactId,
        offset: u64,
        destination: &mut [u8],
    ) -> Result<ChunkRead, ServiceError>;
}

pub struct ChunkRead {
    pub bytes_written: u32,
    pub end_of_artifact: bool,
}

pub enum ServiceErrorCode { UnknownArtifact, ReadFailed, ContractViolation }
pub struct ServiceError {
    pub code: ServiceErrorCode,
    pub message: DiagnosticString,
}

pub enum InspectionPolicy {
    Disabled,
    LocalTrusted { scope: DebugScope },
}

pub enum DebugScope {
    AnyMappedGuestMemory,
    Ranges(BoundedList<GuestRange, MAX_DEBUG_RANGES>),
}

// Fields and constructors are private to the factory crate. The value is not
// serializable or cloneable and is bound to exactly one backend instance.
pub struct DebugPermit { /* private instance and scope */ }

pub struct GuestRange {
    pub address: u32,
    pub length: u32,
}

pub struct OwnedGuestBytes {
    pub epoch: u64,
    pub cycle: VirtualCycle,
    pub range: GuestRange,
    pub bytes: BoundedBytes<MAX_INSPECT_BYTES>,
}

pub enum ResetKind { PowerOn, Software, Watchdog, External }

pub struct GuestStop {
    pub code: u32,
    pub reason: GuestStopReason,
}

pub enum GuestStopReason { Halted, RebootLoop, FirmwareExit, Fault }

pub struct LoadReceipt {
    pub artifact_hashes:
        BoundedList<(ArtifactId, [u8; 32]), MAX_ARTIFACTS>,
    pub artifact_set_hash: [u8; 32],
    pub identity_set_hash: [u8; 32],
    pub boot_set_hash: [u8; 32],
    pub active_boot: ActiveBootMode,
}

pub struct ResetReceipt {
    pub epoch: u64,
    pub cycle: VirtualCycle,
    pub kind: ResetKind,
    pub active_boot: ActiveBootMode,
    pub identity_set_hash: [u8; 32],
    pub boot_set_hash: [u8; 32],
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
    pub active_boot: ActiveBootMode,
    pub identity_set_hash: [u8; 32],
    pub boot_set_hash: [u8; 32],
    pub instructions: [u64; 2],
    pub pending_instruction: Option<PendingInstructionSummary>,
    pub stop: RunStop,
    pub ledger: LedgerDelta,
}

pub enum RunStop {
    DeadlineReached,
    BudgetExhausted { budget: BudgetKind },
    Idle { next_event: Option<VirtualCycle> },
    ResetRequested { kind: ResetKind },
    GuestStop(GuestStop),
    TimingBlocked(TimingBlock),
    QuotaExceeded { quota: QuotaKind },
    UnsupportedCapability { capability: CapabilityId },
}

pub enum BudgetKind {
    Cycles,
    Instructions,
    WallClock,
    OutputEvents,
    OutputBytes,
    LedgerEntries,
}

pub enum QuotaKind {
    ArtifactCount,
    ArtifactBytes,
    RuntimeBytes,
    InputEvents,
    InputBytes,
    OutputEvents,
    OutputBytes,
    SingleOutputBytes,
    FrameBytes,
    AudioBytes,
    LedgerEntries,
    InspectBytes,
}

pub enum CapabilityId {
    FastInterpreter,
    FastJit,
    MeasuredInterpreter,
    MeasuredSingleCore,
    MeasuredDualCore,
    NetworkTranscript,
    LiveNetworkEgress,
    Snapshot,
    PrivilegedInspection,
    RomFlashBoot,
    DirectApplicationBoot,
}

pub struct PendingInstructionSummary {
    pub core: u8,
    pub instruction_sequence: u64,
    pub pc: u32,
    pub start_cycle: VirtualCycle,
    pub completion_cycle: VirtualCycle,
}

pub struct TimingBlock {
    pub event_sequence: u64,
    pub instruction_sequence: Option<u64>,
    pub code: TimingBlockCode,
    pub tier_candidate: CostTierName,
    pub reason: DiagnosticString,
    pub source: ReceiptPath,
}

pub enum TimingBlockCode {
    UnknownCost,
    UnsupportedAccessShape,
    UnknownDeviceDeadline,
    InterveningAccessImpact,
    AffinePhaseUnresolved,
}

pub enum CostTierName { Exact, Affine, Interval, Distribution, Unexplained }

pub struct InputEvent {
    pub schema_version: u16,
    pub epoch: u64,
    pub cycle: VirtualCycle,
    pub sequence: u64,
    pub payload: InputPayload,
}

pub enum InputPayload {
    Gpio { pin: u16, level: GpioLevel },
    Touch { contact: u8, x: u16, y: u16, phase: TouchPhase },
    Serial {
        port: u8,
        bytes: BoundedBytes<MAX_SERIAL_PAYLOAD_BYTES>,
    },
    Reset { kind: ResetKind },
}

pub enum GpioLevel { Low, High }
pub enum TouchPhase { Down, Move, Up, Cancel }

pub struct BackendEvent {
    pub schema_version: u16,
    pub epoch: u64,
    pub cycle: VirtualCycle,
    pub sequence: u64,
    pub payload: EventPayload,
}

pub enum EventPayload {
    Serial {
        port: u8,
        bytes: BoundedBytes<MAX_SERIAL_PAYLOAD_BYTES>,
    },
    Framebuffer {
        surface: Identifier,
        width: u16,
        height: u16,
        row_start: u16,
        row_count: u16,
        stride_bytes: u32,
        format: PixelFormat,
        bytes: BoundedBytes<MAX_FRAME_PAYLOAD_BYTES>,
    },
    Audio {
        stream: Identifier,
        sample_rate_hz: u32,
        channels: u8,
        frames: u32,
        format: AudioFormat,
        bytes: BoundedBytes<MAX_AUDIO_PAYLOAD_BYTES>,
    },
    Gpio { pin: u16, level: GpioLevel },
    Diagnostic {
        code: DiagnosticCode,
        severity: Severity,
        message: DiagnosticString,
    },
    Reset { kind: ResetKind },
    GuestStop(GuestStop),
}

pub enum PixelFormat { Rgb565Le, Rgb888, Rgba8888 }
pub enum AudioFormat { Signed16Le }
pub enum Severity { Debug, Info, Warning, Error }
pub struct DiagnosticCode(pub Identifier);

pub struct DrainLimit {
    pub max_events: u32,
    pub max_bytes: u32,
}

pub struct EventBatch {
    pub events: BoundedList<BackendEvent, MAX_QUEUED_OUTPUT_EVENTS>,
    pub remaining_events: u32,
    pub remaining_bytes: u32,
}

pub struct Capabilities {
    pub adapter: ProtocolVersion,
    pub event_schema: u16,
    pub ledger_schema: u16,
    pub backend_name: Identifier,
    pub esp32sim_commit: CommitId,
    pub adapter_commit: CommitId,
    pub chip: Identifier,
    pub board: Identifier,
    pub active_boot: ActiveBootMode,
    pub engine: ActiveEngine,
    pub trust: TrustClass,
    pub interpreter: bool,
    pub native_jit: bool,
    pub wasm_jit: bool,
    pub network_none: bool,
    pub network_transcript: bool,
    pub live_network_egress: bool,
    pub snapshots: bool,
    pub privileged_inspection: bool,
    pub measured_single_core: bool,
    pub measured_dual_core: bool,
    pub rom_flash_boot: bool,
    pub direct_application_boot: bool,
    pub identity_set_sha256: Option<[u8; 32]>,
    pub boot_set_sha256: Option<[u8; 32]>,
    pub timing_manifest_sha256: Option<[u8; 32]>,
    pub receipt_set_sha256: Option<[u8; 32]>,
    pub observation_proof: ObservationProof,
    pub quotas: Quotas,
}

pub enum ActiveBootMode { RomFlash, DirectApplication }

pub enum ObservationProof {
    Absent,
    Proven { corpus_sha256: [u8; 32], result_sha256: [u8; 32] },
}
```

All `Quotas` fields must be nonzero and no greater than their same-named hard
maximum. `max_single_output_bytes` may not exceed
`max_queued_output_bytes`. Frame and audio payload limits may not exceed
`max_single_output_bytes`. A manifest may not contain duplicate IDs and may
contain each `ArtifactKind` at most once. The sum of declared artifact
sizes uses checked `u64` addition and may not exceed either the requested quota
or `MAX_TOTAL_ARTIFACT_BYTES`. The fixed per-kind limits are: Mask ROM ELF 64
MiB, flash image 32 MiB, application 32 MiB, symbols ELF 64 MiB, efuse register
image exactly 512 bytes, complete raw reset-register capture 16 MiB,
reset-register applied subset 64 KiB, reset-register filter receipt exactly 152
bytes, input transcript 16 MiB, and timing manifest 4 MiB.
`DebugScope::Ranges` contains 1 through `MAX_DEBUG_RANGES` nonempty valid
ranges. A `CommitId` contains lowercase hexadecimal and has exactly 40 or 64
bytes. `ChipConfig.core_count` is 1 or 2, CPU frequency is nonzero, and flash
and PSRAM sizes may not exceed `max_runtime_bytes` when combined with declared
internal RAM and adapter-owned runtime state. Flash size may not exceed 32 MiB.
Measured lane B additionally requires one active core.

A valid version-1 manifest always contains the exact `EfuseRegisterImage`,
`RawResetRegisterCapture`, `ResetRegisterState`, and
`ResetRegisterFilterReceipt` named and hash-pinned by `ChipIdentity`.
`TimingManifest` is required in measured mode and forbidden in fast mode.
`InputTranscript` is present if and only if
`NetworkPolicy::Transcript` names it. `SymbolsElf` is optional and never changes
guest state. Canonical manifest and artifact-set hashes sort descriptors by
`ArtifactKind` tag then artifact ID bytes and hash their version-1 canonical
encoding.

`BootMode::RomFlash` is the product boot. Its manifest contains the exact real
`MaskRomElf` and exact complete `FlashImage` named by the two hash-pinned
references and contains no `Application`. The flash artifact's declared and
actual size equals `ChipConfig.flash_bytes`; it includes bootloader, partition
table, application, and erased regions at their physical offsets and is written
at flash offset zero. The ROM ELF must parse successfully, map the reset vector
at `0x4000_0400`, and match its configured hash before any boot state changes.
Reset starts at the mask-ROM vector and must not call `Machine::boot_app` or
install second-stage-bootloader presets.

`BootMode::DirectApplication` is a separate test and diagnosis capability. Its
manifest contains the exact named `Application` and no `MaskRomElf` or
`FlashImage`. Version 1 requires `flash_offset == DIRECT_APP_FLASH_OFFSET`,
writes the application at that offset, and invokes the existing HLE
`Machine::boot_app` path including its synthetic second-stage state. A backend
advertises `direct_application_boot` explicitly or rejects the config. Direct
application output cannot satisfy a product-boot, real-ROM, or correlation
claim, and every run receipt and capability report names the active boot mode.
Checked addition must prove `DIRECT_APP_FLASH_OFFSET + application_size <=
ChipConfig.flash_bytes` before flash allocation or mutation.

The A-01/E identity handoff is not an efuse digest. It is the tuple of the
hash-pinned raw efuse image, exact strap word, complete hash-pinned raw OpenOCD
reset-register capture, separately hash-pinned canonical applied subset,
hash-pinned derivation and filter receipt, expected MAC, revision metadata, and
lane A/E adoption-receipt hash in `ChipIdentity`. Absence or mismatch of any
element is `IdentityMismatch` and blocks both boot modes.

`EfuseRegisterImage` version 1 is exactly 128 little-endian `u32` words. Word
index `i` replaces, rather than overlays, the efuse register at absolute address
`0x6000_7000 + 4*i`. With `w44 = words[0x44 / 4]` and
`w48 = words[0x48 / 4]`, the adapter decodes the base MAC as bytes
`[w48 >> 8, w48, w44 >> 24, w44 >> 16, w44 >> 8, w44]`, with each expression
truncated to `u8`, and requires it to equal `expected_base_mac`. Raw efuse words
are authoritative for guest-visible revision fields. `ChipIdentity.revision`
is provenance metadata and `adoption_receipt_sha256` pins the lane A/E
disposition that associates the efuse capture, strap, complete raw
reset-register capture, applied subset, filter receipt, board, and revision.

`RawResetRegisterCapture` is the complete byte-for-byte A-01 OpenOCD reset dump.
It is retained as provenance and is never passed to `Peripherals::init_regs`,
partially applied, or treated as the emulator reset state. Its bytes may contain
registers outside esp32sim's current modeled allowlist. The runtime verifies its
declared size and hash, then retains its artifact reference for receipts.

`ResetRegisterFilterReceipt` version 1 is a 152-byte canonical binary record:
`schema_version: u16 = 1`, `algorithm: u16 = 0`, raw-capture SHA-256,
applied-subset SHA-256, 40-byte lowercase hexadecimal pinned esp32sim commit,
parsed-record count `u32`, applied-record count `u32`, omitted-record count
`u32`, and lane A/E adoption-receipt SHA-256, in that order. Integers are
little-endian. Algorithm zero means the derivation tool parsed the complete
OpenOCD capture, retained only address and value pairs for which
`Peripherals::init_regs` at the pinned commit returned true, rejected duplicate
applied addresses, and emitted the sorted version-1 `ResetRegisterState` below.
The adapter requires both embedded hashes and the adoption hash to match
`ChipIdentity`, requires `parsed == applied + omitted` with checked addition,
and requires `applied` to equal the subset record count. Any mismatch is
`IdentityMismatch`. The receipt records the derivation; it does not make the raw
capture safe to apply.

`ResetRegisterState` version 1 is little-endian `schema_version: u16 = 1`,
reserved zero `u16`, record count `u32`, then strictly increasing unique
`{ address: u32, value: u32 }` records. Count is at most
`MAX_RESET_REGISTER_RECORDS`. Every address is 4-byte aligned and must be
in one of these inclusive version-1 ranges: `0x60008000..0x60008ffc`,
`0x600c0000..0x600c0ffc`, `0x600c4000..0x600c4ffc`,
`0x60009000..0x60009ffc`, `0x60002004..0x60002ffc`,
`0x60003004..0x60003ffc`, `0x6000e000..0x6000effc`,
`0x60026000..0x60026ffc`, or `0x600c1000..0x600c1ffc`. The excluded holes are
`0x600c0030..0x600c003c` and `0x60002058..0x60002094`. These are exactly the
pinned esp32sim `Peripherals::init_regs` allowlist. The loader rejects the whole
artifact if any record is outside it. No record is silently skipped. The raw
capture is never applied. This canonical artifact is the complete applied
subset and is applied before first reset, then
`strap_word` is assigned to `gpio.strap`. Power-on reset reapplies the adopted
applied subset, efuses, and strap. Software, watchdog, and external reset retain
or clear state according to the esp32sim reset implementation and do not
reapply either artifact.

`identity_set_hash` is SHA-256 over `b"esp32sim-identity-v1\0"`, revision major
and minor as two `u8` values, the six MAC bytes, little-endian strap `u32`, the
efuse artifact reference, complete raw reset-register capture reference,
applied-subset reference, filter-receipt reference, and adoption receipt hash in
that order. An artifact reference is canonical ID byte length as little-endian
`u32`, ID UTF-8 bytes, then its 32-byte hash.

`boot_set_hash` is SHA-256 over `b"esp32sim-boot-v1\0"`, boot tag as
little-endian `u16` (`0` ROM-flash, `1` direct application), then the active
artifact references in field order. Direct application appends its little-endian
offset `u32`; ROM-flash has no trailing field. Both hashes are exposed in load,
reset, run, and capability receipts.

For framebuffer output, bytes per pixel is fixed by `PixelFormat`. Checked
arithmetic must prove `row_start + row_count <= height`, `stride_bytes >= width
* bytes_per_pixel`, and `bytes.len() == stride_bytes * row_count`. For audio,
`sample_rate_hz` is 1 through 384,000, `channels` is 1 through 8, and checked
arithmetic must prove `bytes.len() == frames * channels * 2`. Queue accounting
includes the complete canonical serialization, not only the payload bytes.
Timing observations live in `LedgerDelta`, not diagnostics. The Rust adapter's
`ValidatedEventBuilder` is the primary validator and quota authority. A producer
passes scalar fields plus borrowed bytes or a bounded chunk reader. The builder
validates declared and computed lengths, guest ranges, per-event quota, and
remaining queue count and bytes before reserving or allocating owned payload
storage. It then copies at most the validated exact length, constructs
`BackendEvent`, and commits the queue reservation atomically. An output producer
may not construct an unbounded `Vec` as an intermediate. Puck TypeScript checks
the schema and bounds of the already-owned event again as thin-client defense,
but is not the primary safety seam.

The version-1 canonical encoding uses little-endian unsigned integers, `u16`
enum tags in declaration order starting at zero, `u32` byte-length prefixes for
every string or byte field, and no padding. The event envelope is schema `u16`,
epoch `u64`, cycle `u64`, sequence `u64`, payload tag `u16`, then payload fields
in declaration order. Queue byte accounting is exactly the canonical encoded
length, including envelope, tags, and length prefixes. Unknown tags, trailing
bytes, noncanonical UTF-8, and length mismatches are rejected.

`DeterministicServices::read_artifact_chunk` is the entire deterministic
service surface in version 1. `destination.len()` is at most
`MAX_ARTIFACT_CHUNK_BYTES`. Calls are sequential for each artifact and load
attempt, beginning at offset zero. A service must return the same bytes for a
given ID, offset, and manifest hash in every load attempt. `bytes_written` may
not exceed `destination.len()`, EOF before or after the declared size is
`InvalidArtifact`, and one one-byte probe at the declared size must return zero
bytes with EOF true. No wall time, filesystem path, environment,
socket, entropy, callback, or transcript stream is exposed. A transcript is an
immutable hashed artifact and is consumed internally.

The host wrapper implements wall cancellation outside `DeterministicServices`.
It sets a one-way cancellation flag after `max_wall_millis`; the backend polls
that flag only at scheduler checkpoints. Cancellation returns a deterministic
state prefix and does not fabricate virtual time or ledger entries. Re-running
without wall cancellation resumes from that exact prefix.

`create` rejects incompatible protocol majors or event schemas. The lane B
spike also rejects measured mode unless `MeasuredEngine::Interpreter`,
`NetworkPolicy::None`, and the policy ID `lane-b-single-core-v1` are requested.
`load` first validates the complete manifest, all declared sizes, aggregate
size, runtime-memory request, and all quotas before allocation. It then streams
each artifact through one fixed 64 KiB scratch region, computes SHA-256, and
stages backing storage. No guest-visible state changes until every actual size,
EOF, and hash matches and the staged set commits atomically.

The WebAssembly bridge does not marshal an `ArtifactSet { bytes }`. It exposes
`begin_load(manifest)`, sequential `load_chunk(session, artifact_index, offset,
ptr, len)`, `finish_artifact`, and `commit_load`. `begin_load` rejects a manifest
length over 64 KiB before reading its pointer. Checked `ptr + len` must fit
current Wasm memory before a borrowed manifest slice is constructed. A bounded
parser uses a fixed 16-descriptor array and validates every count, string length,
declared size, aggregate, and quota before artifact or guest-memory allocation.
`load_chunk` rejects `len > 64 KiB`, non-sequential offsets, bytes beyond the
declaration, and checked `ptr + len` outside current Wasm memory before
constructing a slice or copying a byte. The bridge uses one preallocated 64 KiB
staging region, verifies one explicit EOF marker and the hash, and rejects
commit unless actual size equals declared size. The exported general allocator
is not used for artifact bytes. Native
`DeterministicServices` and this Wasm session feed the same internal bounded
loader state machine.

The backend state machine is `Created`, `Loaded`, `Runnable`, `AwaitingReset`,
and `Closed`. `load` is valid only in `Created` and moves atomically to `Loaded`.
The first `reset` after `load` must be `PowerOn`; it installs the verified boot
artifacts and adopted identity before entering the selected boot path. Later
`reset` calls are valid in `Runnable` or `AwaitingReset`, increment the epoch,
zero virtual time and event sequences, and enter `Runnable`.
`run_until` and `inject` require `Runnable`. A due reset request discards any
pending instruction, consumes that request, enters `AwaitingReset`, and returns
`ResetRequested`; only explicit `reset` may resume. `drain_events` is valid in
every state except `Closed`. `capabilities` and repeatable `close` are valid in
all states.

`inject` validates schema, board pin or port existence, touch coordinates, and
canonical encoded length before reserving input queue count and bytes. It
rejects rather than dropping when either input quota is full. `DrainLimit`
fields may be zero and may not exceed configured output queue quotas. A drain
returns the longest queue prefix fitting both limits and never splits one
event. If the first event exceeds either requested limit, it returns an empty
batch with accurate remaining counts.

`GuestRange` is valid only when checked `u64` addition proves `address + length
<= 2^32`, `length > 0`, and `length <= min(max_bytes,
quotas.max_inspect_bytes)`. Inspection is all-or-error for one fully mapped
range and returns exactly `length` owned bytes. `DebugPermit` is issued only for
`TrustClass::LocalTrusted` plus `InspectionPolicy::LocalTrusted`. It is bound to
one backend instance and scope. The Wasm bridge stores it in a trusted host
handle table and never places its token in guest memory.

`close` is repeatable at the host-wrapper boundary. After its first successful
call, all methods except `capabilities` and `close` return `Closed`.

## Stable error contract

```rust
pub struct BackendError {
    pub code: ErrorCode,
    pub message: DiagnosticString,
}

pub enum ErrorCode {
    IncompatibleVersion,
    InvalidConfig,
    InvalidState,
    InvalidArtifact,
    InvalidBootSet,
    IdentityMismatch,
    ArtifactSizeMismatch,
    ArtifactHashMismatch,
    QuotaExceeded,
    UnsupportedCapability,
    EventInPast,
    EventSequence,
    PendingInstructionConflict,
    InspectionDenied,
    InvalidGuestRange,
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
- backend name, esp32sim commit, and fork adapter commit;
- chip and board identifiers;
- active boot mode, ROM-flash and direct-application boot capabilities, and
  verified boot-set and identity-set hashes;
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

```rust
pub struct DeviceId(pub Identifier);

pub enum DeviceDeadline {
    At(VirtualCycle),
    None,
    Unknown {
        device: DeviceId,
        reason: DeviceDeadlineUnknown,
    },
}

pub struct DeviceDeadlineUnknown {
    pub code: DeviceDeadlineUnknownCode,
    pub detail: DiagnosticString,
}

pub enum DeviceDeadlineUnknownCode {
    NoDeadlineModel,
    UnsupportedActivePath,
    UnresolvedExternalClock,
    UnresolvedDmaCompletion,
    UnresolvedSharedResource,
}
```

The measured scheduler owns one monotonic `u64` virtual-cycle counter per reset
epoch. `reset` increments the epoch and sets its virtual cycle to zero. CCOUNT
is a wrapping `u32` projection advanced by every virtual-cycle delta, including
a delta that ends with an instruction still pending.

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
reaches their cycle. If an instruction is pending, injection at or before its
completion is accepted only when the input-impact classifier proves the event
compatible with that instruction's access phase and dependency set. Unknown or
conflicting impact returns `PendingInstructionConflict` without queue mutation.

Each request validates every budget against `BackendConfig.quotas` and the hard
limits. `deadline` may not precede `start_cycle`. The cycle-budget endpoint is
checked `start_cycle + max_cycles`; overflow is `InvalidConfig`. The effective
cycle endpoint is the lesser of that endpoint and `deadline`. A zero cycle
budget provides a deterministic no-time-progress probe. A zero instruction
budget prevents a new instruction from starting and prevents a pending CPU
commit, but it may still advance an existing pending latency or idle devices
within a positive cycle budget.

Before a new instruction starts, the measured interpreter resolves its full
cost and every possible access shape without a guest-visible side effect. An
unresolved cost or access returns `TimingBlocked` before pending state is
created and changes no machine, device, CCOUNT, or virtual-time state. A
resolved instruction becomes a persistent `PendingInstruction` with its start,
completion, decoded operation, staged timing mutations, and receipt references.
Fetch is observed at its priced fetch phase.

Version 1 has one exact data-access phase:
`CompletionAfterSameCycleExternalEvents`. Effective addresses and non-memory
operands are captured from CPU state at instruction start. No load value, fault,
MMIO return, atomic result, store effect, or MMIO side effect is previewed or
cached. At the completion cycle, reset, device and DMA completion, and injected
input run in event-order positions 1 through 3. The interpreter then performs
the data access exactly once against that resulting state at position 4 and
commits its architectural effects. A RAM load therefore observes a DMA write
completed earlier in the interval or at the same cycle; a store becomes visible
after those external transitions. Atomic read and write are one indivisible
position-4 access.

The pending plan carries a dependency set covering accessed guest ranges, MMU
and cache versions, bus routing, target device state, possible fault, receipt
match fields, returned value, and side effects. Every device deadline, DMA
completion, and input classifier declares its possible impacts in the same
vocabulary. Before pending state is created, any intervening event that can
change access timing or a receipt match blocks unless a committed receipt proves
the duration invariant and completion-phase execution remains exact. An
implementation that cannot defer the actual value, fault, match-key evaluation,
and side effect to the completion phase must block whenever an intervening
impact can change any of them. It may not reuse a preflight value or validate
only the cost key.

If an exact device transition generates a previously unknowable impact before
completion, the scheduler commits only the complete prefix through that device
event, leaves the instruction pending, and returns `TimingBlocked` with reason
`InterveningAccessImpact`. No later run may complete that instruction; reset or
a backend recreated with a stronger reviewed model is required. This terminal
fail-closed path and all impact declarations are included in contract tests.

The scheduler may stop between the pending instruction's start and completion.
It advances virtual time, CCOUNT, devices, CCOMPARE assertions, and injected
events through the effective endpoint, but it does not commit the pending CPU
effects. The pending instruction is returned in `RunSlice` and resumes without
replanning on the next call. At its completion cycle, same-cycle reset, device,
and input events run first. Reset discards the pending instruction. Otherwise
the CPU effects commit at event-order position 4 and the completed-instruction
count increments once.

`max_cycles` limits virtual cycles advanced in this call. Crossing it preserves
the pending instruction and stops exactly at the cycle-budget endpoint. The
completion boundary is included, so an instruction completing exactly at that
endpoint commits when the instruction budget permits. `max_instructions`
counts only CPU commits in this call. When the count is exhausted at a pending
completion cycle, higher-priority same-cycle events are applied, CPU commit is
withheld, and the pending instruction remains ready at that cycle. A later call
with instruction budget commits it without advancing time. No budget ever
causes a partial architectural commit.

The stable stop precedence is semantic stop (`ResetRequested`, `GuestStop`,
`TimingBlocked`, `QuotaExceeded`, or `UnsupportedCapability`), requested
deadline, cycle budget, instruction budget, wall cancellation, output-event
budget, output-byte budget, ledger budget, then idle. Thus equality of the run
deadline and cycle-budget endpoint returns `DeadlineReached`. A budget stop
reports the first budget in that order which prevents the next transition.

If all CPUs are idle, time advances to the minimum of the effective endpoint,
next injection, and next known device deadline. Wall cancellation is checked
between scheduler transitions. It returns the last complete deterministic
prefix; it does not undo committed state or assign a cycle delta.

Every active time-aware device returns `At(cycle)`, `None`, or the typed bounded
`Unknown`. `None` proves there is no autonomous change. `Unknown` stops at the
last known complete prefix and yields `TimingBlocked`. `At` may not precede the
current cycle. Delivery at the current cycle must change device state so the
next result advances, becomes `None`, or becomes `Unknown`; otherwise it is a
`BackendFault`. Device time is delivered through the current virtual cycle
before MMIO. Batches split at every declared device deadline.

CCOMPARE matching is computed against every CCOUNT delta, including a delta
inside a pending priced instruction and a wrapping interval. Each matching
timer interrupt asserts at its exact virtual cycle in event-order position 2.
The current instruction cannot accept it. Interrupt acceptance is considered
only after that instruction commits and before the next instruction starts.
Device deadlines inside the same priced interval are delivered at their exact
cycles and may schedule further deadlines before instruction completion.

Pending state, all same-cycle ordering cursors, CCOUNT, device clocks, input
cursors, and ledger sequence are persistent. Therefore one call ending at cycle
1000 and calls ending at 100, 200, through 1000 produce identical state, event
bytes, ordered canonical ledger entries, and cumulative `chain_after` at every
shared entry sequence, including when CCOMPARE or a device deadline lies inside
an instruction. Per-call delta hashes may differ.

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
    pub first_sequence: u64,
    pub entries: BoundedList<LedgerEntry, MAX_LEDGER_ENTRIES_PER_RUN>,
    pub status: LedgerStatus,
    pub delta_entries_hash: [u8; 32],
    pub chain_before: [u8; 32],
    pub chain_after: [u8; 32],
    pub cumulative_entry_count: u64,
    pub cumulative_resolved_cycles: Option<u64>,
    pub ledger_state_hash: [u8; 32],
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
    Open,
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
        measured_counts: BoundedList<u64, MAX_AFFINE_COUNTS>,
        cohort: CohortKey,
    },
    Interval { min: u64, max: u64, cause: DiagnosticString },
    Distribution { quantiles: Quantiles, cause: DiagnosticString },
    Unexplained { reason: DiagnosticString },
}

pub struct Quantiles {
    pub sample_count: u64,
    pub minimum: u64,
    pub p50: u64,
    pub p90: u64,
    pub p99: u64,
    pub maximum: u64,
}

pub struct ReceiptRef {
    pub repository: Identifier,
    pub commit: CommitId,
    pub path: ReceiptPath,
    pub sha256: [u8; 32],
    pub firmware_sha256: [u8; 32],
    pub sdkconfig_sha256: [u8; 32],
    pub toolchain: ToolchainIdentity,
    pub board_revision: Identifier,
    pub adoption: AdoptionStatus,
}

pub struct ToolchainIdentity {
    pub esp_idf_version: Identifier,
    pub compiler: Identifier,
    pub compiler_version: Identifier,
    pub target: Identifier,
}

pub enum AdoptionStatus { Accepted }

pub enum TimingEventKind {
    InstructionBase,
    InstructionFetch,
    LiteralLoad,
    DataLoad,
    DataStore,
    AtomicAccess,
    MmioRead,
    MmioWrite,
    CacheHit,
    CacheLineFill,
    CacheWriteback,
    BranchRoute,
    LoadUseDependency,
    WindowException,
    LoopAlignment,
    OrdinaryException,
    InterruptAssertion,
    InterruptAcceptance,
    CodeInvalidation,
    IdleAdvance,
    DeviceDeadlineDelivery,
}
```

`BoundedList::new` checks count before taking ownership of its backing vector.
The artifact, output-event, and ledger loaders use fixed-capacity builders so an
untrusted count is rejected before the vector allocation. `Quantiles` requires
`sample_count > 0` and nondecreasing minimum, p50, p90, p99, and maximum. Every
affine count is positive, the count list is nonempty, and checked signed
arithmetic must resolve a nonnegative `u64` for an executable occurrence.

`resolved_cycles` is present only when a deterministic event-scoped resolver
maps the claim to this occurrence. An interval or distribution claim therefore
does not imply sampling. Its accepted deterministic phase model, if any, is
part of the hashed timing manifest. `Unexplained` never resolves.

`LedgerDelta.entries` contains only the contiguous entries emitted by this
`run_until` call. `first_sequence` equals the first entry sequence, or the next
global sequence for an empty delta. `start_cycle`, `end_cycle`, delta grouping,
and `delta_entries_hash` are transport metadata and are not inputs to the
cumulative chain. `delta_entries_hash` is SHA-256 over the domain string
`esp32sim-ledger-delta-v1\0`, first sequence and entry count as little-endian
`u64`, and each canonical entry prefixed by its little-endian `u64` byte length.
It may differ when the caller partitions a run differently.

The cumulative chain is independent of run slicing. At power-on reset:

```text
H0 = SHA256(
  "esp32sim-ledger-chain-v1\0" ||
  ledger_schema_le || epoch_le || artifact_set_hash || identity_set_hash ||
  boot_set_hash || timing_manifest_hash || receipt_set_hash
)
H(n+1) = SHA256(
  "esp32sim-ledger-entry-v1\0" || Hn || entry_length_le ||
  canonical_ledger_entry_bytes
)
```

Canonical ledger entry bytes use the version-1 little-endian encoding, fixed
enum tags, bounded UTF-8 bytes, and the complete `ReceiptRef`; they never contain
a run-slice identifier. `chain_before` is the chain at `first_sequence`,
`chain_after` is the result after exactly this delta's entries, and
`cumulative_entry_count` is the number of entries incorporated in
`chain_after`. An empty delta has equal before and after hashes. Software,
watchdog, and external resets close the current epoch and seed the next epoch
with its incremented epoch and unchanged provenance hashes.

`timing_manifest_hash` and `receipt_set_hash` in the seed use one tag byte:
zero for absent, or one followed by the 32 hash bytes for present. Measured mode
requires both present. All other seed hashes are exactly 32 bytes.

For `LedgerStatus::Open`, `ledger_state_hash == chain_after`. For
`LedgerStatus::Blocked(block)`, `ledger_state_hash` is SHA-256 over
`"esp32sim-ledger-blocked-v1\0"`, `chain_after`, and the canonical bounded
`TimingBlock`. The blocked hash identifies the same known prefix and refusal but
does not make it a complete timing total. `cumulative_resolved_cycles` is
present only while every chained claim through `chain_after` has an event-scoped
deterministic resolution; it is absent after a block.

Affine claims retain slope, intercept, measured cell sizes, and cohort scope.
The committed same-value MMIO evidence is `3n - 8`. The current schema-1
profile's scalar `3` is not an executable replacement for that claim. The
initial importer rejects it for measured totals, and the online scheduler
blocks until an event-scoped resolver is reviewed. It does not distribute or
discard the negative intercept.

Unknown cost blocks the ledger total. No cumulative chain or state hash is
described as a complete total when `LedgerStatus::Blocked` is present. Same
normalized trace plus the same provenance hashes must produce the same ordered
canonical entries and the same `chain_after` at every global entry sequence.
Different run partitions may return different delta groupings and
`delta_entries_hash` values without changing that cumulative chain.

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

Lane 0's current rebaseline reports a toolchain-sensitive pooled single-core
first-line diagnostic: instruction-cache flash changed from 204 to 203 cycles,
data-cache flash from 115 to 114, and data-cache PSRAM from 82 to 81. The
per-boot ladders are internally identical, so neither the minus-one values nor
the superseded first-line values are executable costs until lane 0 diagnoses
the pooling probe and commits an adoption disposition. The importer blocks the
whole first-line class. Subsequent-line observations remain 266, 473, and 170
cycles, and MMIO observations remain 8 cycles per read and 12,280 cycles for
4,096 writes. Those unchanged observations are usable only through committed,
accepted, exact-toolchain receipts under decision 0008.

## Proposed source-document amendments, unaccepted

These replacements record the maintainer's product direction precisely enough
for review. They do not modify or supersede the accepted decisions or roadmap
until the maintainer accepts their exact text.

Replace decision 0011's `The role of puck` section with:

> The product is a browser-hosted cycle-accurate ESP32-S3 emulator built from
> our esp32sim fork. The fork owns the Rust machine, measured mode, browser
> interface, Wasm JIT, device models, safety seams, and web UI shell. Puck is the
> donor, evidence, differential, and decision repository. Its UI, recorder and
> replay, differential harness, timing model, receipts, and browser pieces may
> be ported selectively with provenance. Cross-lane architectural decisions are
> recorded in Puck's `docs/decisions`. Puck is not the cycle-accurate product
> and does not carry a separate execution engine or deep backend adapter.

Replace decision 0012's first backend-adapter paragraph with:

> All browser product entry points reach the machine through one versioned Rust
> adapter owned by our esp32sim fork. Nothing outside that adapter imports
> machine, bus, peripheral, JIT, or internal WebAssembly types. The adapter
> creates from validated deterministic configuration, loads hash-pinned
> immutable artifacts through a bounded pre-allocation loader, applies the
> hash-pinned raw efuse, strap, and canonical reset-register subset, preserves
> the separate complete raw OpenOCD capture and its filter receipt as identity
> provenance, selects an explicit
> real-ROM flash or capability-gated direct-app boot mode, resets with an
> explicit cause, runs to a virtual deadline under explicit budgets, injects
> timestamped events, drains typed bounded events, gates memory inspection,
> reports capabilities, versions its schemas, and shuts down deterministically.
> It owns virtual time, event and ledger schemas, quota enforcement, stable
> errors, cumulative slice-independent ledger chaining, primary guest-output
> validation before `BackendEvent` construction, and browser marshalling. The
> fork-owned web UI TypeScript is a thin UI and transport client which
> schema-checks already-owned bounded events as defense in depth.

Replace decision 0012's lane B consequence with:

> Lane B builds the Rust adapter, measured scheduler, timing importer and
> ledger, quota module, primary guest-output validator, and CPU observation
> contract in our esp32sim fork before peripheral grafting. The boundary and CI
> lanes test the versioned Rust interface. Puck supplies donor code and evidence,
> not a second adapter implementation.

Publish roadmap revision 4 with this product identity and scope paragraph:

> The final product is a browser-hosted cycle-accurate ESP32-S3 emulator built
> from our esp32sim fork. Its scope is the complete ESP32-S3 SoC plus the exact
> Waveshare TinyDraw V2 board: SRAM, flash, octal PSRAM, caches and MMU, DMA and
> peripherals, and eventually the CO5300 panel with GRAM, TE and scan-out,
> CST820 touch, QMI8658, PCF85063A, and TCA9554. Radio and SoC blocks omitted
> from the first TinyDraw V2 milestone are deferred, not permanently out of
> scope. The first useful milestone remains real TinyDraw V2 firmware boot,
> draw, and touch in the browser.

Replace roadmap lane B with:

> Measured mode and the fork-owned Rust browser adapter: versioned adapter and
> event schemas, interpreter-first timing-driven execution, pending instruction
> scheduling, exact device deadlines, block-batched CCOUNT, CPU-backend
> observation, receipt-pinned timing importer and ledger, pre-allocation quotas,
> raw adopted chip identity, real-ROM flash and direct-app boot capabilities,
> slice-independent cumulative ledger chaining, primary guest-output validation,
> and shared Rust contract tests. Puck timing traces and receipts are donor
> evidence. No TypeScript execution engine is built.

Replace roadmap lane F's `puck UX wrap (or successor UI)` phrase with
`fork-owned thin web UI shell over the versioned Wasm interface, using selected
Puck UI and browser pieces with provenance`. Replace the standing dependency rule
with `No browser TypeScript or other product caller imports esp32sim internals;
all machine access crosses the fork-owned Rust adapter, and dependency lint
enforces it`.

## Consequences if accepted

- Fast mode keeps its current run loop, block layouts, CCOUNT behavior, and JIT
  inner path with no measured bookkeeping.
- Measured mode starts as interpreter-only, single-core, networking-off, and
  fail-closed on unknown device deadlines or costs.
- The adapter interface, event and ledger schemas, scheduler, timing importer,
  quotas, and primary validator live in the esp32sim fork and have shared Rust
  fake-backend contract tests.
- Product boot requires the exact hash-pinned real mask ROM, complete flash
  image, raw efuse image, strap, complete raw reset-register capture, canonical
  applied subset, and filter receipt. Only the applied subset reaches
  `Peripherals::init_regs`. The HLE direct-app path is a separately advertised
  non-product capability.
- Ledger deltas remain per call, while the canonical cumulative chain is
  independent of `run_until` slicing.
- Lane C must propose the measured dual-core policy before enabling that
  capability.
- Lane H reviews the Rust validated-output seam used before `BackendEvent`
  construction. Puck performs only thin-client schema checks after ownership.
- Snapshot capability stays absent until a complete versioned state design is
  approved.
- Schema-1 `timing.json` cannot support a measured total because it lacks
  decision-0008 tier structure and loses the affine MMIO intercept.

## Approval checklist

Approval must accept or amend the exact scheduler ordering, single-core
capability boundary, fork crate and module homes, and the proposed amendments
to decision 0011, decision 0012, and roadmap revision 4. It must assign the
numbered decision record to the recommended Puck `docs/decisions` home or record
a different maintainer disposition, and record the owner of timing-profile
schema version 2 and the lane H validator review.

Until that happens, this document remains a design-spike artifact only.
