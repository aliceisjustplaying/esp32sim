use crate::{price_operation, CostComponent, TimingMutation, TimingRefusal, VirtualCycle};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum CoreId {
    Core0,
    Core1,
}

impl CoreId {
    pub const ALL: [Self; 2] = [Self::Core0, Self::Core1];

    const fn index(self) -> usize {
        match self {
            Self::Core0 => 0,
            Self::Core1 => 1,
        }
    }

    const fn encoded(self) -> u8 {
        match self {
            Self::Core0 => 0,
            Self::Core1 => 1,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CoreState {
    pub cycle: VirtualCycle,
    pub committed_instructions: u64,
}

/// Scheduler state is dual-core structurally, including before contention
/// calibration is adopted.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SchedulerState {
    pub cores: [CoreState; 2],
    pub last_mmio_write: Option<(u32, u32, u32)>,
    pub committed_cache_fills: u64,
    pub committed_loop_edges: [u64; 2],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExecutionOutcome {
    Committed,
    Faulted,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TraceEvent {
    pub core: CoreId,
    pub pc: u32,
    pub operation: crate::Operation,
    pub outcome: ExecutionOutcome,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LedgerEntry {
    pub core: CoreId,
    pub pc: u32,
    pub start: VirtualCycle,
    pub completion: VirtualCycle,
    pub components: Vec<CostComponent>,
}

impl LedgerEntry {
    fn canonical_bytes(&self, output: &mut Vec<u8>) {
        output.push(self.core.encoded());
        output.extend_from_slice(&self.pc.to_le_bytes());
        output.extend_from_slice(&self.start.to_le_bytes());
        output.extend_from_slice(&self.completion.to_le_bytes());
        output.extend_from_slice(&(self.components.len() as u32).to_le_bytes());
        for component in &self.components {
            encode_component(*component, output);
        }
    }
}

fn encode_component(component: CostComponent, output: &mut Vec<u8>) {
    output.extend_from_slice(&format_free_class_code(component.class).to_le_bytes());
    output.push(tier_code(component.tier));
    match component.expression {
        crate::CostExpression::Exact(cycles) => {
            output.push(0);
            output.extend_from_slice(&cycles.to_le_bytes());
        }
        crate::CostExpression::Affine {
            slope,
            intercept,
            count,
        } => {
            output.push(1);
            output.extend_from_slice(&slope.to_le_bytes());
            output.extend_from_slice(&intercept.to_le_bytes());
            output.extend_from_slice(&count.to_le_bytes());
        }
    }
    output.push(receipt_code(component.receipt));
}

const fn format_free_class_code(class: crate::CostClass) -> u16 {
    match class {
        crate::CostClass::BranchZero { taken: false } => 0,
        crate::CostClass::BranchZero { taken: true } => 1,
        crate::CostClass::SameValueMmioWriteRun { .. } => 2,
        crate::CostClass::CacheLineFill {
            cache: crate::CacheKind::InstructionFlash,
            position: crate::CacheFillPosition::First,
        } => 3,
        crate::CostClass::CacheLineFill {
            cache: crate::CacheKind::InstructionFlash,
            position: crate::CacheFillPosition::Subsequent,
        } => 4,
        crate::CostClass::CacheLineFill {
            cache: crate::CacheKind::DataFlash,
            position: crate::CacheFillPosition::First,
        } => 5,
        crate::CostClass::CacheLineFill {
            cache: crate::CacheKind::DataFlash,
            position: crate::CacheFillPosition::Subsequent,
        } => 6,
        crate::CostClass::CacheLineFill {
            cache: crate::CacheKind::DataPsram,
            position: crate::CacheFillPosition::First,
        } => 7,
        crate::CostClass::CacheLineFill {
            cache: crate::CacheKind::DataPsram,
            position: crate::CacheFillPosition::Subsequent,
        } => 8,
        crate::CostClass::LoopAlignment { .. } => 10,
        crate::CostClass::InternalInstruction => 11,
        crate::CostClass::UnknownMmio => 12,
    }
}

const fn tier_code(tier: crate::CostTier) -> u8 {
    match tier {
        crate::CostTier::Exact => 0,
        crate::CostTier::Affine => 1,
        crate::CostTier::Interval => 2,
        crate::CostTier::Distribution => 3,
        crate::CostTier::Unexplained => 4,
    }
}

const fn receipt_code(receipt: crate::ReceiptId) -> u8 {
    match receipt {
        crate::ReceiptId::BeqzAdoption2bf3ffd => 0,
        crate::ReceiptId::MmioWriteAdoptionE8a9f0e => 1,
        crate::ReceiptId::CacheBurstAdoptionA91d1d7 => 2,
        crate::ReceiptId::Idf61ToolchainDelta => 3,
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TransactionReceipt {
    pub entry: Option<LedgerEntry>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TraceReport {
    pub total_cycles: u64,
    pub ledger: Vec<LedgerEntry>,
    pub canonical_ledger: Vec<u8>,
}

/// Shared transaction engine used by fake and real backends.
#[derive(Clone, Debug, Default)]
pub struct TransactionEngine {
    state: SchedulerState,
    ledger: Vec<LedgerEntry>,
}

impl TransactionEngine {
    pub fn state(&self) -> &SchedulerState {
        &self.state
    }

    pub fn ledger(&self) -> &[LedgerEntry] {
        &self.ledger
    }

    pub fn execute(&mut self, event: TraceEvent) -> Result<TransactionReceipt, TimingRefusal> {
        let (component, mutation) = price_operation(event.core, event.operation)?;
        self.execute_priced(
            event.core,
            event.pc,
            event.outcome,
            vec![component],
            mutation.into_iter().collect(),
        )
    }

    /// Commit a transaction priced by an interpreter adapter through the same
    /// state and ledger path used by `execute`.
    pub fn execute_priced(
        &mut self,
        core: CoreId,
        pc: u32,
        outcome: ExecutionOutcome,
        components: Vec<CostComponent>,
        mutations: Vec<TimingMutation>,
    ) -> Result<TransactionReceipt, TimingRefusal> {
        let start = self.state.cores[core.index()].cycle;
        let cycles = components.iter().try_fold(0u64, |total, component| {
            total.checked_add(component.cycles()?)
        });
        let cycles = cycles.ok_or(TimingRefusal {
            class: components
                .first()
                .map_or(crate::CostClass::InternalInstruction, |component| {
                    component.class
                }),
            tier_candidate: crate::CostTier::Unexplained,
            reason: crate::RefusalReason::CycleOverflow,
        })?;
        let completion = start.checked_add(cycles).ok_or(TimingRefusal {
            class: components
                .first()
                .map_or(crate::CostClass::InternalInstruction, |component| {
                    component.class
                }),
            tier_candidate: crate::CostTier::Unexplained,
            reason: crate::RefusalReason::CycleOverflow,
        })?;
        if outcome == ExecutionOutcome::Faulted {
            return Ok(TransactionReceipt { entry: None });
        }
        for mutation in mutations {
            self.commit_mutation(Some(mutation));
        }
        let state = &mut self.state.cores[core.index()];
        state.cycle = completion;
        state.committed_instructions = state.committed_instructions.saturating_add(1);
        let entry = LedgerEntry {
            core,
            pc,
            start,
            completion,
            components,
        };
        self.ledger.push(entry.clone());
        Ok(TransactionReceipt { entry: Some(entry) })
    }

    fn commit_mutation(&mut self, mutation: Option<TimingMutation>) {
        match mutation {
            Some(TimingMutation::RecordMmioWrite {
                address,
                value,
                count,
            }) => self.state.last_mmio_write = Some((address, value, count)),
            Some(TimingMutation::RecordCacheFill { .. }) => {
                self.state.committed_cache_fills =
                    self.state.committed_cache_fills.saturating_add(1);
            }
            Some(TimingMutation::RecordLoopBackEdge { core, .. }) => {
                let count = &mut self.state.committed_loop_edges[core.index()];
                *count = count.saturating_add(1);
            }
            None => {}
        }
    }

    pub fn run_trace(&mut self, trace: &[TraceEvent]) -> Result<TraceReport, TimingRefusal> {
        for event in trace {
            self.execute(*event)?;
        }
        let total_cycles = self
            .state
            .cores
            .iter()
            .try_fold(0u64, |total, core| total.checked_add(core.cycle))
            .ok_or(TimingRefusal {
                class: crate::CostClass::InternalInstruction,
                tier_candidate: crate::CostTier::Unexplained,
                reason: crate::RefusalReason::CycleOverflow,
            })?;
        Ok(TraceReport {
            total_cycles,
            ledger: self.ledger.clone(),
            canonical_ledger: canonical_ledger_bytes(&self.ledger),
        })
    }
}

pub fn canonical_ledger_bytes(entries: &[LedgerEntry]) -> Vec<u8> {
    let mut output = Vec::new();
    output.extend_from_slice(&1u16.to_le_bytes());
    output.extend_from_slice(&(entries.len() as u64).to_le_bytes());
    for entry in entries {
        entry.canonical_bytes(&mut output);
    }
    output
}

pub trait Backend {
    fn engine(&self) -> &TransactionEngine;
    fn engine_mut(&mut self) -> &mut TransactionEngine;

    fn execute(&mut self, event: TraceEvent) -> Result<TransactionReceipt, TimingRefusal> {
        self.engine_mut().execute(event)
    }

    fn run_trace(&mut self, trace: &[TraceEvent]) -> Result<TraceReport, TimingRefusal> {
        self.engine_mut().run_trace(trace)
    }
}
