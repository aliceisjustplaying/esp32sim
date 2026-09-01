//! Receipt-backed measured adapter for the ESP32-S3 interpreter.

use backend_api::{
    price_operation, Backend, CoreId, CostComponent, ExecutionOutcome, InterruptLevel,
    InterruptPhase, Operation, RefusalReason, TierCandidate, TimingMutation, TimingRefusal,
    TransactionEngine, TransactionReceipt,
};
use xtensa_lx7::measured::{complete_instruction, plan_instruction, CompletionError, PlanError};
use xtensa_lx7::measured::{InstructionObservation, MemoryClass, TimingPlan, TimingSource};
use xtensa_lx7::state::{Cpu, INTTYPE_LEVEL, INT_LEVEL};
use xtensa_lx7::{Op, Trap};

/// Product adapter. Fake and product adapters both delegate scheduling state,
/// transactional commit, and canonical ledger generation to `TransactionEngine`.
#[derive(Clone, Debug, Default)]
pub struct Esp32Backend {
    engine: TransactionEngine,
}

impl Esp32Backend {
    fn operation_for(observation: &InstructionObservation) -> Operation {
        match observation.instruction.op {
            Op::Beqz | Op::BeqzN => Operation::BranchZero {
                taken: observation
                    .branch_taken
                    .expect("a decoded zero branch records its outcome"),
            },
            Op::Rfe | Op::Rfue => Operation::Interrupt {
                level: InterruptLevel::Level1,
                phase: InterruptPhase::Resume,
            },
            Op::Rfi => Operation::Interrupt {
                level: interrupt_level(observation.instruction.imm as u8),
                phase: InterruptPhase::Resume,
            },
            _ => observation
                .access_memory
                .map_or(Operation::InternalInstruction, |memory| {
                    if memory == MemoryClass::Mmio {
                        Operation::UnknownMmio {
                            address: observation
                                .access
                                .expect("classified memory access has an access shape")
                                .address,
                        }
                    } else {
                        Operation::InternalInstruction
                    }
                }),
        }
    }

    /// Accept the highest-priority pending interrupt as one priced transaction.
    /// Unsupported levels restore the CPU and leave timing state unchanged.
    pub fn accept_interrupt(
        &mut self,
        core: CoreId,
        cpu: &mut Cpu,
    ) -> Result<Option<(u32, TransactionReceipt)>, TimingRefusal> {
        let before = cpu.clone();
        let Some(Trap::Interrupt(irq)) = cpu.check_interrupts() else {
            return Ok(None);
        };
        let operation = Operation::Interrupt {
            level: interrupt_level(INT_LEVEL[irq as usize]),
            phase: InterruptPhase::Entry,
        };
        match self.execute(backend_api::TraceEvent {
            core,
            pc: before.pc,
            operation,
            outcome: ExecutionOutcome::Committed,
        }) {
            Ok(receipt) => Ok(Some((irq, receipt))),
            Err(refusal) => {
                *cpu = before;
                Err(refusal)
            }
        }
    }
}

const fn interrupt_level(level: u8) -> InterruptLevel {
    match level {
        1 => InterruptLevel::Level1,
        3 => InterruptLevel::Level3,
        other => InterruptLevel::Other(other),
    }
}

impl Backend for Esp32Backend {
    fn engine(&self) -> &TransactionEngine {
        &self.engine
    }

    fn engine_mut(&mut self) -> &mut TransactionEngine {
        &mut self.engine
    }
}

impl TimingSource for Esp32Backend {
    fn price(&self, observation: &InstructionObservation) -> Result<TimingPlan, TimingRefusal> {
        let operation = Self::operation_for(observation);
        let (component, mutation) = price_operation(observation.core, operation)?;
        let cycles = component.cycles().ok_or(TimingRefusal {
            class: component.class,
            tier_candidate: TierCandidate::Unexplained,
            reason: RefusalReason::CycleOverflow,
        })?;
        Ok(TimingPlan {
            cycles,
            components: vec![component],
            mutations: mutation.into_iter().collect(),
        })
    }

