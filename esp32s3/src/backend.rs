//! Receipt-backed measured adapter for the ESP32-S3 interpreter.

use backend_api::{
    price_operation, Backend, CacheAccessKind, CacheAccessResult, CacheFillPosition, CacheKind,
    CacheModel, CacheSource, ChipConfig, CoreId, CostClass, CostComponent, ExecutionOutcome, FillPosition,
    FlashMode, InstructionCost, MmioTier, Operation, PsramMode, RefusalReason, TimingMutation,
    TimingRefusal, TransactionCheckpoint, TransactionEngine,
};
use emu_core::{
    ControlEventKind, CostModel, ExecutionFacts, LifecycleFacts, LifecycleKind, MemoryAccess,
    MemoryAccessKind, ModeledBlockEvent, StepKind,
};
use std::cell::RefCell;
use std::rc::Rc;
use xtensa_lx7::measured::{
    complete_instruction, observe_instruction, plan_instruction, CompletionError, PlanError,
};
use xtensa_lx7::measured::{
    AccessKind, AccessShape, BlockCostPayload, InstructionObservation, MemoryClass, TimingPlan,
    TimingSource,
};
use xtensa_lx7::state::{exc, INTTYPE_LEVEL};
use xtensa_lx7::{Op, Trap};

/// Product adapter. Fake and product adapters both delegate scheduling state,
/// transactional commit, and canonical ledger generation to `TransactionEngine`.
#[derive(Clone, Debug)]
pub struct Esp32Backend {
    engine: TransactionEngine,
    config: ChipConfig,
    previous_load: [Option<u8>; 2],
    cache: Option<CacheModel>,
    config_registers: ConfigRegisters,
    mmu: [u32; crate::bus::MMU_ENTRIES],
    hook_cache_accessed: bool,
    modeled_template: Option<ModeledTemplate>,
}

#[derive(Clone, Debug)]
struct ModeledTemplate {
    config: ChipConfig,
    events: Vec<ModeledBlockEvent>,
    priced: Vec<(CoreId, u32, CostComponent)>,
}

impl Default for Esp32Backend {
    fn default() -> Self {
        Self {
            engine: TransactionEngine::default(),
            config: ChipConfig::RECEIPT_SCOPE,
            previous_load: [None; 2],
            cache: CacheModel::new(ChipConfig::RECEIPT_SCOPE).ok(),
            config_registers: ConfigRegisters::default(),
            mmu: [crate::bus::MMU_INVALID; crate::bus::MMU_ENTRIES],
            hook_cache_accessed: false,
            modeled_template: None,
        }
    }
}

#[derive(Clone, Debug, Default)]
struct ConfigRegisters {
    system_cpu_per_conf: u32,
    system_sysclk_conf: u32,
    spi0_user: u32,
    spi0_clock: u32,
    spi0_sram_cmd: u32,
    spi0_sram_clk: u32,
    spi1_user: u32,
    spi1_clock: u32,
    extmem_dcache_ctrl: u32,
    extmem_icache_ctrl: u32,
}

#[derive(Clone, Debug)]
struct Esp32BatchCheckpoint {
    engine: TransactionCheckpoint,
    previous_load: [Option<u8>; 2],
}

/// Shared handle for a machine-attached timing model and its deterministic report state.
#[derive(Clone, Debug)]
pub struct Esp32CostModel {
    inner: Rc<RefCell<Esp32Backend>>,
}

impl Default for Esp32CostModel {
    fn default() -> Self {
        Self {
            inner: Rc::new(RefCell::new(Esp32Backend::default())),
        }
    }
}

impl Esp32CostModel {
    pub fn core_cycles(&self) -> Result<[u64; 2], String> {
        let model = self
            .inner
            .try_borrow()
            .map_err(|_| "CostModelState: model is already borrowed".to_string())?;
        Ok([
            model.engine.state().cores[0].cycle,
            model.engine.state().cores[1].cycle,
        ])
    }

    pub fn configuration(&self) -> Result<ChipConfig, String> {
        self.inner
            .try_borrow()
            .map(|model| model.config)
            .map_err(|_| "CostModelState: model is already borrowed".to_string())
    }

    /// Canonical ledger bytes committed by the attached product model.
    pub fn canonical_ledger(&self) -> Result<Vec<u8>, String> {
        let model = self
            .inner
            .try_borrow()
            .map_err(|_| "CostModelState: model is already borrowed".to_string())?;
        let mut engine = model.engine.clone();
        engine
            .run_trace(&[])
            .map(|report| report.canonical_ledger)
            .map_err(format_timing_refusal)
    }
}

impl CostModel for Esp32CostModel {
    fn lifecycle(&mut self, facts: &LifecycleFacts) -> Result<(), String> {
        let mut model = self
            .inner
            .try_borrow_mut()
            .map_err(|_| "CostModelReentry: lifecycle callback reentered the model".to_string())?;
        CostModel::lifecycle(&mut *model, facts)
    }

    fn cycles(&mut self, facts: &ExecutionFacts<'_>) -> Result<u32, String> {
        let mut model = self
            .inner
            .try_borrow_mut()
            .map_err(|_| "CostModelReentry: cycle callback reentered the model".to_string())?;
        CostModel::cycles(&mut *model, facts)
    }

    fn commit_modeled_block(&mut self, events: &[ModeledBlockEvent]) -> Option<Result<(), String>> {
        let mut model = self.inner.try_borrow_mut().ok()?;
        CostModel::commit_modeled_block(&mut *model, events)
    }

    fn begin_batch(&mut self) -> Option<Box<dyn std::any::Any>> {
        self.inner
            .try_borrow()
            .ok()
            .map(|model| Box::new(model.batch_checkpoint()) as Box<dyn std::any::Any>)
    }

    fn rollback_batch(
        &mut self,
        checkpoint: Box<dyn std::any::Any>,
    ) -> Result<(), String> {
        let checkpoint = checkpoint
            .downcast::<Esp32BatchCheckpoint>()
            .map_err(|_| "CostModelBatch: incompatible checkpoint".to_string())?;
        let mut model = self
            .inner
            .try_borrow_mut()
            .map_err(|_| "CostModelReentry: batch rollback reentered the model".to_string())?;
        model.rollback_batch(*checkpoint);
        Ok(())
    }
}

impl Esp32Backend {
    fn batch_checkpoint(&self) -> Esp32BatchCheckpoint {
        Esp32BatchCheckpoint {
            engine: self.engine.checkpoint(),
            previous_load: self.previous_load,
        }
    }

