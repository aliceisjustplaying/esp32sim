//! Receipt-backed measured adapter for the ESP32-S3 interpreter.

use backend_api::{
    price_operation, Backend, CacheAccessKind, CacheAccessResult, CacheFillPosition, CacheKind,
    CacheModel, CacheSource, ChipConfig, CoreId, CostComponent, ExecutionOutcome, FillPosition,
    FlashMode, InstructionCost, MmioTier, Operation, PsramMode, RefusalReason, TimingMutation,
    TimingRefusal, TransactionEngine,
};
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
    cache: Option<CacheModel>,
}

impl Default for Esp32Backend {
    fn default() -> Self {
        Self {
            engine: TransactionEngine::default(),
            config: ChipConfig::RECEIPT_SCOPE,
            previous_load: [None; 2],
            cache: CacheModel::new(ChipConfig::RECEIPT_SCOPE).ok(),
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

    fn update_config(&mut self, config: ChipConfig) {
        if self.config != config {
            self.config = config;
            self.cache = CacheModel::new(config).ok();
        }
    }

    fn cache_operations(&self, observation: &InstructionObservation) -> Vec<Operation> {
        let Some(mut cache) = self.cache.clone() else {
            return Vec::new();
        };
        let mut operations = Vec::new();
        match observation.fetch_memory {
            MemoryClass::Flash => operations.push(cache_operation(
                cache.access(CacheAccessKind::Fetch, observation.pc),
                true,
                observation.pc,
                self.config.icache_line_bytes,
            )),
            MemoryClass::Psram => operations.push(Operation::UnadoptedInstruction),
            _ => {}
        }
        if let (Some(memory), Some(access)) = (observation.access_memory, observation.access) {
            if matches!(memory, MemoryClass::Flash | MemoryClass::Psram) {
                let kind = match access.kind {
                    AccessKind::Load => CacheAccessKind::Load,
                    AccessKind::Store | AccessKind::Atomic => CacheAccessKind::Store,
                };
                operations.push(cache_operation(
                    cache.access(kind, access.address),
                    false,
                    access.address,
                    self.config.dcache_line_bytes,
                ));
            }
        }
        operations
    }

    fn commit_cache_accesses(&mut self, observation: &InstructionObservation) {
        let Some(cache) = &mut self.cache else {
            return;
        };
        if observation.fetch_memory == MemoryClass::Flash {
            let _result = cache.access(CacheAccessKind::Fetch, observation.pc);
        }
        if let (Some(memory), Some(access)) = (observation.access_memory, observation.access) {
            if matches!(memory, MemoryClass::Flash | MemoryClass::Psram) {
                let kind = match access.kind {
                    AccessKind::Load => CacheAccessKind::Load,
                    AccessKind::Store | AccessKind::Atomic => CacheAccessKind::Store,
                };
                let _result = cache.access(kind, access.address);
            }
        }
    }
}

fn cache_operation(
    result: CacheAccessResult,
    instruction: bool,
    address: u32,
    line_bytes: u8,
) -> Operation {
    match result {
        CacheAccessResult::Hit => Operation::HotCacheHit,
        CacheAccessResult::Miss { position, source } => Operation::CacheLineFill {
            cache: match (instruction, source) {
                (true, CacheSource::Flash) => CacheKind::InstructionFlash,
                (false, CacheSource::Flash) => CacheKind::DataFlash,
                (false, CacheSource::Psram) => CacheKind::DataPsram,
                (true, CacheSource::Psram) => return Operation::UnadoptedInstruction,
            },
            position: match position {
                FillPosition::First => CacheFillPosition::First,
                FillPosition::Subsequent => CacheFillPosition::Subsequent,
            },
            line: address / u32::from(line_bytes),
        },
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
        operations.extend(self.cache_operations(observation));
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
        self.commit_cache_accesses(observation);
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
        backend.update_config(chip_config_from_registers(self));
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
    let icache_control = extmem.read(0x60);
    let dcache_control = extmem.read(0x0);
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
        icache_size_bytes: if icache_control & (1 << 2) != 0 {
            32 * 1024
        } else {
            16 * 1024
        },
        icache_ways: if icache_control & (1 << 1) != 0 { 8 } else { 4 },
        icache_line_bytes: if icache_control & (1 << 3) != 0 {
            32
        } else {
            16
        },
        dcache_size_bytes: if dcache_control & (1 << 2) != 0 {
            64 * 1024
        } else {
            32 * 1024
        },
        dcache_ways: 8,
        dcache_line_bytes: match (dcache_control >> 3) & 3 {
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
    use backend_api::{CacheFillPosition, CacheKind, CostClass, ReceiptId};
    use xtensa_lx7::measured::BlockCostPayload;
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
        machine
            .bus
            .periph
            .extmem
            .ram
            .write(0x60, (1 << 3) | (1 << 1));
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

    fn flash_observation(core: CoreId) -> InstructionObservation {
        InstructionObservation {
            core,
            pc: 0x4200_0000,
            bytes: [BEQZ_N_A6[0], BEQZ_N_A6[1], 0, 0],
            instruction: xtensa_lx7::decode(0x4200_0000, [BEQZ_N_A6[0], BEQZ_N_A6[1], 0, 0]),
            fetch_memory: MemoryClass::Flash,
            access: None,
            access_memory: None,
            branch_taken: Some(true),
            load_destination: None,
            read_registers: 1 << 6,
            loop_back_edge_residue: None,
            block_cost: BlockCostPayload {
                start_pc: 0x4200_0000,
                static_cycles: 0,
                components: Vec::new(),
            },
        }
    }

    #[test]
    fn real_backend_passes_the_same_contract_as_fake() {
        assert_backend_contract::<Esp32Backend>();
        assert_receipt_correlation::<Esp32Backend>();
    }

    #[test]
    fn register_derived_configuration_includes_shared_cache_geometry() {
        let machine = branch_machine(0);
        assert_eq!(
            chip_config_from_registers(&machine),
            ChipConfig::RECEIPT_SCOPE
        );
    }

    #[test]
    fn cache_pricing_is_transactional_and_shared_between_live_cores() {
        let mut backend = Esp32Backend::default();
        let core0 = flash_observation(CoreId::Core0);
        let first = backend.price(&core0).expect("first fetch prices");
        let repeated_plan = backend.price(&core0).expect("planning stays immutable");
        assert!(first.components.iter().any(|component| {
            component.class
                == CostClass::CacheLineFill {
                    cache: CacheKind::InstructionFlash,
                    position: CacheFillPosition::First,
                }
        }));
        assert_eq!(first, repeated_plan);
        backend
            .commit(&core0, &first.components, &first.mutations)
            .expect("core 0 fetch commits");

        let core1 = flash_observation(CoreId::Core1);
        let shared_hit = backend.price(&core1).expect("core 1 fetch prices");
        assert!(shared_hit
            .components
            .iter()
            .any(|component| component.class == CostClass::HotCacheHit));
        backend
            .commit(&core1, &shared_hit.components, &shared_hit.mutations)
            .expect("core 1 fetch commits");
        assert_eq!(backend.engine().ledger()[0].core, CoreId::Core0);
        assert_eq!(backend.engine().ledger()[1].core, CoreId::Core1);
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
