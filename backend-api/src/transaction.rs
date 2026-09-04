use crate::{
    price_operation, ChipConfig, CostComponent, TimingMutation, TimingRefusal, VirtualCycle,
};

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
    pub posted_mmio_writes: u8,
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
        crate::CostClass::Instruction(kind) => 20 + instruction_code(kind),
        crate::CostClass::MmioRead(tier) => 40 + mmio_code(tier),
        crate::CostClass::MmioWriteEnqueue => 50,
        crate::CostClass::MmioWriteDrain(tier) => 51 + mmio_code(tier),
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
        crate::CostClass::IndependentSramAccess => 11,
        crate::CostClass::HotCacheHit => 12,
        crate::CostClass::DmaAdditiveDelay => 13,
        crate::CostClass::UnadoptedInstruction => 14,
        crate::CostClass::UnknownMmio => 15,
    }
}

const fn instruction_code(kind: crate::InstructionCost) -> u16 {
    match kind {
        crate::InstructionCost::Issue => 0,
        crate::InstructionCost::Branch { taken: false } => 1,
        crate::InstructionCost::Branch { taken: true } => 2,
        crate::InstructionCost::Jump => 3,
        crate::InstructionCost::JumpRegister => 4,
        crate::InstructionCost::LoopSetup => 5,
        crate::InstructionCost::Quotient => 6,
        crate::InstructionCost::Remainder => 7,
        crate::InstructionCost::AtomicStore => 8,
        crate::InstructionCost::LoadUse => 9,
        crate::InstructionCost::LiteralLoad => 10,
        crate::InstructionCost::InstructionSync => 11,
    }
}

