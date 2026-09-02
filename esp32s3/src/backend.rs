//! Receipt-backed measured adapter for the ESP32-S3 interpreter.

use backend_api::{
    price_operation, Backend, CacheFillPosition, CacheKind, ChipConfig, CoreId, CostComponent,
    ExecutionOutcome, FlashMode, InstructionCost, MmioTier, Operation, PsramMode, RefusalReason,
    TimingMutation, TimingRefusal, TransactionEngine,
};
use std::collections::BTreeSet;
use xtensa_lx7::measured::{complete_instruction, plan_instruction, CompletionError, PlanError};
use xtensa_lx7::measured::{
    AccessKind, InstructionObservation, MemoryClass, TimingPlan, TimingSource,
};
use xtensa_lx7::state::INTTYPE_LEVEL;
use xtensa_lx7::{Op, Trap};

/// Product adapter. Fake and product adapters both delegate scheduling state,
/// transactional commit, and canonical ledger generation to `TransactionEngine`.
#[derive(Clone, Debug)]
pub struct Esp32Backend {
    engine: TransactionEngine,
    config: ChipConfig,
    previous_load: [Option<u8>; 2],
    seen_cache_lines: BTreeSet<(CoreId, CacheKind, u32)>,
}

impl Default for Esp32Backend {
    fn default() -> Self {
        Self {
            engine: TransactionEngine::default(),
            config: ChipConfig::RECEIPT_SCOPE,
            previous_load: [None; 2],
            seen_cache_lines: BTreeSet::new(),
        }
    }
}

impl Esp32Backend {
    fn instruction_operation(&self, observation: &InstructionObservation) -> Operation {
        use Op::*;
        let kind = match observation.instruction.op {
            Beqz | Bnez | Bltz | Bgez | BeqzN | BnezN | Beqi | Bnei | Blti | Bgei | Bltui
            | Bgeui | Bnone | Beq | Blt | Bltu | Ball | Bbc | Bbci | Bany | Bne | Bge | Bgeu
            | Bnall | Bbs | Bbsi | Bf | Bt => InstructionCost::Branch {
                taken: observation
                    .branch_taken
                    .expect("conditional branch has an outcome"),
            },
            J => InstructionCost::Jump,
            Jx => InstructionCost::JumpRegister,
            Loop | Loopnez | Loopgtz => InstructionCost::LoopSetup,
            Quos | Quou => InstructionCost::Quotient,
            Rems | Remu => InstructionCost::Remainder,
            S32c1i => InstructionCost::AtomicStore,
            L32r => InstructionCost::LiteralLoad,
            Isync => InstructionCost::InstructionSync,
            Call0 | Call4 | Call8 | Call12 | Callx0 | Callx4 | Callx8 | Callx12 | Ret | RetN
            | Retw | RetwN | Ill | IllN | Break | BreakN | Syscall | Simcall => {
                return Operation::UnadoptedInstruction;
            }
            _ => InstructionCost::Issue,
        };
        Operation::Instruction(kind)
    }

