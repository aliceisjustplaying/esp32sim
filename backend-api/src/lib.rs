//! Versioned, deterministic backend boundary for the ESP32-S3 product.
//!
//! The API owns virtual time and bounded inputs and outputs. Implementations
//! must fail closed when a measured cost or autonomous device deadline is
//! unknown.

pub mod contract_suite;
mod fake;
mod timing_profile;

pub use fake::{test_claim, FakeBackend, FakeInstruction};
use sha2::{Digest, Sha256};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
pub use timing_profile::{
    import_timing_profile_v2, CostBinding, ImportedTimingProfile, ProfileError, ReceiptManifest,
};

pub const ADAPTER_VERSION: ProtocolVersion = ProtocolVersion::new(1, 0, 0);
pub const EVENT_SCHEMA_VERSION: u16 = 1;
pub const LEDGER_SCHEMA_VERSION: u16 = 1;
pub const MAX_ARTIFACTS: usize = 16;
pub const MAX_TOTAL_ARTIFACT_BYTES: u64 = 128 * 1024 * 1024;
pub const MAX_QUEUED_INPUT_EVENTS: usize = 4096;
pub const MAX_QUEUED_OUTPUT_EVENTS: usize = 4096;
pub const MAX_INSPECT_BYTES: usize = 1024 * 1024;
pub const MAX_RUN_CYCLES: u64 = 1 << 32;
pub const MAX_RUN_INSTRUCTIONS: u64 = 10_000_000;
pub const MAX_LEDGER_ENTRIES_PER_RUN: usize = 65_536;

pub type VirtualCycle = u64;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ProtocolVersion {
    pub major: u16,
    pub minor: u16,
    pub patch: u16,
}