const fn mmio_code(tier: crate::MmioTier) -> u16 {
    match tier {
        crate::MmioTier::Fast => 0,
        crate::MmioTier::Apb => 1,
        crate::MmioTier::Nrx => 2,
        crate::MmioTier::Rtc => 3,
        crate::MmioTier::Efuse => 4,
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
        crate::ReceiptId::Idf61ToolchainDelta => 0,
        crate::ReceiptId::OpcodeLadders => 1,
        crate::ReceiptId::RegisterBlocks => 2,
        crate::ReceiptId::HotHitAdoption => 3,
        crate::ReceiptId::CacheBurstAdoptionA91d1d7 => 4,
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
#[derive(Clone, Debug)]
pub struct TransactionEngine {
    state: SchedulerState,
    ledger: Vec<LedgerRecord>,
    config: ChipConfig,
}

#[derive(Clone, Debug)]
enum LedgerRecord {
    Entry(LedgerEntry),
    StaticSramBatch {
        entries: Vec<CompactLedgerEntry>,
        delta: [u64; 2],
        repetitions: u64,
    },
}

#[derive(Clone, Copy, Debug)]
struct CompactLedgerEntry {
    core: CoreId,
    pc: u32,
    start: VirtualCycle,
    completion: VirtualCycle,
    component: CostComponent,
}

#[derive(Clone, Debug)]
pub struct TransactionCheckpoint {
    state: SchedulerState,
    ledger_len: usize,
    tail_repetitions: Option<u64>,
    config: ChipConfig,
}

impl Default for TransactionEngine {
    fn default() -> Self {
        Self {
            state: SchedulerState::default(),
            ledger: Vec::new(),
            config: ChipConfig::RECEIPT_SCOPE,
        }
    }
}

impl TransactionEngine {
    pub fn checkpoint(&self) -> TransactionCheckpoint {
        TransactionCheckpoint {
            state: self.state.clone(),
            ledger_len: self.ledger.len(),
            tail_repetitions: match self.ledger.last() {
                Some(LedgerRecord::StaticSramBatch { repetitions, .. }) => Some(*repetitions),
                _ => None,
            },
            config: self.config,
        }
    }

    pub fn rollback(&mut self, checkpoint: TransactionCheckpoint) {
        self.state = checkpoint.state;
        self.ledger.truncate(checkpoint.ledger_len);
        if let (Some(repetitions), Some(LedgerRecord::StaticSramBatch { repetitions: tail, .. })) =
            (checkpoint.tail_repetitions, self.ledger.last_mut())
        {
            *tail = repetitions;
        }
        self.config = checkpoint.config;
    }

    pub fn state(&self) -> &SchedulerState {
        &self.state
    }

    pub fn ledger(&self) -> Vec<LedgerEntry> {
        self.expanded_ledger()
    }

    pub fn execute(&mut self, event: TraceEvent) -> Result<TransactionReceipt, TimingRefusal> {
        let (component, mutation) = price_operation(self.config, event.core, event.operation)?;
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
                .map_or(crate::CostClass::UnadoptedInstruction, |component| {
                    component.class
                }),
            tier_candidate: crate::CostTier::Unexplained,
            reason: crate::RefusalReason::CycleOverflow,
            configuration: None,
        })?;
        let completion = start.checked_add(cycles).ok_or(TimingRefusal {
            class: components
                .first()
                .map_or(crate::CostClass::UnadoptedInstruction, |component| {
                    component.class
                }),
            tier_candidate: crate::CostTier::Unexplained,
            reason: crate::RefusalReason::CycleOverflow,
            configuration: None,
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
        self.ledger.push(LedgerRecord::Entry(entry.clone()));
        Ok(TransactionReceipt { entry: Some(entry) })
    }

    /// Commit a validated receipt-static SRAM batch without allocating per-event components.
    pub fn execute_static_sram_batch(
        &mut self,
        events: &[(CoreId, u32, CostComponent)],
    ) -> Result<(), TimingRefusal> {
        let mut cycles = self.state.cores.map(|core| core.cycle);
        let base = cycles;
        if let Some(LedgerRecord::StaticSramBatch { entries, delta, repetitions }) = self.ledger.last_mut() {
            let same = entries.len() == events.len() && entries.iter().zip(events).all(|(entry, event)| {
                entry.core == event.0 && entry.pc == event.1 && entry.component == event.2
            });
            let expected = [
                entries.iter().find(|entry| entry.core == CoreId::Core0).map_or(base[0], |entry| entry.start)
                    .checked_add(delta[0].saturating_mul(*repetitions)),
                entries.iter().find(|entry| entry.core == CoreId::Core1).map_or(base[1], |entry| entry.start)
                    .checked_add(delta[1].saturating_mul(*repetitions)),
            ];
            if same && expected == [Some(base[0]), Some(base[1])] {
                for &(core, _, component) in events {
                    let index = core.index();
                    cycles[index] = cycles[index].checked_add(component.cycles().ok_or(TimingRefusal {
                        class: component.class,
                        tier_candidate: crate::CostTier::Unexplained,
                        reason: crate::RefusalReason::CycleOverflow,
                        configuration: None,
                    })?).ok_or(TimingRefusal {
                        class: component.class,
                        tier_candidate: crate::CostTier::Unexplained,
                        reason: crate::RefusalReason::CycleOverflow,
                        configuration: None,
                    })?;
                }
                *repetitions = repetitions.checked_add(1).ok_or(TimingRefusal {
                    class: crate::CostClass::UnadoptedInstruction,
                    tier_candidate: crate::CostTier::Unexplained,
                    reason: crate::RefusalReason::CycleOverflow,
                    configuration: None,
                })?;
                for core in CoreId::ALL { self.state.cores[core.index()].cycle = cycles[core.index()]; }
                for &(core, _, _) in events {
                    self.state.cores[core.index()].committed_instructions = self.state.cores[core.index()].committed_instructions.saturating_add(1);
                }
                return Ok(());
            }
        }
        let mut compact = Vec::with_capacity(events.len());
        for &(core, pc, component) in events {
            let index = core.index();
            let start = cycles[index];
            let completion = start.checked_add(component.cycles().ok_or(TimingRefusal {
                class: component.class,
                tier_candidate: crate::CostTier::Unexplained,
                reason: crate::RefusalReason::CycleOverflow,
                configuration: None,
            })?).ok_or(TimingRefusal {
                class: component.class,
                tier_candidate: crate::CostTier::Unexplained,
                reason: crate::RefusalReason::CycleOverflow,
                configuration: None,
            })?;
            cycles[index] = completion;
            compact.push(CompactLedgerEntry { core, pc, start, completion, component });
        }
        for core in CoreId::ALL {
            let state = &mut self.state.cores[core.index()];
            state.cycle = cycles[core.index()];
        }
        for &(core, _, _) in events {
            let state = &mut self.state.cores[core.index()];
            state.committed_instructions = state.committed_instructions.saturating_add(1);
        }
        self.ledger.push(LedgerRecord::StaticSramBatch {
            entries: compact,
            delta: [cycles[0] - base[0], cycles[1] - base[1]],
            repetitions: 1,
        });
        Ok(())
    }

    fn commit_mutation(&mut self, mutation: Option<TimingMutation>) {
        match mutation {
            Some(TimingMutation::RecordMmioWrite) => {
                self.state.posted_mmio_writes = self.state.posted_mmio_writes.saturating_add(1);
            }
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
                class: crate::CostClass::UnadoptedInstruction,
                tier_candidate: crate::CostTier::Unexplained,
                reason: crate::RefusalReason::CycleOverflow,
                configuration: None,
            })?;
        Ok(TraceReport {
            total_cycles,
            ledger: self.expanded_ledger(),
            canonical_ledger: self.canonical_ledger_bytes(),
        })
    }


    fn expanded_ledger(&self) -> Vec<LedgerEntry> {
        let mut entries = Vec::new();
        for record in &self.ledger {
            match record {
                LedgerRecord::Entry(entry) => entries.push(entry.clone()),
                LedgerRecord::StaticSramBatch { entries: batch, delta, repetitions } => {
                    for repetition in 0..*repetitions {
                        entries.extend(batch.iter().map(|entry| {
                            let shift = delta[entry.core.index()] * repetition;
                            LedgerEntry {
                                core: entry.core,
                                pc: entry.pc,
                                start: entry.start + shift,
                                completion: entry.completion + shift,
                                components: vec![entry.component],
                            }
                        }));
                    }
                }
            }
        }
        entries
    }

    fn canonical_ledger_bytes(&self) -> Vec<u8> {
        let entry_count = self.ledger.iter().map(|record| match record {
            LedgerRecord::Entry(_) => 1usize,
            LedgerRecord::StaticSramBatch { entries, repetitions, .. } => entries.len() * *repetitions as usize,
        }).sum::<usize>();
        let mut output = Vec::new();
        output.extend_from_slice(&1u16.to_le_bytes());
        output.extend_from_slice(&(entry_count as u64).to_le_bytes());
        for record in &self.ledger {
            match record {
                LedgerRecord::Entry(entry) => entry.canonical_bytes(&mut output),
                LedgerRecord::StaticSramBatch { entries, delta, repetitions } => {
                    for repetition in 0..*repetitions {
                        for entry in entries {
                            let shift = delta[entry.core.index()] * repetition;
                            output.push(entry.core.encoded());
                            output.extend_from_slice(&entry.pc.to_le_bytes());
                            output.extend_from_slice(&(entry.start + shift).to_le_bytes());
                            output.extend_from_slice(&(entry.completion + shift).to_le_bytes());
                            output.extend_from_slice(&1u32.to_le_bytes());
                            encode_component(entry.component, &mut output);
                        }
                    }
                }
            }
        }
        output
    }
}

#[cfg(test)]
fn canonical_ledger_bytes(entries: &[LedgerEntry]) -> Vec<u8> {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compact_sram_batch_preserves_canonical_ledger() {
        let mut engine = TransactionEngine::default();
        engine.execute(TraceEvent {
            core: CoreId::Core0,
            pc: 0x4037_0000,
            operation: crate::Operation::Instruction(crate::InstructionCost::Issue),
            outcome: ExecutionOutcome::Committed,
        }).unwrap();
        let issue = price_operation(
            ChipConfig::RECEIPT_SCOPE,
            CoreId::Core0,
            crate::Operation::Instruction(crate::InstructionCost::Issue),
        ).unwrap().0;
        let jump = price_operation(
            ChipConfig::RECEIPT_SCOPE,
            CoreId::Core1,
            crate::Operation::Instruction(crate::InstructionCost::Jump),
        ).unwrap().0;
        engine.execute_static_sram_batch(&[
            (CoreId::Core1, 0x4037_0100, jump),
            (CoreId::Core0, 0x4037_0003, issue),
        ]).unwrap();

        let expanded = engine.expanded_ledger();
        assert_eq!(engine.canonical_ledger_bytes(), canonical_ledger_bytes(&expanded));
    }
}