    fn cache_fill(&self, observation: &InstructionObservation) -> Option<Operation> {
        let (cache, address, line_bytes) = match observation.fetch_memory {
            MemoryClass::Flash => (
                CacheKind::InstructionFlash,
                observation.pc,
                self.config.icache_line_bytes,
            ),
            _ => match (observation.access_memory, observation.access) {
                (Some(MemoryClass::Flash), Some(access)) => (
                    CacheKind::DataFlash,
                    access.address,
                    self.config.dcache_line_bytes,
                ),
                (Some(MemoryClass::Psram), Some(access)) => (
                    CacheKind::DataPsram,
                    access.address,
                    self.config.dcache_line_bytes,
                ),
                _ => return None,
            },
        };
        let line = address / u32::from(line_bytes);
        let key = (observation.core, cache, line);
        if self.seen_cache_lines.contains(&key) {
            return Some(Operation::HotCacheHit);
        }
        let position = if self
            .seen_cache_lines
            .iter()
            .any(|(core, kind, _)| *core == observation.core && *kind == cache)
        {
            CacheFillPosition::Subsequent
        } else {
            CacheFillPosition::First
        };
        Some(Operation::CacheLineFill {
            cache,
            position,
            line,
        })
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
        let access = observation.access;
        let primary = if observation.access_memory == Some(MemoryClass::Mmio) {
            let access = access.expect("classified access has a shape");
            let tier = mmio_tier(access.address).ok_or(TimingRefusal {
                class: backend_api::CostClass::UnknownMmio,
                tier_candidate: backend_api::CostTier::Unexplained,
                reason: RefusalReason::UnknownMmioRegister,
                configuration: None,
            })?;
            match access.kind {
                AccessKind::Load => Operation::MmioRead { tier },
                AccessKind::Store | AccessKind::Atomic => Operation::MmioWrite {
                    tier,
                    buffer_has_room: self.engine.state().posted_mmio_writes < 8,
                },
            }
        } else {
            self.instruction_operation(observation)
        };
        let mut operations = vec![primary];
        if access.is_some_and(|_| observation.access_memory == Some(MemoryClass::InternalSram)) {
            operations.push(Operation::IndependentSramAccess);
        }
        if let Some(cache) = self.cache_fill(observation) {
            operations.push(cache);
        }
        if self.previous_load[core_index(observation.core)]
            .is_some_and(|register| observation.read_registers & (1 << register) != 0)
        {
            operations.push(Operation::Instruction(InstructionCost::LoadUse));
        }
        if let Some(body_residue) = observation.loop_back_edge_residue {
            operations.push(Operation::LoopBackEdge { body_residue });
        }
        let mut components = Vec::new();
        let mut mutations = Vec::new();
        for operation in operations {
            let (component, mutation) = price_operation(self.config, observation.core, operation)?;
            components.push(component);
            mutations.extend(mutation);
        }
        let cycles = components
            .iter()
            .try_fold(0u64, |sum, component| sum.checked_add(component.cycles()?))
            .ok_or(TimingRefusal {
                class: components[0].class,
                tier_candidate: backend_api::CostTier::Unexplained,
                reason: RefusalReason::CycleOverflow,
                configuration: None,
            })?;
        Ok(TimingPlan {
            cycles,
            components,
            mutations,
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
        self.previous_load[core_index(observation.core)] = observation.load_destination;
        for mutation in mutations {
            if let TimingMutation::RecordCacheFill { core, cache, line } = *mutation {
                self.seen_cache_lines.insert((core, cache, line));
            }
        }
        Ok(())
    }
}

fn mmio_tier(address: u32) -> Option<MmioTier> {
    match address {
        0x600c_0000..=0x600c_ffff => Some(MmioTier::Fast),
        0x6000_8000..=0x6000_8fff => Some(MmioTier::Rtc),
        0x6000_7000..=0x6000_7fff => Some(MmioTier::Efuse),
        0x6001_cc00..=0x6001_cfff => Some(MmioTier::Nrx),
        0x6000_0000..=0x600b_ffff => Some(MmioTier::Apb),
        _ => None,
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
    Deadline(backend_api::DeadlineError),
}

impl crate::Machine {
    /// Execute one receipt-priced transaction on one of the two native cores.
    pub fn step_measured(
        &mut self,
        backend: &mut Esp32Backend,
        core: CoreId,
    ) -> Result<MeasuredStep, MeasuredStepError> {
        backend.config = chip_config_from_registers(self);
        self.advance_measured_devices(self.bus.cycles)?;
        let before_cycle = backend.engine().state().cores[core_index(core)].cycle;
        let interrupt = {
            let cpu = match core {
                CoreId::Core0 => &mut self.cpu,
                CoreId::Core1 => &mut self.cpu1,
            };
            cpu.check_interrupts()
        };
        if let Some(Trap::Interrupt(irq)) = interrupt {
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
        if cycle >= self.bus.cycles {
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
        flash_mode: if spi0.read(0x8) & spi1.read(0x8) & (1 << 24) != 0 {
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
        let mut machine = crate::Machine::new([0; 6]);
        set_receipt_config_registers(&mut machine);
        machine
            .bus
            .load_bytes(RESET_PC, &BEQZ_N_A6)
            .expect("test branch maps in mask ROM");
        machine.cpu.set_ar(6, register);
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
            assert_eq!(entry.components[0].receipt, ReceiptId::OpcodeLadders);
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
