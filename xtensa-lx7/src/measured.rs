//! Side-effect-free planning and transactional completion for measured LX7 execution.

use crate::bus::{Bus, Fault, FlatRam};
use crate::decode::{decode, Insn};
use crate::exec::{exec_insn, max_ar, Trap};
use crate::state::Cpu;
use backend_api::{CoreId, CostComponent, TimingMutation, TimingRefusal, VirtualCycle};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AccessKind {
    Load,
    Store,
    Atomic,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AccessShape {
    pub kind: AccessKind,
    pub address: u32,
    pub width: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MemoryClass {
    InternalSram,
    MaskRom,
    Flash,
    Psram,
    Rtc,
    Mmio,
    Unknown,
}

/// Cost payload shared by an interpreted block and its future JIT lowering.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BlockCostPayload {
    pub start_pc: u32,
    pub static_cycles: u64,
    pub components: Vec<CostComponent>,
}

#[derive(Clone, Debug)]
pub struct InstructionObservation {
    pub core: CoreId,
    pub pc: u32,
    pub bytes: [u8; 4],
    pub instruction: Insn,
    pub fetch_memory: MemoryClass,
    pub access: Option<AccessShape>,
    pub access_memory: Option<MemoryClass>,
    pub branch_taken: Option<bool>,
    pub block_cost: BlockCostPayload,
}

/// Typed price and mutations produced without changing timing state.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TimingPlan {
    pub cycles: u64,
    pub components: Vec<CostComponent>,
    pub mutations: Vec<TimingMutation>,
}

pub trait TimingSource {
    fn price(&self, observation: &InstructionObservation) -> Result<TimingPlan, TimingRefusal>;
    fn commit(
        &mut self,
        observation: &InstructionObservation,
        components: &[CostComponent],
        mutations: &[TimingMutation],
    ) -> Result<(), TimingRefusal>;
}

/// Planning-only view of the bus. Implementations must not mutate guest,
/// device, cache, fault, or timing state while serving these methods.
pub trait MeasuredBus: Bus {
    fn measured_fetch(&self, pc: u32) -> Result<[u8; 4], Fault>;
    fn measured_memory_class(&self, address: u32) -> MemoryClass;
}

impl MeasuredBus for FlatRam {
    fn measured_fetch(&self, pc: u32) -> Result<[u8; 4], Fault> {
        let offset = pc.wrapping_sub(self.base) as usize;
        let mut bytes = [0u8; 4];
        if offset >= self.mem.len() {
            return Err(Fault::Unmapped);
        }
        let available = (self.mem.len() - offset).min(bytes.len());
        bytes[..available].copy_from_slice(&self.mem[offset..offset + available]);
        Ok(bytes)
    }

    fn measured_memory_class(&self, address: u32) -> MemoryClass {
        if address.wrapping_sub(self.base) < self.mem.len() as u32 {
            MemoryClass::InternalSram
        } else {
            MemoryClass::Unknown
        }
    }
}

