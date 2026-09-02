//! Receipt-backed measured adapter for the ESP32-S3 interpreter.

use backend_api::{
    price_operation, Backend, ChipConfig, CoreId, CostComponent, ExecutionOutcome, FlashMode,
    Operation, PsramMode, RefusalReason, TimingMutation, TimingRefusal, TransactionEngine,
};
use xtensa_lx7::measured::{complete_instruction, plan_instruction, CompletionError, PlanError};
use xtensa_lx7::measured::{InstructionObservation, MemoryClass, TimingPlan, TimingSource};
use xtensa_lx7::state::INTTYPE_LEVEL;
use xtensa_lx7::{Op, Trap};

/// Product adapter. Fake and product adapters both delegate scheduling state,
/// transactional commit, and canonical ledger generation to `TransactionEngine`.
#[derive(Clone, Debug)]
pub struct Esp32Backend {
    engine: TransactionEngine,
    config: ChipConfig,
}

impl Default for Esp32Backend {
    fn default() -> Self {
        Self {
            engine: TransactionEngine::default(),
            config: ChipConfig::RECEIPT_SCOPE,
        }
    }
}

impl Esp32Backend {
    fn operation_for(
        &self,
        observation: &InstructionObservation,
    ) -> Result<Operation, TimingRefusal> {
        match observation.instruction.op {
            Op::Beqz | Op::BeqzN => Ok(Operation::BranchZero {
                taken: observation
                    .branch_taken
                    .expect("a decoded zero branch records its outcome"),
            }),
            _ => Ok(observation
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
                })),
        }
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
        let operation = self.operation_for(observation)?;
        let (component, mutation) = price_operation(self.config, observation.core, operation)?;
        let cycles = component.cycles().ok_or(TimingRefusal {
            class: component.class,
            tier_candidate: backend_api::CostTier::Unexplained,
            reason: RefusalReason::CycleOverflow,
            configuration: None,
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
    Plan(PlanError),
    Completion(CompletionError),
    Deadline(crate::board::BoardDeadlineError),
}

/// Receipt-priced execution operations for an ESP32-S3 machine.
pub trait MeasuredMachine {
    /// Execute one receipt-priced transaction on one of the two native cores.
    fn step_measured(
        &mut self,
        backend: &mut Esp32Backend,
        core: CoreId,
    ) -> Result<MeasuredStep, MeasuredStepError>;

    /// Earliest autonomous board transition in the shared deadline clock.
    fn next_measured_deadline(&self) -> Option<backend_api::VirtualCycle>;

    /// Deliver exactly timestamped device and board transitions through `cycle`.
    fn advance_measured_devices(
        &mut self,
        cycle: backend_api::VirtualCycle,
    ) -> Result<(), MeasuredStepError>;
}

impl MeasuredMachine for crate::Machine {
    fn step_measured(
        &mut self,
        backend: &mut Esp32Backend,
        core: CoreId,
    ) -> Result<MeasuredStep, MeasuredStepError> {
        backend.config = chip_config_from_registers(self);
        self.advance_measured_devices(self.bus.cycles)?;
        let index = core_index(core);
        let before_cycle = backend.engine().state().cores[index].cycle;
        let interrupt = self.cores[index].check_interrupts();
        if let Some(Trap::Interrupt(irq)) = interrupt {
            self.interrupts = self.interrupts.saturating_add(1);
            self.irq_hist[index][irq as usize] =
                self.irq_hist[index][irq as usize].saturating_add(1);
            return Ok(MeasuredStep::Interrupt(irq));
        }

        let pending = {
            let cpu = &self.cores[index];
            plan_instruction(core, cpu, &self.bus, backend, before_cycle)
                .map_err(MeasuredStepError::Plan)?
        };
        let completion = pending.completion;
        let result = complete_instruction(
            &mut self.cores[index],
            &mut self.bus,
            backend,
            pending,
            completion,
        );
        match result {
            Ok(()) => {
                advance_measured_clocks(self, core, completion.saturating_sub(before_cycle));
                self.advance_measured_devices(completion)?;
                Ok(MeasuredStep::Instruction)
            }
            Err(CompletionError::Trap(trap)) => Ok(MeasuredStep::Trap(trap)),
            Err(error) => Err(MeasuredStepError::Completion(error)),
        }
    }

    /// Earliest autonomous board transition in the shared deadline clock.
    fn next_measured_deadline(&self) -> Option<backend_api::VirtualCycle> {
        self.bus.board.next_deadline()
    }

    fn advance_measured_devices(
        &mut self,
        cycle: backend_api::VirtualCycle,
    ) -> Result<(), MeasuredStepError> {
        if cycle >= self.bus.cycles {
            self.bus
                .advance_measured_to(cycle)
                .map_err(MeasuredStepError::Deadline)?;
        }
        refresh_measured_interrupt_lines(self);
        Ok(())
    }
}

fn advance_measured_clocks(machine: &mut crate::Machine, core: CoreId, mut cycles: u64) {
    let cpu = &mut machine.cores[core_index(core)];
    while cycles != 0 {
        let step = cycles.min(u64::from(u32::MAX)) as u32;
        cpu.advance_ccount(step);
        cycles -= u64::from(step);
    }
}

fn refresh_measured_interrupt_lines(machine: &mut crate::Machine) {
    let dirty = machine.bus.periph.lines_dirty() || machine.bus.periph.intmatrix_dirty;
    if machine.bus.irq_dirty || dirty {
        machine.bus.irq_dirty = false;
        machine.bus.periph.intmatrix_dirty = false;
        let (core0, core1) = machine.bus.periph.cpu_lines_both();
        machine.cores[0].interrupt =
            (machine.cores[0].interrupt & !INTTYPE_LEVEL) | (core0 & INTTYPE_LEVEL);
        machine.cores[1].interrupt =
            (machine.cores[1].interrupt & !INTTYPE_LEVEL) | (core1 & INTTYPE_LEVEL);
    }
}

fn chip_config_from_registers(machine: &crate::Machine) -> ChipConfig {
    let system = &machine.bus.periph.system.ram;
    let spi0 = &machine.bus.periph.spi0.regs;
    let spi1 = &machine.bus.periph.spi1.regs;
    let extmem = &machine.bus.periph.extmem.ram;
    let cpu_mhz = match ((system.read(0x60) >> 10) & 3, system.read(0x10) & 3) {
        (1, 0) => 80,
        (1, 1) => 160,
        (1, 2) => 240,
        _ => 0,
    };
    let clock_mhz = |register: u32| {
        if register & (1 << 31) != 0 {
            160
        } else {
            160 / (((register >> 16) & 0xff) as u16 + 1)
        }
    };
    let flash0 = clock_mhz(spi0.read(0x14));
    let flash1 = clock_mhz(spi1.read(0x14));
    ChipConfig {
        cpu_mhz,
        flash_mode: if spi0.read(0x8) & (1 << 24) != 0 {
            FlashMode::Qio
        } else {
            FlashMode::Other
        },
        flash_mhz: if flash0 == flash1 { flash0 } else { 0 },
        psram_mode: if spi0.read(0x40) & (1 << 21) != 0 {
            PsramMode::OctalDtr
        } else {
            PsramMode::Other
        },
        psram_mhz: clock_mhz(spi0.read(0x50)),
        icache_line_bytes: if extmem.read(0x60) & (1 << 3) != 0 {
            32
        } else {
            16
        },
        dcache_line_bytes: match (extmem.read(0x0) >> 3) & 3 {
            0 => 16,
            1 => 32,
            2 => 64,
            _ => 0,
        },
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
    use backend_api::contract_suite::{assert_backend_contract, assert_receipt_correlation};
    use backend_api::{CacheFillPosition, CacheKind, ReceiptId};
    use xtensa_lx7::state::ps;

    const RESET_PC: u32 = 0x4000_0400;
    const BEQZ_N_A6: [u8; 2] = [0x8c, 0x06];

    fn branch_machine(register: u32) -> crate::Machine {
        let mut machine = crate::machine([0; 6]);
        set_receipt_config_registers(&mut machine);
        machine
            .bus
            .load_bytes(RESET_PC, &BEQZ_N_A6)
            .expect("test branch maps in mask ROM");
        machine.cores[0].set_ar(6, register);
        machine
    }

    fn set_receipt_config_registers(machine: &mut crate::Machine) {
        machine.bus.periph.system.ram.write(0x10, 6);
        machine.bus.periph.system.ram.write(0x60, 1 << 10);
        for spi in [&mut machine.bus.periph.spi0, &mut machine.bus.periph.spi1] {
            spi.regs.write(0x8, 1 << 24);
            spi.regs.write(0x14, 0x0001_0001);
        }
        machine.bus.periph.spi0.regs.write(0x40, 1 << 21);
        machine.bus.periph.spi0.regs.write(0x50, 0x0001_0001);
        machine.bus.periph.extmem.ram.write(0x0, 2 << 3);
        machine.bus.periph.extmem.ram.write(0x60, 1 << 3);
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
        assert_receipt_correlation::<Esp32Backend>();
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
            assert_eq!(machine.cores[0].ccount, expected as u32);
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
        machine.cores[0].ps = ps::WOE;
        machine.cores[0].windowbase = 0;
        machine.cores[0].windowstart = 1 << 1;
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
        let _ = plan_instruction(CoreId::Core0, &planned.cores[0], &planned.bus, &backend, 0)
            .expect("planning succeeds");
        assert_eq!(
            xtensa_lx7::step(&mut planned.cores[0], &mut planned.bus),
            xtensa_lx7::step(&mut control.cores[0], &mut control.bus)
        );
        assert_eq!(planned.cores[0].pc, control.cores[0].pc);
        assert_eq!(planned.cores[0].ps, control.cores[0].ps);
        assert_eq!(planned.cores[0].ccount, control.cores[0].ccount);
        assert_eq!(planned.cores[0].insn_count, control.cores[0].insn_count);
        assert_eq!(planned.bus.cycles, control.bus.cycles);
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