    fn rollback_batch(&mut self, checkpoint: Esp32BatchCheckpoint) {
        self.engine.rollback(checkpoint.engine);
        self.previous_load = checkpoint.previous_load;
    }

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
            Rfe | Rfue | Rfde | Rfwo | Rfwu | Rfi | Simcall => {
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

    fn cache_operations(&self, observation: &InstructionObservation) -> CacheOperations {
        let Some(mut cache) = self.cache.clone() else {
            return CacheOperations::default();
        };
        let mut projected = CacheOperations::default();
        match observation.fetch_memory {
            MemoryClass::Flash => projected.push(cache_operation(
                cache.access(CacheAccessKind::Fetch, observation.pc),
                true,
                observation.pc,
                self.config.icache_line_bytes,
            )),
            MemoryClass::Psram => {
                let _result = cache.access(CacheAccessKind::Fetch, observation.pc);
                projected.operations.push(Operation::UnadoptedInstruction);
            }
            _ => {}
        }
        if let (Some(memory), Some(access)) = (observation.access_memory, observation.access) {
            if matches!(memory, MemoryClass::Flash | MemoryClass::Psram) {
                let kind = match access.kind {
                    AccessKind::Load => CacheAccessKind::Load,
                    AccessKind::Store | AccessKind::Atomic => CacheAccessKind::Store,
                };
                projected.push(cache_operation(
                    cache.access(kind, access.address),
                    false,
                    access.address,
                    self.config.dcache_line_bytes,
                ));
            }
        }
        projected
    }

    fn commit_cache_accesses(&mut self, observation: &InstructionObservation) {
        let Some(cache) = &mut self.cache else {
            return;
        };
        if observation.fetch_memory == MemoryClass::Flash {
            self.hook_cache_accessed = true;
            let _result = cache.access(CacheAccessKind::Fetch, observation.pc);
        }
        if let (Some(memory), Some(access)) = (observation.access_memory, observation.access) {
            if matches!(memory, MemoryClass::Flash | MemoryClass::Psram) {
                self.hook_cache_accessed = true;
                let kind = match access.kind {
                    AccessKind::Load => CacheAccessKind::Load,
                    AccessKind::Store | AccessKind::Atomic => CacheAccessKind::Store,
                };
                let _result = cache.access(kind, access.address);
            }
        }
    }

    fn reset_hook_state(&mut self) {
        self.engine = TransactionEngine::default();
        self.previous_load = [None; 2];
        self.config_registers = ConfigRegisters::default();
        self.mmu.fill(crate::bus::MMU_INVALID);
        self.hook_cache_accessed = false;
        self.modeled_template = None;
        self.update_config(self.config_registers.chip_config());
    }

    fn memory_class(&self, address: u32) -> MemoryClass {
        use crate::bus::{
            DBUS_HIGH, DBUS_LOW, DRAM_LOW, IBUS_HIGH, IBUS_LOW, IRAM_LOW, MMU_INVALID,
            MMU_SPIRAM,
        };
        match address {
            DRAM_LOW..=0x3fcf_ffff | IRAM_LOW..=0x403d_ffff => MemoryClass::InternalSram,
            crate::bus::IROM_MASK_LOW..=0x4005_ffff | crate::bus::DROM_MASK_LOW..=0x3ff1_ffff => {
                MemoryClass::MaskRom
            }
            crate::bus::RTC_FAST_LOW..=0x600f_ffff
            | crate::bus::RTC_SLOW_LOW..=0x5000_1fff => MemoryClass::Rtc,
            0x6000_0000..=0x600d_ffff => MemoryClass::Mmio,
            _ if (DBUS_LOW..DBUS_HIGH).contains(&address)
                || (IBUS_LOW..IBUS_HIGH).contains(&address) =>
            {
                let entry = self.mmu[((address & 0x1ff_ffff) >> 16) as usize];
                if entry & MMU_INVALID != 0 {
                    MemoryClass::Unknown
                } else if entry & MMU_SPIRAM != 0 {
                    MemoryClass::Psram
                } else {
                    MemoryClass::Flash
                }
            }
            _ => MemoryClass::Unknown,
        }
    }

    fn observation_from_facts(
        &self,
        facts: &ExecutionFacts<'_>,
    ) -> Result<InstructionObservation, String> {
        let bytes = facts
            .outcome
            .bytes
            .ok_or_else(|| "InstructionBytes: unavailable (Unexplained tier)".to_string())?;
        let instruction = xtensa_lx7::decode(facts.outcome.pc, bytes);
        let accesses: Vec<_> = facts
            .accesses
            .iter()
            .filter(|access| access.kind != MemoryAccessKind::Fetch)
            .collect();
        let first_access = accesses.first().copied();
        if let Some(first) = first_access {
            let same_transaction = accesses
                .iter()
                .all(|access| access.address == first.address);
            if !same_transaction {
                return Err("MultipleMemoryAccesses: cost not adopted (Unexplained tier)".into());
            }
        }
        let access = first_access.map(|access| AccessShape {
            kind: if instruction.op == Op::S32c1i {
                AccessKind::Atomic
            } else {
                match access.kind {
                    MemoryAccessKind::Read => AccessKind::Load,
                    MemoryAccessKind::Write => AccessKind::Store,
                    MemoryAccessKind::Fetch => AccessKind::Load,
                }
            },
            address: access.address,
            width: access.width,
        });
        let sequential_pc = facts
            .outcome
            .pc
            .wrapping_add(u32::from(facts.outcome.length));
        let conditional = is_conditional_branch(instruction.op);
        let loop_back_edge_residue = (!conditional
            && !is_explicit_control_flow(instruction.op)
            && facts.outcome.next_pc < sequential_pc)
            .then_some((facts.outcome.next_pc & 3) as u8);
        Ok(InstructionObservation {
            core: hook_core(facts.core)?,
            pc: facts.outcome.pc,
            bytes,
            instruction,
            fetch_memory: self.memory_class(facts.outcome.pc),
            access_memory: access.map(|shape| self.memory_class(shape.address)),
            access,
            branch_taken: conditional.then_some(facts.outcome.next_pc != sequential_pc),
            load_destination: hook_load_destination(instruction),
            read_registers: hook_read_registers(instruction),
            loop_back_edge_residue,
            block_cost: BlockCostPayload {
                start_pc: facts.outcome.pc,
                static_cycles: 0,
                components: Vec::new(),
            },
        })
    }

    fn validate_hook_writes(&self, accesses: &[MemoryAccess]) -> Result<(), String> {
        for access in accesses.iter().filter(|access| {
            access.kind == MemoryAccessKind::Write && access.fault.is_none()
        }) {
            let mmu_write = (crate::bus::MMU_TABLE
                ..crate::bus::MMU_TABLE + crate::bus::MMU_ENTRIES as u32 * 4)
                .contains(&access.address);
            if mmu_write && (access.width != 4 || access.address & 3 != 0) {
                return Err("PartialMmuTableWrite: cost not adopted (Unexplained tier)".into());
            }
            if mmu_write && self.hook_cache_accessed {
                return Err("MmuRemapWithLiveCache: cost not adopted (Unexplained tier)".into());
            }
            if let Some(base) = ConfigRegisters::word_base(access.address) {
                if access.width != 4 || access.address != base {
                    return Err("PartialChipConfigWrite: cost not adopted (Unexplained tier)".into());
                }
                if self.hook_cache_accessed {
                    return Err("CacheReconfigurationAfterAccess: cost not adopted (Unexplained tier)".into());
                }
            }
        }
        Ok(())
    }

    fn apply_hook_writes(&mut self, accesses: &[MemoryAccess]) {
        let mut config_changed = false;
        for access in accesses.iter().filter(|access| {
            access.kind == MemoryAccessKind::Write && access.fault.is_none()
        }) {
            if (crate::bus::MMU_TABLE
                ..crate::bus::MMU_TABLE + crate::bus::MMU_ENTRIES as u32 * 4)
                .contains(&access.address)
            {
                self.mmu[((access.address - crate::bus::MMU_TABLE) >> 2) as usize] =
                    access.value;
            }
            config_changed |= self.config_registers.apply_write(*access);
        }
        if config_changed {
            self.update_config(self.config_registers.chip_config());
        }
    }
}

impl ConfigRegisters {
    fn chip_config(&self) -> ChipConfig {
        let cpu_mhz = match (
            (self.system_sysclk_conf >> 10) & 3,
            self.system_cpu_per_conf & 3,
        ) {
            (0, _) => 40,
            (1, 0) => 80,
            (1, 1) => 160,
            (1, 2) => 240,
            _ => 40,
        };
        let clock_mhz = |register: u32| {
            if register & (1 << 31) != 0 {
                160
            } else {
                160 / (((register >> 16) & 0xff) as u16 + 1)
            }
        };
        let flash0 = clock_mhz(self.spi0_clock);
        let flash1 = clock_mhz(self.spi1_clock);
        ChipConfig {
            cpu_mhz,
            apb_mhz: cpu_mhz.min(80),
            flash_mode: if self.spi0_user & self.spi1_user & (1 << 24) != 0 {
                FlashMode::Qio
            } else {
                FlashMode::Other
            },
            flash_mhz: if flash0 == flash1 { flash0 } else { 0 },
            psram_mode: if self.spi0_sram_cmd & (1 << 21) != 0 {
                PsramMode::OctalDtr
            } else {
                PsramMode::Other
            },
            psram_mhz: clock_mhz(self.spi0_sram_clk),
            icache_size_bytes: if self.extmem_icache_ctrl & (1 << 2) != 0 {
                32 * 1024
            } else {
                16 * 1024
            },
            icache_ways: if self.extmem_icache_ctrl & (1 << 1) != 0 {
                8
            } else {
                4
            },
            icache_line_bytes: if self.extmem_icache_ctrl & (1 << 3) != 0 {
                32
            } else {
                16
            },
            dcache_size_bytes: if self.extmem_dcache_ctrl & (1 << 2) != 0 {
                64 * 1024
            } else {
                32 * 1024
            },
            dcache_ways: 8,
            dcache_line_bytes: match (self.extmem_dcache_ctrl >> 3) & 3 {
                0 => 16,
                1 => 32,
                2 => 64,
                _ => 0,
            },
        }
    }

