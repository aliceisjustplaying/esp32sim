//! Typed contract shared by measured ESP32-S3 execution backends.
//!
//! Timing is planned as an immutable transaction, then committed only after
//! the architectural operation succeeds. Autonomous models expose future
//! deadlines so the scheduler never reconstructs device activity afterward.

mod fake;
mod timing;
mod transaction;

pub mod contract_suite;

pub use fake::FakeBackend;
pub use timing::{
    price_operation, CacheFillPosition, CacheKind, ChipConfig, CostClass, CostComponent,
    CostExpression, CostTier, FlashMode, InstructionCost, MmioTier, Operation, PsramMode,
    ReceiptId, RefusalReason, TimingMutation, TimingRefusal,
};
pub use transaction::{
    Backend, CoreId, CoreState, ExecutionOutcome, LedgerEntry, SchedulerState, TraceEvent,
    TraceReport, TransactionEngine, TransactionReceipt,
};

/// Cycle count in the emulator's virtual clock domain.
pub type VirtualCycle = u64;

/// Failure from advancing an autonomous model's virtual clock.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeadlineError {
    TimeReversed {
        current: VirtualCycle,
        requested: VirtualCycle,
    },
}

/// Future-facing device and board timing contract.
///
/// `next_deadline` returns the earliest transition strictly after the model's
/// current cycle. `advance_to` applies every transition through `cycle` at its
/// scheduled timestamp and returns with no deadline at or before `cycle`.
pub trait DeadlineModel {
    fn next_deadline(&self) -> Option<VirtualCycle>;
    fn advance_to(&mut self, cycle: VirtualCycle) -> Result<(), DeadlineError>;
}