#[derive(Clone, Debug)]
pub struct PendingInstruction {
    pub start: VirtualCycle,
    pub completion: VirtualCycle,
    pub observation: InstructionObservation,
    pub components: Vec<CostComponent>,
    mutations: Vec<TimingMutation>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlanError {
    Fetch { pc: u32 },
    Timing(TimingRefusal),
    CompletionOverflow,
}

pub fn plan_instruction<B: MeasuredBus, T: TimingSource>(
    core: CoreId,
    cpu: &Cpu,
    bus: &B,
    timing: &T,
    now: VirtualCycle,
) -> Result<PendingInstruction, PlanError> {
    let pc = cpu.pc;
    let bytes = bus
        .measured_fetch(pc)
        .map_err(|_| PlanError::Fetch { pc })?;
    let instruction = decode(pc, bytes);
    let access = access_shape(cpu, instruction);
    let mut observation = InstructionObservation {
        core,
        pc,
        bytes,
        instruction,
        fetch_memory: bus.measured_memory_class(pc),
        access_memory: access.map(|shape| bus.measured_memory_class(shape.address)),
        access,
        branch_taken: matches!(instruction.op, crate::Op::Beqz | crate::Op::BeqzN)
            .then(|| cpu.get_ar(instruction.s) == 0),
        block_cost: BlockCostPayload {
            start_pc: pc,
            static_cycles: 0,
            components: Vec::new(),
        },
    };
    let plan = timing.price(&observation).map_err(PlanError::Timing)?;
    observation.block_cost.static_cycles = plan
        .components
        .iter()
        .filter_map(|component| match component.expression {
            backend_api::CostExpression::Exact(cycles) => Some(cycles),
            backend_api::CostExpression::Affine { .. } => None,
        })
        .try_fold(0u64, u64::checked_add)
        .ok_or(PlanError::CompletionOverflow)?;
    observation
        .block_cost
        .components
        .clone_from(&plan.components);
    let completion = now
        .checked_add(plan.cycles)
        .ok_or(PlanError::CompletionOverflow)?;
    Ok(PendingInstruction {
        start: now,
        completion,
        observation,
        components: plan.components,
        mutations: plan.mutations,
    })
}

fn access_shape(cpu: &Cpu, instruction: Insn) -> Option<AccessShape> {
    use crate::Op;
    let immediate_address = || {
        cpu.get_ar(instruction.s)
            .wrapping_add(instruction.imm as u32)
    };
    let indexed_address = || {
        cpu.get_ar(instruction.s)
            .wrapping_add(cpu.get_ar(instruction.t))
    };
    let (kind, address, width) = match instruction.op {
        Op::L8ui => (AccessKind::Load, immediate_address(), 1),
        Op::L16ui | Op::L16si => (AccessKind::Load, immediate_address(), 2),
        Op::L32i | Op::L32iN | Op::L32ai | Op::L32e | Op::Lsi | Op::Lsip => {
            (AccessKind::Load, immediate_address(), 4)
        }
        Op::L32r => (AccessKind::Load, instruction.imm as u32, 4),
        Op::Lsx | Op::Lsxp => (AccessKind::Load, indexed_address(), 4),
        Op::S8i => (AccessKind::Store, immediate_address(), 1),
        Op::S16i => (AccessKind::Store, immediate_address(), 2),
        Op::S32i | Op::S32iN | Op::S32ri | Op::S32e | Op::S32nb | Op::Ssi | Op::Ssip => {
            (AccessKind::Store, immediate_address(), 4)
        }
        Op::Ssx | Op::Ssxp => (AccessKind::Store, indexed_address(), 4),
        Op::S32c1i => (AccessKind::Atomic, immediate_address(), 4),
        _ => return None,
    };
    Some(AccessShape {
        kind,
        address,
        width,
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CompletionError {
    BeforeCompletion,
    Trap(Trap),
    Timing(TimingRefusal),
}

pub fn complete_instruction<B: MeasuredBus, T: TimingSource>(
    cpu: &mut Cpu,
    bus: &mut B,
    timing: &mut T,
    pending: PendingInstruction,
    now: VirtualCycle,
) -> Result<(), CompletionError> {
    if now < pending.completion {
        return Err(CompletionError::BeforeCompletion);
    }
    if let Some(trap) = cpu.check_overflow(max_ar(&pending.observation.instruction)) {
        return Err(CompletionError::Trap(trap));
    }
    bus.note_pc(cpu.pc);
    let result = exec_insn(cpu, bus, &pending.observation.instruction);
    cpu.insn_count = cpu.insn_count.saturating_add(1);
    match result {
        Ok(()) => {
            timing
                .commit(
                    &pending.observation,
                    &pending.components,
                    &pending.mutations,
                )
                .map_err(CompletionError::Timing)?;
            Ok(())
        }
        Err(trap) => Err(CompletionError::Trap(trap)),
    }
}

#[derive(Clone)]
pub struct MeasuredCore {
    pub cpu: Cpu,
    pub pending: Option<PendingInstruction>,
}

/// Native two-core measured scheduler shape.
#[derive(Clone)]
pub struct MeasuredScheduler {
    pub cores: [MeasuredCore; 2],
    pub now: VirtualCycle,
}

impl Default for MeasuredScheduler {
    fn default() -> Self {
        Self {
            cores: [
                MeasuredCore {
                    cpu: Cpu::new(0),
                    pending: None,
                },
                MeasuredCore {
                    cpu: Cpu::new(1),
                    pending: None,
                },
            ],
            now: 0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use backend_api::{
        price_operation, CacheFillPosition, CacheKind, CostClass, Operation, ReceiptId,
    };

    #[derive(Clone, Debug, Default, PartialEq, Eq)]
    struct FixedTiming {
        applied: Vec<TimingMutation>,
    }

    impl TimingSource for FixedTiming {
        fn price(&self, observation: &InstructionObservation) -> Result<TimingPlan, TimingRefusal> {
            let (component, _) = price_operation(
                observation.core,
                Operation::CacheLineFill {
                    cache: CacheKind::DataFlash,
                    position: CacheFillPosition::Subsequent,
                    line: 1,
                },
            )?;
            Ok(TimingPlan {
                cycles: component
                    .cycles()
                    .expect("the adopted exact cost is representable"),
                components: vec![component],
                mutations: vec![TimingMutation::RecordCacheFill {
                    core: observation.core,
                    cache: CacheKind::DataFlash,
                    line: 1,
                }],
            })
        }

        fn commit(
            &mut self,
            _observation: &InstructionObservation,
            _components: &[CostComponent],
            mutations: &[TimingMutation],
        ) -> Result<(), TimingRefusal> {
            self.applied.extend_from_slice(mutations);
            Ok(())
        }
    }

    fn ram_with_instruction(bytes: &[u8]) -> FlatRam {
        let mut ram = FlatRam::new(0x4000_0000, 0x1000);
        ram.mem[0x400..0x400 + bytes.len()].copy_from_slice(bytes);
        ram
    }

    #[test]
    fn scheduler_is_structurally_dual_core() {
        let scheduler = MeasuredScheduler::default();
        assert_eq!(scheduler.cores.len(), 2);
        assert_eq!(scheduler.cores[0].cpu.prid, 0);
        assert_eq!(scheduler.cores[1].cpu.prid, 1);
    }

    #[test]
    fn planning_is_side_effect_free_and_exposes_jit_payload() {
        let cpu = Cpu::new(0);
        let ram = ram_with_instruction(&[0xf0, 0x20, 0x00]);
        let timing = FixedTiming::default();
        let pending =
            plan_instruction(CoreId::Core0, &cpu, &ram, &timing, 7).expect("instruction plans");
        assert_eq!(pending.start, 7);
        assert_eq!(pending.completion, 480);
        assert_eq!(pending.observation.block_cost.start_pc, 0x4000_0400);
        assert_eq!(pending.observation.block_cost.static_cycles, 473);
        assert_eq!(
            pending.observation.block_cost.components,
            pending.components
        );
        assert_eq!(cpu.pc, 0x4000_0400);
        assert!(timing.applied.is_empty());
        assert_eq!(
            pending.components[0].class,
            CostClass::CacheLineFill {
                cache: CacheKind::DataFlash,
                position: CacheFillPosition::Subsequent,
            }
        );
        assert_eq!(
            pending.components[0].receipt,
            ReceiptId::CacheBurstAdoptionA91d1d7
        );
    }

    #[test]
    fn successful_completion_commits_typed_mutations_once() {
        let mut cpu = Cpu::new(0);
        let mut ram = ram_with_instruction(&[0xf0, 0x20, 0x00]);
        let mut timing = FixedTiming::default();
        let pending =
            plan_instruction(CoreId::Core0, &cpu, &ram, &timing, 0).expect("instruction plans");
        complete_instruction(&mut cpu, &mut ram, &mut timing, pending, 473)
            .expect("instruction completes");
        assert_eq!(timing.applied.len(), 1);
        assert_eq!(cpu.pc, 0x4000_0403);
    }

    #[test]
    fn faulted_instruction_discards_timing_mutations() {
        let mut cpu = Cpu::new(0);
        cpu.set_ar(3, 0x5000_0000);
        let mut ram = ram_with_instruction(&[0x22, 0x23, 0x00]);
        let mut timing = FixedTiming::default();
        let before = timing.clone();
        let pending = plan_instruction(CoreId::Core0, &cpu, &ram, &timing, 0)
            .expect("faulting load still plans");
        assert!(matches!(
            complete_instruction(&mut cpu, &mut ram, &mut timing, pending, 473),
            Err(CompletionError::Trap(Trap::Exception(_)))
        ));
        assert_eq!(timing, before);
    }
}