    fn word_base(address: u32) -> Option<u32> {
        let bases: [u32; 10] = [
            0x600c_0010,
            0x600c_0060,
            0x6000_3008,
            0x6000_3014,
            0x6000_3040,
            0x6000_3050,
            0x6000_2008,
            0x6000_2014,
            0x600c_4000,
            0x600c_4060,
        ];
        bases
        .into_iter()
        .find(|base| (*base..(*base).saturating_add(4)).contains(&address))
    }

    fn apply_write(&mut self, access: MemoryAccess) -> bool {
        let target = match access.address {
            0x600c_0010 => Some(&mut self.system_cpu_per_conf),
            0x600c_0060 => Some(&mut self.system_sysclk_conf),
            0x6000_3008 => Some(&mut self.spi0_user),
            0x6000_3014 => Some(&mut self.spi0_clock),
            0x6000_3040 => Some(&mut self.spi0_sram_cmd),
            0x6000_3050 => Some(&mut self.spi0_sram_clk),
            0x6000_2008 => Some(&mut self.spi1_user),
            0x6000_2014 => Some(&mut self.spi1_clock),
            0x600c_4000 => Some(&mut self.extmem_dcache_ctrl),
            0x600c_4060 => Some(&mut self.extmem_icache_ctrl),
            _ => None,
        };
        let Some(target) = target else { return false };
        *target = access.value;
        true
    }
}

#[derive(Default)]
struct CacheOperations {
    operations: Vec<Operation>,
    dirty_eviction: Option<CacheSource>,
}

impl CacheOperations {
    fn push(&mut self, access: CacheOperation) {
        self.operations.push(access.operation);
        self.dirty_eviction = self.dirty_eviction.or(access.dirty_eviction);
    }
}

struct CacheOperation {
    operation: Operation,
    dirty_eviction: Option<CacheSource>,
}

fn cache_operation(
    result: CacheAccessResult,
    instruction: bool,
    address: u32,
    line_bytes: u8,
) -> CacheOperation {
    match result {
        CacheAccessResult::Hit => CacheOperation {
            operation: Operation::HotCacheHit,
            dirty_eviction: None,
        },
        CacheAccessResult::Miss {
            position,
            source,
            eviction,
        } => CacheOperation {
            operation: match (instruction, source) {
                (true, CacheSource::Flash) => Operation::CacheLineFill {
                    cache: CacheKind::InstructionFlash,
                    position: cache_fill_position(position),
                    line: address / u32::from(line_bytes),
                },
                (false, CacheSource::Flash) => Operation::CacheLineFill {
                    cache: CacheKind::DataFlash,
                    position: cache_fill_position(position),
                    line: address / u32::from(line_bytes),
                },
                (false, CacheSource::Psram) => Operation::CacheLineFill {
                    cache: CacheKind::DataPsram,
                    position: cache_fill_position(position),
                    line: address / u32::from(line_bytes),
                },
                (true, CacheSource::Psram) => Operation::UnadoptedInstruction,
            },
            dirty_eviction: eviction
                .filter(|evicted| evicted.dirty)
                .map(|evicted| evicted.source),
        },
    }
}

const fn cache_fill_position(position: FillPosition) -> CacheFillPosition {
    match position {
        FillPosition::First => CacheFillPosition::First,
        FillPosition::Subsequent => CacheFillPosition::Subsequent,
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
        let cache = self.cache_operations(observation);
        if cache.dirty_eviction.is_some() {
            return Err(TimingRefusal {
                class: backend_api::CostClass::UnadoptedInstruction,
                tier_candidate: backend_api::CostTier::Unexplained,
                reason: RefusalReason::CostNotAdopted,
                configuration: Some(self.config),
            });
        }
        operations.extend(cache.operations);
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

impl CostModel for Esp32Backend {
    fn lifecycle(&mut self, facts: &LifecycleFacts) -> Result<(), String> {
        if facts.chip != "esp32s3" || facts.cores != 2 || facts.cpu_hz != 240_000_000 {
            return Err(format!(
                "UnsupportedChipConfig: {} with {} cores at {} Hz (Unexplained tier)",
                facts.chip, facts.cores, facts.cpu_hz
            ));
        }
        match facts.kind {
            LifecycleKind::Attach | LifecycleKind::ChipReset => self.reset_hook_state(),
            LifecycleKind::CoreReset(core) => {
                let index = core_index(hook_core(core)?);
                self.previous_load[index] = None;
            }
        }
        Ok(())
    }

    fn cycles(&mut self, facts: &ExecutionFacts<'_>) -> Result<u32, String> {
        match facts.outcome.kind {
            StepKind::Idle => return Ok(1),
            StepKind::TrapBefore(trap) | StepKind::TrapDuring(trap) => {
                return Err(trap_refusal(trap));
            }
            StepKind::Retired => {}
        }
        if let Some(control) = facts.outcome.control {
            let class = match control.kind {
                ControlEventKind::Cache(operation) => format!("CacheControl::{operation:?}"),
                ControlEventKind::Tlb(operation) => format!("TlbControl::{operation:?}"),
            };
            return Err(format!(
                "{class}: cost not adopted at {:#010x} (Unexplained tier)",
                control.address
            ));
        }
        self.validate_hook_writes(facts.accesses)?;
        let observation = self.observation_from_facts(facts)?;
        if observation.fetch_memory == MemoryClass::MaskRom {
            return Err("MaskRomInstructionFetch: cost not adopted (Unexplained tier)".into());
        }
        if let Some(class) = exception_return_class(observation.instruction.op) {
            return Err(format!(
                "ExceptionReturn::{class:?}: cost not adopted (Unexplained tier)"
            ));
        }
        let plan = self.price(&observation).map_err(format_timing_refusal)?;
        let cycles = u32::try_from(plan.cycles)
            .map_err(|_| "CycleOverflow: event exceeds u32 cycles (Unexplained tier)".to_string())?;
        self.commit(&observation, &plan.components, &plan.mutations)
            .map_err(format_timing_refusal)?;
        self.apply_hook_writes(facts.accesses);
        Ok(cycles)
    }

    fn commit_modeled_block(&mut self, events: &[ModeledBlockEvent]) -> Option<Result<(), String>> {
        if let Some(template) = &self.modeled_template {
            if template.config == self.config && template.events == events {
                let result = self.engine.execute_static_sram_batch(&template.priced).map_err(format_timing_refusal);
                if result.is_ok() {
                    for event in events { self.previous_load[event.core] = None; }
                }
                return Some(result);
            }
        }
        let mut priced = Vec::with_capacity(events.len());
        for event in events {
            if event.outcome.kind != StepKind::Retired
                || event.outcome.control.is_some()
                || self.memory_class(event.outcome.pc) != MemoryClass::InternalSram
            {
                return None;
            }
            let fetch = [MemoryAccess {
                kind: MemoryAccessKind::Fetch,
                address: event.outcome.pc,
                width: 4,
                value: u32::from_le_bytes(event.outcome.bytes?),
                fault: None,
            }];
            let facts = ExecutionFacts { core: event.core, outcome: event.outcome, accesses: &fetch };
            let observation = match self.observation_from_facts(&facts) {
                Ok(observation) => observation,
                Err(_) => return None,
            };
            let plan = match self.price(&observation) {
                Ok(plan) => plan,
                Err(_) => return None,
            };
            let [component] = plan.components.as_slice() else { return None; };
            if !matches!(component.class, CostClass::Instruction(_))
                || !plan.mutations.is_empty()
                || component.cycles() != Some(u64::from(event.applied_cycles))
            {
                return None;
            }
            let core = match hook_core(event.core) {
                Ok(core) => core,
                Err(error) => return Some(Err(error)),
            };
            priced.push((core, event.outcome.pc, *component));
        }
        self.modeled_template = Some(ModeledTemplate {
            config: self.config,
            events: events.to_vec(),
            priced,
        });
        let result = self.engine.execute_static_sram_batch(&self.modeled_template.as_ref().expect("template was stored").priced).map_err(format_timing_refusal);
        if result.is_ok() {
            for event in events {
                self.previous_load[event.core] = None;
            }
        }
        Some(result)
    }

    fn begin_batch(&mut self) -> Option<Box<dyn std::any::Any>> {
        Some(Box::new(self.batch_checkpoint()))
    }

    fn rollback_batch(
        &mut self,
        checkpoint: Box<dyn std::any::Any>,
    ) -> Result<(), String> {
        let checkpoint = checkpoint
            .downcast::<Esp32BatchCheckpoint>()
            .map_err(|_| "CostModelBatch: incompatible checkpoint".to_string())?;
        self.rollback_batch(*checkpoint);
        Ok(())
    }
}

fn hook_core(core: usize) -> Result<CoreId, String> {
    match core {
        0 => Ok(CoreId::Core0),
        1 => Ok(CoreId::Core1),
        _ => Err(format!(
            "UnsupportedCore({core}): ESP32-S3 has two cores (Unexplained tier)"
        )),
    }
}

fn trap_refusal(trap: emu_core::Trap) -> String {
    match trap {
        Trap::Exception(cause) => format!(
            "ExceptionEntry::{:?}: cost not adopted (Unexplained tier)",
            exception_entry_class(cause)
        ),
        Trap::Interrupt(irq) => {
            format!("InterruptEntry({irq}): cost not adopted (Unexplained tier)")
        }
        Trap::Unimplemented(_, raw) => format!(
            "UnimplementedInstruction({raw:#010x}): cost not adopted (Unexplained tier)"
        ),
        Trap::Simcall => "Simcall: terminal event is not priced (Unexplained tier)".into(),
        Trap::Ebreak(_) => "EbreakEntry: cost not adopted (Unexplained tier)".into(),
    }
}

fn format_timing_refusal(refusal: TimingRefusal) -> String {
    format!(
        "{:?}: {:?} ({:?} tier, configuration {:?})",
        refusal.class, refusal.reason, refusal.tier_candidate, refusal.configuration
    )
}

const fn is_conditional_branch(op: Op) -> bool {
    use Op::*;
    matches!(
        op,
        Beqz | Bnez
            | Bltz
            | Bgez
            | BeqzN
            | BnezN
            | Beqi
            | Bnei
            | Blti
            | Bgei
            | Bltui
            | Bgeui
            | Bnone
            | Beq
            | Blt
            | Bltu
            | Ball
            | Bbc
            | Bbci
            | Bany
            | Bne
            | Bge
            | Bgeu
            | Bnall
            | Bbs
            | Bbsi
            | Bf
            | Bt
    )
}

const fn is_explicit_control_flow(op: Op) -> bool {
    use Op::*;
    is_conditional_branch(op)
        || matches!(
            op,
            J | Jx
                | Call0
                | Call4
                | Call8
                | Call12
                | Callx0
                | Callx4
                | Callx8
                | Callx12
                | Ret
                | RetN
                | Retw
                | RetwN
                | Rfe
                | Rfue
                | Rfde
                | Rfwo
                | Rfwu
                | Rfi
    )
}

fn hook_load_destination(instruction: xtensa_lx7::Insn) -> Option<u8> {
    use Op::*;
    if matches!(
        instruction.op,
        L8ui | L16ui | L16si | L32i | L32iN | L32r | L32ai | L32e | Lsi | Lsip | Lsx | Lsxp
    ) {
        Some(instruction.t)
    } else {
        None
    }
}

fn hook_read_registers(instruction: xtensa_lx7::Insn) -> u16 {
    use Op::*;
    let bit = |register: u8| 1u16 << register;
    match instruction.op {
        L32r | Movi | MoviN | J | Call0 | Call4 | Call8 | Call12 => 0,
        L8ui | L16ui | L16si | L32i | L32iN | L32ai | L32e | Lsi | Lsip => {
            bit(instruction.s)
        }
        S8i | S16i | S32i | S32iN | S32ri | S32e | S32nb | Ssi | Ssip | S32c1i => {
            bit(instruction.s) | bit(instruction.t)
        }
        _ => bit(instruction.s) | bit(instruction.t),
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
    Deadline(crate::board::BoardDeadlineError),
    Unpriced(UnpricedTimingClass),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UnpricedTimingClass {
    MaskRomInstructionFetch,
    ExceptionEntry(ExceptionEntryClass),
    ExceptionReturn(ExceptionReturnClass),
    InterruptEntry { irq: u32 },
    DirtyCacheEvictionWriteback(CacheSource),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MeasuredBootRefusal {
    pub error: MeasuredStepError,
    pub configuration: ChipConfig,
    pub core: CoreId,
    pub pc: u32,
    pub symbol: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MeasuredBootStop {
    Ready {
        boot_cycle: u64,
        core_cycles: [u64; 2],
    },
    Refusal {
        boot_cycle: u64,
        core_cycles: [u64; 2],
        refusal: MeasuredBootRefusal,
    },
    StepLimit {
        core_cycles: [u64; 2],
    },
}

pub struct MeasuredBootScheduler {
    pub machine: crate::Machine,
    pub backend: Esp32Backend,
    core_cycles: [u64; 2],
}

impl MeasuredBootScheduler {
    pub fn new(machine: crate::Machine) -> Self {
        Self {
            machine,
            backend: Esp32Backend::default(),
            core_cycles: [0; 2],
        }
    }

    pub const fn core_cycles(&self) -> [u64; 2] {
        self.core_cycles
    }

    pub fn run_until(&mut self, ready_marker: &[u8], max_steps: u64) -> MeasuredBootStop {
        for _ in 0..max_steps {
            if self.console_contains(ready_marker) {
                return MeasuredBootStop::Ready {
                    boot_cycle: self.core_cycles[0],
                    core_cycles: self.core_cycles,
                };
            }
            let core = self.next_core();
            match self.advance_waiting_core(core) {
                Ok(true) => continue,
                Ok(false) => {}
                Err(error) => return self.refusal(core, error),
            }
            let core_index = core_index(core);
            let before = self.backend.engine().state().cores[core_index].cycle;
            if let Err(error) = self
                .machine
                .advance_measured_devices(self.core_cycles[core_index])
            {
                return self.refusal(core, error);
            }
            match self.machine.step_measured(&mut self.backend, core) {
                Ok(_) => {
                    let after = self.backend.engine().state().cores[core_index].cycle;
                    self.core_cycles[core_index] =
                        self.core_cycles[core_index].saturating_add(after.saturating_sub(before));
                }
                Err(error) => return self.refusal(core, error),
            }
        }
        MeasuredBootStop::StepLimit {
            core_cycles: self.core_cycles,
        }
    }

    fn next_core(&self) -> CoreId {
        if self.core_cycles[0] <= self.core_cycles[1] {
            CoreId::Core0
        } else {
            CoreId::Core1
        }
    }

    fn advance_waiting_core(&mut self, core: CoreId) -> Result<bool, MeasuredStepError> {
        let index = core_index(core);
        let cpu = &self.machine.cores[index];
        if !cpu.waiting || cpu.check_interrupts_pending() != 0 {
            return Ok(false);
        }
        let current = self.core_cycles[index];
        let other = self.core_cycles[1 - index];
        let other_ahead = (other > current).then_some(other);
        let target = [self.machine.next_measured_deadline(), other_ahead]
            .into_iter()
            .flatten()
            .min()
            .unwrap_or(current);
        let elapsed = target.saturating_sub(current);
        self.core_cycles[index] = target;
        advance_measured_clocks(&mut self.machine, core, elapsed);
        self.machine.advance_measured_devices(target)?;
        Ok(true)
    }

    fn console_contains(&self, marker: &[u8]) -> bool {
        !marker.is_empty()
            && [
                self.machine.console.all.as_slice(),
                self.machine.console.usb.as_slice(),
                self.machine.console.uart0.as_slice(),
                self.machine.bus.periph.usb.tx_out.as_slice(),
                self.machine.bus.periph.usb.tx_fifo.as_slice(),
                self.machine.bus.periph.uart[0].tx_out.as_slice(),
            ]
            .into_iter()
            .any(|bytes| bytes.windows(marker.len()).any(|window| window == marker))
    }

    fn refusal(&self, core: CoreId, error: MeasuredStepError) -> MeasuredBootStop {
        let index = core_index(core);
        let pc = self.machine.cores[index].pc;
        MeasuredBootStop::Refusal {
            boot_cycle: self.core_cycles[index],
            core_cycles: self.core_cycles,
            refusal: MeasuredBootRefusal {
                error,
                configuration: chip_config_from_registers(&self.machine),
                core,
                pc,
                symbol: self.machine.sym(pc),
            },
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExceptionEntryClass {
    WindowOverflow4,
    WindowOverflow8,
    WindowOverflow12,
    WindowUnderflow4,
    WindowUnderflow8,
    WindowUnderflow12,
    Syscall,
    IllegalInstruction,
    InstructionFetchError,
    LoadStoreError,
    LoadStoreAlignment,
    LoadError,
    StoreError,
    Other(u32),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExceptionReturnClass {
    Rfwo,
    Rfwu,
    Rfe,
    Rfue,
    Rfi,
    Rfde,
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
        backend.update_config(chip_config_from_registers(self));
        self.advance_measured_devices(self.bus.cycles)?;
        let index = core_index(core);
        let before_cycle = backend.engine().state().cores[index].cycle;
        let interrupt = self.cores[index].check_interrupts();
        if let Some(Trap::Interrupt(irq)) = interrupt {
            return Err(MeasuredStepError::Unpriced(
                UnpricedTimingClass::InterruptEntry { irq },
            ));
        }

        let observation = {
            let cpu = &self.cores[index];
            observe_instruction(core, cpu, &self.bus).map_err(MeasuredStepError::Plan)?
        };
        if observation.fetch_memory == MemoryClass::MaskRom {
            return Err(MeasuredStepError::Unpriced(
                UnpricedTimingClass::MaskRomInstructionFetch,
            ));
        }
        if let Some(class) = exception_return_class(observation.instruction.op) {
            return Err(MeasuredStepError::Unpriced(
                UnpricedTimingClass::ExceptionReturn(class),
            ));
        }
        if let Some(source) = backend.cache_operations(&observation).dirty_eviction {
            return Err(MeasuredStepError::Unpriced(
                UnpricedTimingClass::DirtyCacheEvictionWriteback(source),
            ));
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
            Err(CompletionError::Trap(Trap::Exception(cause))) => Err(MeasuredStepError::Unpriced(
                UnpricedTimingClass::ExceptionEntry(exception_entry_class(cause)),
            )),
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

const fn exception_return_class(op: Op) -> Option<ExceptionReturnClass> {
    match op {
        Op::Rfwo => Some(ExceptionReturnClass::Rfwo),
        Op::Rfwu => Some(ExceptionReturnClass::Rfwu),
        Op::Rfe => Some(ExceptionReturnClass::Rfe),
        Op::Rfue => Some(ExceptionReturnClass::Rfue),
        Op::Rfi => Some(ExceptionReturnClass::Rfi),
        Op::Rfde => Some(ExceptionReturnClass::Rfde),
        _ => None,
    }
}

const fn exception_entry_class(cause: u32) -> ExceptionEntryClass {
    match cause {
        0x201 => ExceptionEntryClass::WindowOverflow4,
        0x202 => ExceptionEntryClass::WindowOverflow8,
        0x203 => ExceptionEntryClass::WindowOverflow12,
        0x301 => ExceptionEntryClass::WindowUnderflow4,
        0x302 => ExceptionEntryClass::WindowUnderflow8,
        0x303 => ExceptionEntryClass::WindowUnderflow12,
        exc::SYSCALL => ExceptionEntryClass::Syscall,
        exc::ILLEGAL => ExceptionEntryClass::IllegalInstruction,
        exc::IFETCH_ERROR
        | exc::IFETCH_PIF_DATA_ERROR
        | exc::IFETCH_PIF_ADDR_ERROR
        | exc::ITLB_MISS
        | exc::IFETCH_PROHIBITED => ExceptionEntryClass::InstructionFetchError,
        exc::LOAD_STORE_ERROR | exc::LS_PIF_DATA_ERROR | exc::LS_PIF_ADDR_ERROR => {
            ExceptionEntryClass::LoadStoreError
        }
        exc::LOAD_STORE_ALIGNMENT => ExceptionEntryClass::LoadStoreAlignment,
        exc::DTLB_MISS | exc::LOAD_PROHIBITED => ExceptionEntryClass::LoadError,
        exc::STORE_PROHIBITED => ExceptionEntryClass::StoreError,
        other => ExceptionEntryClass::Other(other),
    }
}

fn chip_config_from_registers(machine: &crate::Machine) -> ChipConfig {
    let system = &machine.bus.periph.system.ram;
    let spi0 = &machine.bus.periph.spi0.regs;
    let spi1 = &machine.bus.periph.spi1.regs;
    let extmem = &machine.bus.periph.extmem.ram;
    let cpu_mhz = match ((system.read(0x60) >> 10) & 3, system.read(0x10) & 3) {
        (0, _) => 40,
        (1, 0) => 80,
        (1, 1) => 160,
        (1, 2) => 240,
        _ => 40,
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
        apb_mhz: cpu_mhz.min(80),
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
    use emu_core::{Bus as _, Core as _};
    use xtensa_lx7::measured::BlockCostPayload;
    use xtensa_lx7::state::ps;

    const RESET_PC: u32 = 0x4038_0000;
    const BEQZ_N_A6: [u8; 2] = [0x8c, 0x06];

    fn retired_facts<'a>(
        pc: u32,
        bytes: [u8; 4],
        length: u8,
        next_pc: u32,
        accesses: &'a [MemoryAccess],
    ) -> ExecutionFacts<'a> {
        ExecutionFacts {
            core: 0,
            outcome: emu_core::StepOutcome {
                pc,
                next_pc,
                bytes: Some(bytes),
                length,
                kind: StepKind::Retired,
                control: None,
            },
            accesses,
        }
    }

    struct FailingDeadlineBoard;

    impl crate::board::BoardModel for FailingDeadlineBoard {
        fn name(&self) -> &'static str { "failing-deadline" }

        fn next_deadline(&self) -> Option<backend_api::VirtualCycle> {
            Some(7)
        }

        fn advance_to(
            &mut self,
            cycle: backend_api::VirtualCycle,
        ) -> Result<(), crate::board::BoardDeadlineError> {
            Err(crate::board::BoardDeadlineError::TimeReversed {
                current: cycle + 1,
                requested: cycle,
            })
        }
    }

    fn instruction_machine(instruction: &[u8]) -> crate::Machine {
        let mut machine = crate::machine([0; 6]);
        set_receipt_config_registers(&mut machine);
        machine
            .bus
            .load_bytes(RESET_PC, instruction)
            .expect("test instruction maps in IRAM");
        machine.cores[0].pc = RESET_PC;
        machine
    }

    fn static_loop_machine(jit: bool, deadline: Option<u64>) -> (crate::Machine, Esp32CostModel) {
        let mut program = vec![0x22, 0xa0, 1, 0x32, 0xa0, 2];
        let offset = RESET_PC.wrapping_sub(RESET_PC + 6 + 4) as i32;
        let jump = ((offset as u32 & 0x3ffff) << 6) | 6;
        program.extend_from_slice(&[jump as u8, (jump >> 8) as u8, (jump >> 16) as u8]);
        let mut machine = instruction_machine(&program);
        machine.cores[0].set_jit(jit);
        machine.cores[1].set_jit(jit);
        machine
            .bus
            .write32(0x600c_0000, 0b010)
            .expect("release core 1");
        if let Some(deadline) = deadline {
            machine
                .script
                .events
                .push((deadline, esp_soc::ScriptAction::Stop));
        }
        let model = Esp32CostModel::default();
        machine
            .set_cost_model(Box::new(model.clone()))
            .expect("pristine machine accepts product timing");
        assert!(matches!(machine.run(0), esp_soc::Stop::MaxInsns));
        machine.cores[0].pc = RESET_PC;
        machine.cores[1].pc = RESET_PC;
        (machine, model)
    }

    struct RefusingBatchModel {
        inner: Esp32CostModel,
        calls: usize,
        refuse_at: usize,
    }

    struct RefusingCheckpoint {
        inner: Box<dyn std::any::Any>,
        calls: usize,
    }

    impl CostModel for RefusingBatchModel {
        fn lifecycle(&mut self, facts: &LifecycleFacts) -> Result<(), String> {
            self.inner.lifecycle(facts)
        }

        fn cycles(&mut self, facts: &ExecutionFacts<'_>) -> Result<u32, String> {
            if self.calls == self.refuse_at {
                return Err("test refusal".into());
            }
            self.calls += 1;
            self.inner.cycles(facts)
        }

        fn commit_modeled_block(&mut self, events: &[ModeledBlockEvent]) -> Option<Result<(), String>> {
            if self.calls == self.refuse_at {
                return None;
            }
            self.calls += events.len();
            if self.calls > self.refuse_at {
                return None;
            }
            self.inner.commit_modeled_block(events)
        }

        fn begin_batch(&mut self) -> Option<Box<dyn std::any::Any>> {
            Some(Box::new(RefusingCheckpoint {
                inner: self.inner.begin_batch()?,
                calls: self.calls,
            }))
        }

        fn rollback_batch(
            &mut self,
            checkpoint: Box<dyn std::any::Any>,
        ) -> Result<(), String> {
            let checkpoint = checkpoint
                .downcast::<RefusingCheckpoint>()
                .map_err(|_| "bad test checkpoint".to_string())?;
            self.calls = checkpoint.calls;
            self.inner.rollback_batch(checkpoint.inner)
        }
    }

    fn branch_machine(register: u32) -> crate::Machine {
        let mut machine = instruction_machine(&BEQZ_N_A6);
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

    #[test]
    fn machine_hook_prices_branch_outcomes() {
        let fetch = [MemoryAccess {
            kind: MemoryAccessKind::Fetch,
            address: RESET_PC,
            width: 4,
            value: u32::from_le_bytes([BEQZ_N_A6[0], BEQZ_N_A6[1], 0, 0]),
            fault: None,
        }];
        let mut taken = Esp32Backend::default();
        let facts = retired_facts(
            RESET_PC,
            [BEQZ_N_A6[0], BEQZ_N_A6[1], 0, 0],
            2,
            RESET_PC + 8,
            &fetch,
        );
        assert_eq!(CostModel::cycles(&mut taken, &facts), Ok(3));

        let mut not_taken = Esp32Backend::default();
        let facts = retired_facts(
            RESET_PC,
            [BEQZ_N_A6[0], BEQZ_N_A6[1], 0, 0],
            2,
            RESET_PC + 2,
            &fetch,
        );
        assert_eq!(CostModel::cycles(&mut not_taken, &facts), Ok(1));
    }

    #[test]
    fn machine_hook_tracks_mmu_writes_before_cache_pricing() {
        let store_bytes = [0x29, 0x08, 0, 0];
        assert_eq!(xtensa_lx7::decode(RESET_PC, store_bytes).op, Op::S32iN);
        let store_accesses = [
            MemoryAccess {
                kind: MemoryAccessKind::Fetch,
                address: RESET_PC,
                width: 4,
                value: u32::from_le_bytes(store_bytes),
                fault: None,
            },
            MemoryAccess {
                kind: MemoryAccessKind::Write,
                address: crate::bus::MMU_TABLE,
                width: 4,
                value: 0,
                fault: None,
            },
        ];
        let mut model = Esp32Backend::default();
        let store = retired_facts(RESET_PC, store_bytes, 2, RESET_PC + 2, &store_accesses);
        assert_eq!(CostModel::cycles(&mut model, &store), Ok(1));

        let fetch_bytes = [0xf0, 0x20, 0, 0];
        let flash_accesses = [MemoryAccess {
            kind: MemoryAccessKind::Fetch,
            address: 0x4200_0000,
            width: 4,
            value: u32::from_le_bytes(fetch_bytes),
            fault: None,
        }];
        let fetch = retired_facts(0x4200_0000, fetch_bytes, 3, 0x4200_0003, &flash_accesses);
        assert_eq!(CostModel::cycles(&mut model, &fetch), Ok(204));

        let remap_accesses = [
            store_accesses[0],
            MemoryAccess {
                address: crate::bus::MMU_TABLE + 4,
                ..store_accesses[1]
            },
        ];
        let remap = retired_facts(RESET_PC, store_bytes, 2, RESET_PC + 2, &remap_accesses);
        assert_eq!(
            CostModel::cycles(&mut model, &remap),
            Err("MmuRemapWithLiveCache: cost not adopted (Unexplained tier)".into())
        );
    }

    #[test]
    fn machine_hook_refuses_unpriced_mask_rom_fetch() {
        let mut machine = crate::machine([0; 6]);
        machine
            .bus
            .load_bytes(crate::bus::IROM_MASK_LOW, &[0xf0, 0x20, 0x00])
            .expect("mask ROM test instruction maps");
        machine.cores[0].pc = crate::bus::IROM_MASK_LOW;
        machine
            .set_cost_model(Box::new(Esp32Backend::default()))
            .expect("pristine S3 machine accepts the timing model");
        assert!(matches!(
            machine.run(1),
            esp_soc::Stop::CostModel { core: 0, pc: crate::bus::IROM_MASK_LOW, ref reason }
                if reason == "MaskRomInstructionFetch: cost not adopted (Unexplained tier)"
        ));
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

    fn assert_step_measured_cache_burst(cache: CacheKind, lines: usize) {
        let (instruction, base, line_bytes) = match cache {
            CacheKind::InstructionFlash => (&[0xf0, 0x20, 0x00][..], 0x4200_0000, 32),
            CacheKind::DataFlash => (&[0x22, 0x23, 0x00][..], 0x3c00_0000, 64),
            CacheKind::DataPsram => (&[0x22, 0x23, 0x00][..], 0x3d00_0000, 64),
        };
        let mut machine = instruction_machine(instruction);
        match cache {
            CacheKind::InstructionFlash | CacheKind::DataFlash => machine.bus.mmu[0] = 0,
            CacheKind::DataPsram => machine.bus.mmu[256] = crate::bus::MMU_SPIRAM,
        }
        if cache == CacheKind::InstructionFlash {
            for line in 0..lines {
                machine
                    .bus
                    .load_bytes(base + line as u32 * line_bytes, instruction)
                    .expect("flash burst instruction maps");
            }
        }
        let mut backend = Esp32Backend::default();
        for line in 0..lines {
            let address = base + line as u32 * line_bytes;
            if cache == CacheKind::InstructionFlash {
                machine.cores[0].pc = address;
            } else {
                machine.cores[0].pc = RESET_PC;
                machine.cores[0].set_ar(3, address);
            }
            assert_eq!(
                machine.step_measured(&mut backend, CoreId::Core0),
                Ok(MeasuredStep::Instruction)
            );
        }

        let fills: Vec<_> = backend
            .engine()
            .ledger()
            .iter()
            .flat_map(|entry| &entry.components)
            .filter_map(|component| match component.class {
                CostClass::CacheLineFill {
                    cache: component_cache,
                    position,
                } if component_cache == cache => Some(position),
                _ => None,
            })
            .collect();
        assert_eq!(fills.len(), lines);
        assert_eq!(fills[0], CacheFillPosition::First);
        assert!(fills[1..]
            .iter()
            .all(|position| *position == CacheFillPosition::Subsequent));

        let fills_before_hot_replay = fills.len();
        for line in 0..lines {
            let address = base + line as u32 * line_bytes;
            if cache == CacheKind::InstructionFlash {
                machine.cores[0].pc = address;
            } else {
                machine.cores[0].pc = RESET_PC;
                machine.cores[0].set_ar(3, address);
            }
            assert_eq!(
                machine.step_measured(&mut backend, CoreId::Core0),
                Ok(MeasuredStep::Instruction)
            );
        }
        assert_eq!(
            backend
                .engine()
                .ledger()
                .iter()
                .flat_map(|entry| &entry.components)
                .filter(|component| matches!(component.class, CostClass::CacheLineFill { cache: component_cache, .. } if component_cache == cache))
                .count(),
            fills_before_hot_replay
        );
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
    fn reset_configuration_uses_the_xtal_clock_and_register_defaults() {
        let machine = crate::machine([0; 6]);
        assert_eq!(
            chip_config_from_registers(&machine),
            ChipConfig {
                cpu_mhz: 40,
                apb_mhz: 40,
                flash_mode: FlashMode::Other,
                flash_mhz: 160,
                psram_mode: PsramMode::Other,
                psram_mhz: 160,
                icache_size_bytes: 16 * 1024,
                icache_ways: 4,
                icache_line_bytes: 16,
                dcache_size_bytes: 32 * 1024,
                dcache_ways: 8,
                dcache_line_bytes: 16,
            }
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
    fn receipt_cache_bursts_replay_through_step_measured() {
        for cache in [
            CacheKind::InstructionFlash,
            CacheKind::DataFlash,
            CacheKind::DataPsram,
        ] {
            for lines in [1, 2, 4, 8, 16] {
                assert_step_measured_cache_burst(cache, lines);
            }
        }
    }

    #[test]
    fn dirty_psram_eviction_refuses_named_writeback_class() {
        let mut machine = instruction_machine(&[0x22, 0x63, 0x00]);
        machine.bus.mmu[256] = crate::bus::MMU_SPIRAM;
        let mut backend = Esp32Backend::default();
        for way in 0..8 {
            machine.cores[0].pc = RESET_PC;
            machine.cores[0].set_ar(3, 0x3d00_0000 + way * 4096);
            assert_eq!(
                machine.step_measured(&mut backend, CoreId::Core0),
                Ok(MeasuredStep::Instruction)
            );
        }
        machine.cores[0].pc = RESET_PC;
        machine.cores[0].set_ar(3, 0x3d00_8000);
        assert_eq!(
            format!("{:?}", machine.step_measured(&mut backend, CoreId::Core0)),
            "Err(Unpriced(DirtyCacheEvictionWriteback(Psram)))"
        );
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
            assert_eq!(machine.cores[0].ccount, expected as u32);
            assert_eq!(machine.bus.cycles, expected);
        }
    }

    #[test]
    fn costed_native_jit_matches_measured_interpreter_jump_ledger() {
        if !xtensa_lx7::jit::AVAILABLE {
            return;
        }
        let instruction = [0x06, 0xff, 0xff];
        assert_eq!(xtensa_lx7::decode(RESET_PC, [0x06, 0xff, 0xff, 0]).op, Op::J);

        let mut reference_machine = instruction_machine(&instruction);
        let mut reference_backend = Esp32Backend::default();
        for _ in 0..2 {
            assert_eq!(
                reference_machine.step_measured(&mut reference_backend, CoreId::Core0),
                Ok(MeasuredStep::Instruction)
            );
        }
        let reference_ledger = reference_backend
            .run_trace(&[])
            .expect("measured interpreter ledger")
            .canonical_ledger;

        let mut jit_machine = instruction_machine(&instruction);
        let model = Esp32CostModel::default();
        let report = model.clone();
        jit_machine
            .set_cost_model(Box::new(model))
            .expect("pristine machine accepts product timing");
        assert!(matches!(jit_machine.run(2), esp_soc::Stop::MaxInsns));

        let (_, _, compiled, _) = emu_core::Core::code_cache_stats(&jit_machine.cores[0])
            .expect("LX7 exposes native code statistics");
        assert_eq!(compiled, 1);
        assert_eq!(jit_machine.cores[0].ccount, 6);
        assert_eq!(report.core_cycles(), Ok([6, 0]));
        assert_eq!(report.canonical_ledger(), Ok(reference_ledger));
    }

    #[test]
    fn costed_native_blocks_match_dual_core_scheduler_and_script_deadline() {
        if !xtensa_lx7::jit::AVAILABLE {
            return;
        }
        for deadline in [None, Some(4)] {
            let (mut reference, reference_report) = static_loop_machine(false, deadline);
            let (mut batched, batched_report) = static_loop_machine(true, deadline);
            let reference_stop = reference.run(60);
            let batched_stop = batched.run(60);
            assert_eq!(
                matches!(reference_stop, esp_soc::Stop::Halted),
                matches!(batched_stop, esp_soc::Stop::Halted),
            );
            for core in 0..2 {
                assert_eq!(batched.cores[core].ar, reference.cores[core].ar);
                assert_eq!(batched.cores[core].pc, reference.cores[core].pc);
                assert_eq!(batched.cores[core].ccount, reference.cores[core].ccount);
                assert_eq!(batched.cores[core].insn_count, reference.cores[core].insn_count);
                assert!(batched.cores[core].blocks.costed_native_insns > 1);
                assert!(
                    batched.cores[core].blocks.costed_native_insns
                        > batched.cores[core].blocks.costed_native_runs
                );
            }
            assert_eq!(batched.bus.cycles, reference.bus.cycles);
            assert_eq!(
                batched_report.canonical_ledger(),
                reference_report.canonical_ledger(),
            );
        }
    }

    #[test]
    fn costed_native_batch_rolls_back_before_later_refusal() {
        if !xtensa_lx7::jit::AVAILABLE {
            return;
        }
        let mut machine = instruction_machine(&[
            0x22, 0xa0, 1, 0x32, 0xa0, 2, 0x06, 0xff, 0xff,
        ]);
        let report = Esp32CostModel::default();
        machine
            .set_cost_model(Box::new(RefusingBatchModel {
                inner: report.clone(),
                calls: 0,
                refuse_at: 1,
            }))
            .expect("pristine machine accepts product timing");

        let stop = machine.run(10);
        assert!(matches!(
            stop,
            esp_soc::Stop::CostModel { core: 0, pc, ref reason }
                if pc == RESET_PC + 3 && reason == "test refusal"
        ));
        assert_eq!(machine.cores[0].get_ar(2), 1);
        assert_eq!(machine.cores[0].get_ar(3), 2);
        assert_eq!(machine.cores[0].insn_count, 2);
        assert_eq!(report.core_cycles(), Ok([1, 0]));
    }

    #[test]
    fn costed_native_jit_defers_dynamic_loop_edge_to_interpreter() {
        if !xtensa_lx7::jit::AVAILABLE {
            return;
        }
        let instruction = [0x22, 0xa0, 0x00];
        assert_eq!(xtensa_lx7::decode(RESET_PC, [0x22, 0xa0, 0, 0]).op, Op::Movi);

        let mut machine = instruction_machine(&instruction);
        machine.cores[0].lbeg = RESET_PC - 1;
        machine.cores[0].lend = RESET_PC + 3;
        machine.cores[0].lcount = 1;
        let model = Esp32CostModel::default();
        let report = model.clone();
        machine
            .set_cost_model(Box::new(model))
            .expect("pristine machine accepts product timing");
        assert!(matches!(machine.run(1), esp_soc::Stop::MaxInsns));

        let (_, _, compiled, _) = emu_core::Core::code_cache_stats(&machine.cores[0])
            .expect("LX7 exposes native code statistics");
        assert_eq!(compiled, 0);
        assert_eq!(machine.cores[0].pc, RESET_PC - 1);
        assert_eq!(machine.cores[0].lcount, 0);
        assert_eq!(machine.cores[0].ccount, 2);
        assert_eq!(report.core_cycles(), Ok([2, 0]));
    }

    #[test]
    fn modeled_then_fast_recompiles_for_the_normal_jit_abi() {
        if !xtensa_lx7::jit::AVAILABLE {
            return;
        }
        let mut machine = instruction_machine(&[0x06, 0xff, 0xff]);
        emu_core::Core::set_costed_jit(&mut machine.cores[0], true);
        let modeled = emu_core::Core::step_modeled(&mut machine.cores[0], &mut machine.bus);
        assert_eq!(modeled.execution, emu_core::ModeledExecution::Compiled);
        assert_eq!(modeled.applied_cycles, 3);

        assert!(matches!(machine.run(1), esp_soc::Stop::MaxInsns));
        let (_, _, compiled, _) = emu_core::Core::code_cache_stats(&machine.cores[0])
            .expect("LX7 exposes native code statistics");
        assert_eq!(compiled, 2);
        assert_eq!(machine.cores[0].ccount, 67);
    }

    #[test]
    fn fast_then_modeled_recompiles_for_the_costed_jit_abi() {
        if !xtensa_lx7::jit::AVAILABLE {
            return;
        }
        let mut machine = instruction_machine(&[0x06, 0xff, 0xff]);
        let model = Esp32CostModel::default();
        let report = model.clone();
        machine
            .set_cost_model(Box::new(model))
            .expect("pristine machine accepts product timing");

        assert_eq!(
            emu_core::Core::run(&mut machine.cores[0], &mut machine.bus, 1),
            (1, None)
        );
        assert!(matches!(machine.run(1), esp_soc::Stop::MaxInsns));

        let (_, _, compiled, _) = emu_core::Core::code_cache_stats(&machine.cores[0])
            .expect("LX7 exposes native code statistics");
        assert_eq!(compiled, 2);
        assert_eq!(machine.cores[0].ccount, 4);
        assert_eq!(report.core_cycles(), Ok([3, 0]));
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
    fn waiting_core_deadline_failure_stops_with_typed_refusal() {
        let mut machine = crate::machine([0; 6]);
        machine.bus.board = Box::new(FailingDeadlineBoard);
        machine.cores[0].waiting = true;
        let mut scheduler = MeasuredBootScheduler::new(machine);

        let stop = scheduler.run_until(b"never-ready", 1);

        assert!(matches!(
            stop,
            MeasuredBootStop::Refusal {
                boot_cycle: 7,
                core_cycles: [7, 0],
                refusal: MeasuredBootRefusal {
                    error: MeasuredStepError::Deadline(crate::board::BoardDeadlineError::TimeReversed {
                        current: 8,
                        requested: 7,
                    }),
                    core: CoreId::Core0,
                    ..
                },
            }
        ));
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
            Err(MeasuredStepError::Unpriced(
                UnpricedTimingClass::ExceptionEntry(ExceptionEntryClass::WindowOverflow12)
            ))
        ));
        assert_eq!(backend.engine().state(), &before);
        assert!(backend.engine().ledger().is_empty());
        assert_eq!(machine.bus.cycles, 0);
    }

    #[test]
    fn hardware_exception_entries_refuse_by_typed_class() {
        for (instruction, register, expected) in [
            ([0x00, 0x50, 0x00], None, ExceptionEntryClass::Syscall),
            (
                [0x00, 0x00, 0x00],
                None,
                ExceptionEntryClass::IllegalInstruction,
            ),
            (
                [0x22, 0x23, 0x00],
                Some(0x7000_0000),
                ExceptionEntryClass::LoadError,
            ),
            (
                [0x22, 0x63, 0x00],
                Some(0x7000_0000),
                ExceptionEntryClass::StoreError,
            ),
        ] {
            let mut machine = instruction_machine(&instruction);
            if let Some(value) = register {
                machine.cores[0].set_ar(3, value);
            }
            let mut backend = Esp32Backend::default();
            assert_eq!(
                machine.step_measured(&mut backend, CoreId::Core0),
                Err(MeasuredStepError::Unpriced(
                    UnpricedTimingClass::ExceptionEntry(expected)
                ))
            );
            assert!(backend.engine().ledger().is_empty());
        }
    }

    #[test]
    fn exception_return_family_refuses_by_instruction_name() {
        for (instruction, expected) in [
            ([0x00, 0x34, 0x00], ExceptionReturnClass::Rfwo),
            ([0x00, 0x35, 0x00], ExceptionReturnClass::Rfwu),
            ([0x00, 0x30, 0x00], ExceptionReturnClass::Rfe),
            ([0x10, 0x33, 0x00], ExceptionReturnClass::Rfi),
            ([0x00, 0x32, 0x00], ExceptionReturnClass::Rfde),
        ] {
            let mut machine = instruction_machine(&instruction);
            let mut backend = Esp32Backend::default();
            assert_eq!(
                machine.step_measured(&mut backend, CoreId::Core0),
                Err(MeasuredStepError::Unpriced(
                    UnpricedTimingClass::ExceptionReturn(expected)
                ))
            );
            assert!(backend.engine().ledger().is_empty());
        }
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