    fn commit(
        &mut self,
        observation: &InstructionObservation,
        components: &[CostComponent],
        mutations: &[TimingMutation],
    ) -> Result<(), TimingRefusal> {
        self.engine.execute_priced(
            observation.core,
            observation.pc,
            ExecutionOutcome::Committed,
            components.to_vec(),
            mutations.to_vec(),
        )?;
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MeasuredStep {
    Instruction,
    Interrupt(u32),
    Trap(Trap),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MeasuredStepError {
    Timing(TimingRefusal),
    Plan(PlanError),
    Completion(CompletionError),
    Deadline(backend_api::DeadlineError),
}

impl crate::Machine {
    /// Execute one receipt-priced transaction on one of the two native cores.
    pub fn step_measured(
        &mut self,
        backend: &mut Esp32Backend,
        core: CoreId,
    ) -> Result<MeasuredStep, MeasuredStepError> {
        self.refresh_measured_interrupt_lines();
        let before_cycle = backend.engine().state().cores[core_index(core)].cycle;
        let interrupt = {
            let cpu = match core {
                CoreId::Core0 => &mut self.cpu,
                CoreId::Core1 => &mut self.cpu1,
            };
            backend
                .accept_interrupt(core, cpu)
                .map_err(MeasuredStepError::Timing)?
        };
        if let Some((irq, receipt)) = interrupt {
            let completion = receipt
                .entry
                .expect("accepted interrupt has a ledger entry")
                .completion;
            self.advance_measured_clocks(core, completion.saturating_sub(before_cycle));
            self.advance_measured_devices(completion)?;
            self.interrupts = self.interrupts.saturating_add(1);
            self.irq_hist[core_index(core)][irq as usize] =
                self.irq_hist[core_index(core)][irq as usize].saturating_add(1);
            return Ok(MeasuredStep::Interrupt(irq));
        }

        let pending = {
            let cpu = match core {
                CoreId::Core0 => &self.cpu,
                CoreId::Core1 => &self.cpu1,
            };
            plan_instruction(core, cpu, &self.bus, backend, before_cycle)
                .map_err(MeasuredStepError::Plan)?
        };
        let completion = pending.completion;
        let result = {
            let cpu = match core {
                CoreId::Core0 => &mut self.cpu,
                CoreId::Core1 => &mut self.cpu1,
            };
            complete_instruction(cpu, &mut self.bus, backend, pending, completion)
        };
        match result {
            Ok(()) => {
                self.advance_measured_clocks(core, completion.saturating_sub(before_cycle));
                self.advance_measured_devices(completion)?;
                Ok(MeasuredStep::Instruction)
            }
            Err(CompletionError::Trap(trap)) => Ok(MeasuredStep::Trap(trap)),
            Err(error) => Err(MeasuredStepError::Completion(error)),
        }
    }

    /// Earliest autonomous board transition in the shared deadline clock.
    pub fn next_measured_deadline(&self) -> Option<backend_api::VirtualCycle> {
        self.bus.board.next_deadline()
    }

    /// Deliver exactly timestamped device and board transitions through `cycle`.
    pub fn advance_measured_devices(
        &mut self,
        cycle: backend_api::VirtualCycle,
    ) -> Result<(), MeasuredStepError> {
        if cycle > self.bus.cycles {
            self.bus
                .advance_measured_to(cycle)
                .map_err(MeasuredStepError::Deadline)?;
        }
        self.refresh_measured_interrupt_lines();
        Ok(())
    }

    fn advance_measured_clocks(&mut self, core: CoreId, mut cycles: u64) {
        let cpu = match core {
            CoreId::Core0 => &mut self.cpu,
            CoreId::Core1 => &mut self.cpu1,
        };
        while cycles != 0 {
            let step = cycles.min(u64::from(u32::MAX)) as u32;
            cpu.advance_ccount(step);
            cycles -= u64::from(step);
        }
    }

    fn refresh_measured_interrupt_lines(&mut self) {
        let dirty = self.bus.periph.lines_dirty() || self.bus.periph.intmatrix_dirty;
        if self.bus.irq_dirty || dirty {
            self.bus.irq_dirty = false;
            self.bus.periph.intmatrix_dirty = false;
            let (core0, core1) = self.bus.periph.cpu_lines_both();
            self.cpu.interrupt = (self.cpu.interrupt & !INTTYPE_LEVEL) | (core0 & INTTYPE_LEVEL);
            self.cpu1.interrupt = (self.cpu1.interrupt & !INTTYPE_LEVEL) | (core1 & INTTYPE_LEVEL);
        }
    }
}

const fn core_index(core: CoreId) -> usize {
    match core {
        CoreId::Core0 => 0,
        CoreId::Core1 => 1,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::board::WaveshareAmoled18V2;
    use backend_api::contract_suite::assert_backend_contract;
    use backend_api::{CacheFillPosition, CacheKind, CostClass, ReceiptId};
    use xtensa_lx7::state::ps;

    const RESET_PC: u32 = 0x4000_0400;
    const BEQZ_N_A6: [u8; 2] = [0x8c, 0x06];

    fn branch_machine(register: u32) -> crate::Machine {
        let mut machine = crate::Machine::new([0; 6]);
        machine
            .bus
            .load_bytes(RESET_PC, &BEQZ_N_A6)
            .expect("test branch maps in mask ROM");
        machine.cpu.set_ar(6, register);
        machine
    }

    fn measured_branch_ledger(register: u32) -> Vec<u8> {
        let mut machine = branch_machine(register);
        let mut backend = Esp32Backend::default();
        assert_eq!(
            machine.step_measured(&mut backend, CoreId::Core0),
            Ok(MeasuredStep::Instruction)
        );
        backend
            .run_trace(&[])
            .expect("empty suffix preserves the completed ledger")
            .canonical_ledger
    }

    #[test]
    fn real_backend_passes_the_same_contract_as_fake() {
        assert_backend_contract::<Esp32Backend>();
    }

    #[test]
    fn measured_interpreter_commits_receipt_correlated_branch_end_to_end() {
        for (register, expected) in [(0, 3), (1, 1)] {
            let mut machine = branch_machine(register);
            let mut backend = Esp32Backend::default();
            assert_eq!(
                machine.step_measured(&mut backend, CoreId::Core0),
                Ok(MeasuredStep::Instruction)
            );
            let entry = &backend.engine().ledger()[0];
            assert_eq!(entry.start, 0);
            assert_eq!(entry.completion, expected);
            assert_eq!(entry.components[0].receipt, ReceiptId::BeqzAdoption2bf3ffd);
            assert_eq!(machine.cpu.ccount, expected as u32);
            assert_eq!(machine.bus.cycles, expected);
        }
    }

    #[test]
    fn real_interpreter_ledger_is_byte_identical_twice() {
        assert_eq!(measured_branch_ledger(0), measured_branch_ledger(0));
    }

    #[test]
    fn board_deadline_is_delivered_during_measured_instruction() {
        let mut machine = branch_machine(0);
        machine.bus.board = Box::new(WaveshareAmoled18V2::new());
        machine.bus.board.touch(80, 90, true);
        assert_eq!(machine.next_measured_deadline(), Some(1));
        let mut backend = Esp32Backend::default();
        assert_eq!(
            machine.step_measured(&mut backend, CoreId::Core0),
            Ok(MeasuredStep::Instruction)
        );
        assert_eq!(machine.bus.cycles, 3);
        assert_eq!(machine.bus.periph.gpio.input & (1 << 21), 0);
        assert!(machine
            .next_measured_deadline()
            .is_some_and(|cycle| cycle > 3));
    }

    #[test]
    fn faulted_real_instruction_rolls_back_timing_and_ledger() {
        let mut machine = branch_machine(0);
        machine.cpu.ps = ps::WOE;
        machine.cpu.windowbase = 0;
        machine.cpu.windowstart = 1 << 1;
        let mut backend = Esp32Backend::default();
        let before = backend.engine().state().clone();
        assert!(matches!(
            machine.step_measured(&mut backend, CoreId::Core0),
            Ok(MeasuredStep::Trap(Trap::Exception(_)))
        ));
        assert_eq!(backend.engine().state(), &before);
        assert!(backend.engine().ledger().is_empty());
        assert_eq!(machine.bus.cycles, 0);
    }

    #[test]
    fn measured_adapter_does_not_change_fast_interpreter_state() {
        let mut planned = branch_machine(0);
        let mut control = branch_machine(0);
        let backend = Esp32Backend::default();
        let _ = plan_instruction(CoreId::Core0, &planned.cpu, &planned.bus, &backend, 0)
            .expect("planning succeeds");
        assert_eq!(
            xtensa_lx7::step(&mut planned.cpu, &mut planned.bus),
            xtensa_lx7::step(&mut control.cpu, &mut control.bus)
        );
        assert_eq!(planned.cpu.pc, control.cpu.pc);
        assert_eq!(planned.cpu.ps, control.cpu.ps);
        assert_eq!(planned.cpu.ccount, control.cpu.ccount);
        assert_eq!(planned.cpu.insn_count, control.cpu.insn_count);
        assert_eq!(planned.bus.cycles, control.bus.cycles);
    }

    #[test]
    fn accepted_level_one_and_three_interrupts_are_priced_ledger_transactions() {
        for (irq, expected) in [(0, 227), (11, 222)] {
            let mut backend = Esp32Backend::default();
            let mut cpu = Cpu::new(0);
            cpu.ps = 0;
            cpu.intenable = 1 << irq;
            cpu.interrupt = 1 << irq;
            let (_, receipt) = backend
                .accept_interrupt(CoreId::Core0, &mut cpu)
                .expect("adopted interrupt level prices")
                .expect("pending interrupt is accepted");
            let entry = receipt.entry.expect("acceptance is ledgered");
            assert_eq!(entry.completion, expected);
            assert_eq!(entry.components[0].receipt, ReceiptId::Idf61ToolchainDelta);
            assert!(matches!(
                entry.components[0].class,
                CostClass::Interrupt {
                    phase: InterruptPhase::Entry,
                    ..
                }
            ));
            assert_eq!(
                backend
                    .engine()
                    .state()
                    .cores
                    .map(|core| core.committed_instructions),
                [0, 0]
            );
            assert_eq!(backend.engine().state().committed_interrupt_entries[0], 1);
        }
    }

    #[test]
    fn unsupported_interrupt_level_fails_closed_without_cpu_or_ledger_change() {
        let mut backend = Esp32Backend::default();
        let mut cpu = Cpu::new(0);
        cpu.ps = 0;
        cpu.intenable = 1 << 19;
        cpu.interrupt = 1 << 19;
        let before = cpu.clone();
        let refusal = backend
            .accept_interrupt(CoreId::Core0, &mut cpu)
            .expect_err("level 2 interrupt timing is not adopted");
        assert_eq!(
            refusal.class,
            CostClass::Interrupt {
                level: InterruptLevel::Other(2),
                phase: InterruptPhase::Entry,
            }
        );
        assert_eq!(cpu.pc, before.pc);
        assert_eq!(cpu.ps, before.ps);
        assert!(backend.engine().ledger().is_empty());
    }

    #[test]
    fn direct_real_backend_uses_shared_receipt_engine() {
        let mut backend = Esp32Backend::default();
        let receipt = backend
            .execute(backend_api::TraceEvent {
                core: CoreId::Core1,
                pc: 0x4200_0000,
                operation: Operation::CacheLineFill {
                    cache: CacheKind::DataPsram,
                    position: CacheFillPosition::Subsequent,
                    line: 7,
                },
                outcome: ExecutionOutcome::Committed,
            })
            .expect("adopted operation executes")
            .entry
            .expect("committed operation has a ledger entry");
        assert_eq!(receipt.completion, 170);
        assert_eq!(
            receipt.components[0].receipt,
            ReceiptId::CacheBurstAdoptionA91d1d7
        );
    }
}