impl ProtocolVersion {
    pub const fn new(major: u16, minor: u16, patch: u16) -> Self {
        Self {
            major,
            minor,
            patch,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CostTier {
    Exact {
        cycles: u64,
    },
    Affine {
        slope: i64,
        intercept: i64,
        minimum_count: u64,
        maximum_count: u64,
    },
    Interval {
        minimum: u64,
        maximum: u64,
        cause: String,
    },
    Distribution {
        minimum: u64,
        median: u64,
        maximum: u64,
        samples: u64,
        boots: u32,
        cause: String,
    },
    Unexplained {
        reason: String,
    },
}

impl CostTier {
    pub fn candidate_name(&self) -> &'static str {
        match self {
            Self::Exact { .. } => "exact",
            Self::Affine { .. } => "affine",
            Self::Interval { .. } => "interval",
            Self::Distribution { .. } => "distribution",
            Self::Unexplained { .. } => "unexplained",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReceiptRef {
    pub repository: String,
    pub commit: String,
    pub path: String,
    pub sha256: [u8; 32],
    pub firmware: String,
    pub sdkconfig_sha256: [u8; 32],
    pub toolchain: String,
    pub board_revision: String,
    pub adoption_status: AdoptionStatus,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AdoptionStatus {
    Accepted,
    Candidate,
    Rejected,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CostClaim {
    pub id: String,
    pub tier: CostTier,
    pub receipts: Vec<ReceiptRef>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TimingBlock {
    pub claim_id: String,
    pub tier_candidate: String,
    pub reason: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResetKind {
    PowerOn,
    Software,
    Watchdog,
    External,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BootMode {
    RomFlash,
    DirectApplication,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ArtifactKind {
    MaskRom,
    FlashImage,
    Application,
    TimingProfile,
    OpaqueIdentity,
    InputTranscript,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Artifact {
    pub id: String,
    pub kind: ArtifactKind,
    pub sha256: [u8; 32],
    pub bytes: Vec<u8>,
}

impl Artifact {
    pub fn new(id: impl Into<String>, kind: ArtifactKind, bytes: Vec<u8>) -> Self {
        let sha256 = Sha256::digest(&bytes).into();
        Self {
            id: id.into(),
            kind,
            sha256,
            bytes,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BackendConfig {
    pub requested_adapter: ProtocolVersion,
    pub core_count: u8,
    pub measured: bool,
    pub networking: bool,
    pub boot: BootMode,
    pub inspection: bool,
}

impl Default for BackendConfig {
    fn default() -> Self {
        Self {
            requested_adapter: ADAPTER_VERSION,
            core_count: 1,
            measured: true,
            networking: false,
            boot: BootMode::DirectApplication,
            inspection: false,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LoadReceipt {
    pub artifact_set_sha256: [u8; 32],
    pub artifact_count: usize,
    pub total_bytes: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResetReceipt {
    pub epoch: u64,
    pub cycle: VirtualCycle,
    pub kind: ResetKind,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InputPayload {
    Bytes(Vec<u8>),
    Reset(ResetKind),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InputEvent {
    pub epoch: u64,
    pub cycle: VirtualCycle,
    pub caller_sequence: u64,
    pub payload: InputPayload,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EventPayload {
    Bytes(Vec<u8>),
    Reset(ResetKind),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BackendEvent {
    pub schema_version: u16,
    pub epoch: u64,
    pub cycle: VirtualCycle,
    pub sequence: u64,
    pub payload: EventPayload,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LedgerKind {
    InstructionStart { pc: u32, completion: VirtualCycle },
    InstructionCommit { pc: u32 },
    InputApplied { caller_sequence: u64 },
    DeviceDeadline { device: String },
    CcompareAssert { comparator: u8 },
    IdleAdvance,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LedgerEntry {
    pub epoch: u64,
    pub cycle: VirtualCycle,
    pub sequence: u64,
    pub kind: LedgerKind,
    pub costs: Vec<CostClaim>,
}

impl LedgerEntry {
    pub fn canonical_bytes(&self) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(&self.epoch.to_le_bytes());
        out.extend_from_slice(&self.cycle.to_le_bytes());
        out.extend_from_slice(&self.sequence.to_le_bytes());
        match &self.kind {
            LedgerKind::InstructionStart { pc, completion } => {
                out.extend_from_slice(&0u16.to_le_bytes());
                out.extend_from_slice(&pc.to_le_bytes());
                out.extend_from_slice(&completion.to_le_bytes());
            }
            LedgerKind::InstructionCommit { pc } => {
                out.extend_from_slice(&1u16.to_le_bytes());
                out.extend_from_slice(&pc.to_le_bytes());
            }
            LedgerKind::InputApplied { caller_sequence } => {
                out.extend_from_slice(&2u16.to_le_bytes());
                out.extend_from_slice(&caller_sequence.to_le_bytes());
            }
            LedgerKind::DeviceDeadline { device } => {
                out.extend_from_slice(&3u16.to_le_bytes());
                encode_bytes(&mut out, device.as_bytes());
            }
            LedgerKind::CcompareAssert { comparator } => {
                out.extend_from_slice(&4u16.to_le_bytes());
                out.push(*comparator);
            }
            LedgerKind::IdleAdvance => out.extend_from_slice(&5u16.to_le_bytes()),
        }
        let mut costs: Vec<&CostClaim> = self.costs.iter().collect();
        costs.sort_by(|left, right| left.id.cmp(&right.id));
        out.extend_from_slice(&(costs.len() as u32).to_le_bytes());
        for cost in costs {
            encode_bytes(&mut out, cost.id.as_bytes());
            encode_tier(&mut out, &cost.tier);
            let mut receipts: Vec<&ReceiptRef> = cost.receipts.iter().collect();
            receipts.sort_by(|left, right| {
                (&left.repository, &left.commit, &left.path).cmp(&(
                    &right.repository,
                    &right.commit,
                    &right.path,
                ))
            });
            out.extend_from_slice(&(receipts.len() as u32).to_le_bytes());
            for receipt in receipts {
                encode_receipt(&mut out, receipt);
            }
        }
        out
    }
}

fn encode_tier(out: &mut Vec<u8>, tier: &CostTier) {
    match tier {
        CostTier::Exact { cycles } => {
            out.extend_from_slice(&0u16.to_le_bytes());
            out.extend_from_slice(&cycles.to_le_bytes());
        }
        CostTier::Affine {
            slope,
            intercept,
            minimum_count,
            maximum_count,
        } => {
            out.extend_from_slice(&1u16.to_le_bytes());
            out.extend_from_slice(&slope.to_le_bytes());
            out.extend_from_slice(&intercept.to_le_bytes());
            out.extend_from_slice(&minimum_count.to_le_bytes());
            out.extend_from_slice(&maximum_count.to_le_bytes());
        }
        CostTier::Interval {
            minimum,
            maximum,
            cause,
        } => {
            out.extend_from_slice(&2u16.to_le_bytes());
            out.extend_from_slice(&minimum.to_le_bytes());
            out.extend_from_slice(&maximum.to_le_bytes());
            encode_bytes(out, cause.as_bytes());
        }
        CostTier::Distribution {
            minimum,
            median,
            maximum,
            samples,
            boots,
            cause,
        } => {
            out.extend_from_slice(&3u16.to_le_bytes());
            out.extend_from_slice(&minimum.to_le_bytes());
            out.extend_from_slice(&median.to_le_bytes());
            out.extend_from_slice(&maximum.to_le_bytes());
            out.extend_from_slice(&samples.to_le_bytes());
            out.extend_from_slice(&boots.to_le_bytes());
            encode_bytes(out, cause.as_bytes());
        }
        CostTier::Unexplained { reason } => {
            out.extend_from_slice(&4u16.to_le_bytes());
            encode_bytes(out, reason.as_bytes());
        }
    }
}

fn encode_receipt(out: &mut Vec<u8>, receipt: &ReceiptRef) {
    encode_bytes(out, receipt.repository.as_bytes());
    encode_bytes(out, receipt.commit.as_bytes());
    encode_bytes(out, receipt.path.as_bytes());
    out.extend_from_slice(&receipt.sha256);
    encode_bytes(out, receipt.firmware.as_bytes());
    out.extend_from_slice(&receipt.sdkconfig_sha256);
    encode_bytes(out, receipt.toolchain.as_bytes());
    encode_bytes(out, receipt.board_revision.as_bytes());
    out.extend_from_slice(
        &(match receipt.adoption_status {
            AdoptionStatus::Accepted => 0u16,
            AdoptionStatus::Candidate => 1u16,
            AdoptionStatus::Rejected => 2u16,
        })
        .to_le_bytes(),
    );
}

fn encode_bytes(out: &mut Vec<u8>, bytes: &[u8]) {
    out.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
    out.extend_from_slice(bytes);
}

pub fn canonical_ledger_bytes(entries: &[LedgerEntry]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&LEDGER_SCHEMA_VERSION.to_le_bytes());
    out.extend_from_slice(&(entries.len() as u64).to_le_bytes());
    for entry in entries {
        let encoded = entry.canonical_bytes();
        encode_bytes(&mut out, &encoded);
    }
    out
}

pub fn ledger_sha256(entries: &[LedgerEntry]) -> [u8; 32] {
    Sha256::digest(canonical_ledger_bytes(entries)).into()
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LedgerDelta {
    pub entries: Vec<LedgerEntry>,
    pub canonical_sha256: [u8; 32],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RunBudget {
    pub max_cycles: u64,
    pub max_instructions: u64,
    pub max_output_events: u32,
    pub max_ledger_entries: u32,
}

impl Default for RunBudget {
    fn default() -> Self {
        Self {
            max_cycles: MAX_RUN_CYCLES,
            max_instructions: MAX_RUN_INSTRUCTIONS,
            max_output_events: MAX_QUEUED_OUTPUT_EVENTS as u32,
            max_ledger_entries: MAX_LEDGER_ENTRIES_PER_RUN as u32,
        }
    }
}

#[derive(Clone)]
pub struct CancellationFlag(Arc<AtomicBool>);

impl CancellationFlag {
    pub fn new() -> Self {
        Self(Arc::new(AtomicBool::new(false)))
    }

    pub fn cancel(&self) {
        self.0.store(true, Ordering::Release);
    }

    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Acquire)
    }
}

impl Default for CancellationFlag {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone)]
pub struct RunRequest {
    pub deadline: VirtualCycle,
    pub budget: RunBudget,
    pub cancellation: CancellationFlag,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BudgetKind {
    Cycles,
    Instructions,
    WallCancellation,
    OutputEvents,
    LedgerEntries,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RunStop {
    DeadlineReached,
    BudgetExhausted(BudgetKind),
    Idle { next_event: Option<VirtualCycle> },
    ResetRequested(ResetKind),
    TimingBlocked(TimingBlock),
    UnsupportedCapability(String),
    BackendFault(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PendingInstructionSummary {
    pub pc: u32,
    pub start: VirtualCycle,
    pub completion: VirtualCycle,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RunSlice {
    pub epoch: u64,
    pub start_cycle: VirtualCycle,
    pub end_cycle: VirtualCycle,
    pub completed_instructions: u64,
    pub pending_instruction: Option<PendingInstructionSummary>,
    pub stop: RunStop,
    pub ledger: LedgerDelta,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EventBatch {
    pub events: Vec<BackendEvent>,
    pub remaining: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Inspection {
    pub epoch: u64,
    pub cycle: VirtualCycle,
    pub address: u32,
    pub bytes: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Capabilities {
    pub adapter: ProtocolVersion,
    pub event_schema: u16,
    pub ledger_schema: u16,
    pub backend_name: String,
    pub measured_interpreter: bool,
    pub measured_single_core: bool,
    pub measured_dual_core: bool,
    pub networking: bool,
    pub native_jit_observation_proven: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BackendError {
    InvalidConfig(String),
    InvalidArtifact(String),
    InvalidRequest(String),
    InvalidInput(String),
    Closed,
    NotLoaded,
    NotReset,
    UnsupportedCapability(String),
    InspectionDenied,
    BackendFault(String),
}

pub trait Backend: Send {
    fn load(&mut self, artifacts: Vec<Artifact>) -> Result<LoadReceipt, BackendError>;
    fn reset(&mut self, kind: ResetKind) -> Result<ResetReceipt, BackendError>;
    fn run_until(&mut self, request: RunRequest) -> Result<RunSlice, BackendError>;
    fn inject(&mut self, event: InputEvent) -> Result<(), BackendError>;
    fn drain_events(&mut self, limit: usize) -> Result<EventBatch, BackendError>;
    fn inspect(&self, address: u32, max_bytes: usize) -> Result<Inspection, BackendError>;
    fn capabilities(&self) -> Capabilities;
    fn close(&mut self) -> Result<(), BackendError>;
}

pub fn validate_config(config: &BackendConfig) -> Result<(), BackendError> {
    if config.requested_adapter.major != ADAPTER_VERSION.major {
        return Err(BackendError::InvalidConfig(format!(
            "adapter major {} is unsupported",
            config.requested_adapter.major
        )));
    }
    if config.core_count != 1 {
        return Err(BackendError::UnsupportedCapability(
            "measured-dual-core".into(),
        ));
    }
    if !config.measured {
        return Err(BackendError::UnsupportedCapability(
            "fast-engine-through-versioned-adapter".into(),
        ));
    }
    if config.networking {
        return Err(BackendError::UnsupportedCapability("networking".into()));
    }
    Ok(())
}

pub fn validate_artifacts(artifacts: &[Artifact]) -> Result<LoadReceipt, BackendError> {
    if artifacts.len() > MAX_ARTIFACTS {
        return Err(BackendError::InvalidArtifact(
            "artifact count exceeds limit".into(),
        ));
    }
    let mut ids = std::collections::BTreeSet::new();
    let mut total = 0u64;
    let mut descriptors = Vec::new();
    for artifact in artifacts {
        if artifact.id.is_empty() || artifact.id.as_bytes().contains(&0) {
            return Err(BackendError::InvalidArtifact("invalid artifact id".into()));
        }
        if !ids.insert(artifact.id.clone()) {
            return Err(BackendError::InvalidArtifact(
                "duplicate artifact id".into(),
            ));
        }
        total = total
            .checked_add(artifact.bytes.len() as u64)
            .ok_or_else(|| BackendError::InvalidArtifact("artifact size overflow".into()))?;
        if total > MAX_TOTAL_ARTIFACT_BYTES {
            return Err(BackendError::InvalidArtifact(
                "artifact bytes exceed limit".into(),
            ));
        }
        let actual: [u8; 32] = Sha256::digest(&artifact.bytes).into();
        if actual != artifact.sha256 {
            return Err(BackendError::InvalidArtifact(format!(
                "artifact {} hash mismatch",
                artifact.id
            )));
        }
        descriptors.push((
            artifact.kind as u16,
            artifact.id.as_bytes(),
            artifact.sha256,
        ));
    }
    descriptors.sort_by(|left, right| (left.0, left.1).cmp(&(right.0, right.1)));
    let mut canonical = Vec::new();
    for (kind, id, hash) in descriptors {
        canonical.extend_from_slice(&kind.to_le_bytes());
        encode_bytes(&mut canonical, id);
        canonical.extend_from_slice(&hash);
    }
    Ok(LoadReceipt {
        artifact_set_sha256: Sha256::digest(&canonical).into(),
        artifact_count: artifacts.len(),
        total_bytes: total,
    })
}
