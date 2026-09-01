//! Measured interpreter transaction boundary.
//!
//! Planning is side-effect free. A planned instruction remains pending while
//! virtual time advances, then its guest-visible access and architectural
//! effects commit exactly once at the completion boundary.

use crate::bus::{Bus, Fault, FlatRam};
use crate::decode::{decode, Insn, Op};
use crate::exec::{commit_decoded, Trap};
use crate::state::Cpu;
use backend_api::{CostClaim, TimingBlock, VirtualCycle};
use std::collections::HashMap;

#[cfg(not(target_arch = "wasm32"))]
const MEASURED_BLOCK_CACHE_MAX_INSTRUCTIONS: usize = 1 << 20;
#[cfg(target_arch = "wasm32")]
const MEASURED_BLOCK_CACHE_MAX_INSTRUCTIONS: usize = 1 << 17;

#[derive(Clone, Debug)]
pub struct MeasuredBlockCostPayload {
    pub block_start: u32,
    pub instruction_index: u8,
    pub base_cycles: u64,
    pub base_prefix_cycles: u64,
    pub base_claims: Vec<CostClaim>,
    version_indices: [u32; 2],
    versions: [u32; 2],
}

#[derive(Clone, Default)]
pub struct MeasuredBlockCache {
    entries: HashMap<u32, MeasuredBlockCostPayload>,
}

impl MeasuredBlockCache {
    fn get_valid(&mut self, pc: u32, page_versions: &[u32]) -> Option<MeasuredBlockCostPayload> {
        let valid = self.entries.get(&pc).is_some_and(|payload| {
            payload
                .version_indices
                .iter()
                .zip(payload.versions)
                .all(|(index, version)| {
                    page_versions.get(*index as usize).copied().unwrap_or(0) == version
                })
        });
        if !valid {
            self.entries.remove(&pc);
        }
        self.entries.get(&pc).cloned()
    }

    fn insert_block(
        &mut self,
        payloads: impl IntoIterator<Item = (u32, MeasuredBlockCostPayload)>,
    ) {
        let payloads: Vec<_> = payloads.into_iter().collect();
        if self.entries.len() + payloads.len() > MEASURED_BLOCK_CACHE_MAX_INSTRUCTIONS {
            self.entries.clear();
        }
        self.entries.extend(payloads);
    }

    pub fn payload(&self, pc: u32) -> Option<&MeasuredBlockCostPayload> {
        self.entries.get(&pc)
    }
}

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

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MemoryClass {
    InternalSram,
    MaskRom,
    Flash,
    Psram,
    Rtc,
    Mmio { peripheral: String },
    Unknown,
}

#[derive(Clone, Debug)]
pub struct InstructionObservation {
    pub pc: u32,
    pub bytes: [u8; 4],
    pub instruction: Insn,
    pub fetch_memory: MemoryClass,
    pub access: Option<AccessShape>,
    pub access_memory: Option<MemoryClass>,
    pub window_overflow_pair: bool,
    pub live_window_depth: u32,
    pub loop_back_edge_residue: Option<u8>,
    pub block_base: Option<MeasuredBlockCostPayload>,
}

#[derive(Clone, Debug)]
pub struct Price {
    pub cycles: u64,
    pub claims: Vec<CostClaim>,
    pub staged_mutations: Vec<String>,
}

pub trait TimingSource {
    fn price(&self, observation: &InstructionObservation) -> Result<Price, TimingBlock>;
    fn commit(&mut self, staged_mutations: &[String]);

    fn measured_block_base(&self) -> Result<Option<(u64, Vec<CostClaim>)>, TimingBlock> {
        Ok(None)
    }
}

/// Fetch used only during planning. Implementations must not mutate guest,
/// device, cache, fault, or timing state.
pub trait MeasuredBus: Bus {
    fn measured_fetch(&self, pc: u32) -> Result<[u8; 4], Fault>;
    fn measured_memory_class(&self, address: u32) -> MemoryClass;
    fn measured_code_page(&self, pc: u32) -> u32;
}

impl MeasuredBus for FlatRam {
    fn measured_fetch(&self, pc: u32) -> Result<[u8; 4], Fault> {
        let offset = pc.wrapping_sub(self.base) as usize;
        self.mem
            .get(offset..offset.saturating_add(4))
            .and_then(|bytes| bytes.try_into().ok())
            .ok_or(Fault::Unmapped)
    }

    fn measured_memory_class(&self, address: u32) -> MemoryClass {
        if address.wrapping_sub(self.base) < self.mem.len() as u32 {
            MemoryClass::InternalSram
        } else {
            MemoryClass::Unknown
        }
    }

    fn measured_code_page(&self, _pc: u32) -> u32 {
        0
    }
}

#[derive(Clone, Debug)]
pub struct PendingInstruction {
    pub start: VirtualCycle,
    pub completion: VirtualCycle,
    pub observation: InstructionObservation,
    pub claims: Vec<CostClaim>,
    staged_mutations: Vec<String>,
}

impl PendingInstruction {
    pub fn summary(&self) -> backend_api::PendingInstructionSummary {
        backend_api::PendingInstructionSummary {
            pc: self.observation.pc,
            start: self.start,
            completion: self.completion,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PlanError {
    Timing(TimingBlock),
    Fetch { pc: u32, tier_candidate: String },
    CompletionOverflow,
}

fn access_shape(cpu: &Cpu, instruction: &Insn) -> Result<Option<AccessShape>, TimingBlock> {
    use Op::*;
    let immediate = instruction.imm as u32;
    let base_immediate = || cpu.get_ar(instruction.s).wrapping_add(immediate);
    let base_index = || {
        cpu.get_ar(instruction.s)
            .wrapping_add(cpu.get_ar(instruction.t))
    };
    let shape = match instruction.op {
        L8ui => Some((AccessKind::Load, base_immediate(), 1)),
        L16ui | L16si => Some((AccessKind::Load, base_immediate(), 2)),
        L32i | L32iN | L32ai | L32e | Lsi | Lsip => Some((AccessKind::Load, base_immediate(), 4)),
        L32r => Some((AccessKind::Load, immediate, 4)),
        Lsx | Lsxp => Some((AccessKind::Load, base_index(), 4)),
        S8i => Some((AccessKind::Store, base_immediate(), 1)),
        S16i => Some((AccessKind::Store, base_immediate(), 2)),
        S32i | S32iN | S32ri | S32e | S32nb | Ssi | Ssip => {
            Some((AccessKind::Store, base_immediate(), 4))
        }
        Ssx | Ssxp => Some((AccessKind::Store, base_index(), 4)),
        S32c1i => Some((AccessKind::Atomic, base_immediate(), 4)),
        Pie => {
            return Err(TimingBlock {
                claim_id: format!("instruction:{:08x}", cpu.pc),
                tier_candidate: "unexplained".into(),
                reason: "PIE operation has no reviewed measured access planner".into(),
            });
        }
        _ => None,
    };
    Ok(shape.map(|(kind, address, width)| AccessShape {
        kind,
        address,
        width,
    }))
}

fn predicts_window_overflow(cpu: &Cpu, instruction: &Insn) -> bool {
    let maximum = crate::exec::max_ar(instruction);
    if maximum < 4 || !cpu.woe() || cpu.excm() {
        return false;
    }
    (1..=(maximum / 4) as u32).any(|offset| {
        let window = (cpu.windowbase + offset) & 15;
        cpu.windowstart & (1 << window) != 0
    })
}

fn measured_block_payload<B: MeasuredBus, T: TimingSource>(
    cpu: &mut Cpu,
    bus: &B,
    timing: &T,
) -> Result<Option<MeasuredBlockCostPayload>, TimingBlock> {
    let Some((base_cycles, base_claims)) = timing.measured_block_base()? else {
        return Ok(None);
    };
    if let Some(payload) = cpu.measured_blocks.get_valid(cpu.pc, bus.page_versions()) {
        return Ok(Some(payload));
    }

    let pc0 = cpu.pc;
    let mut decoded = Vec::new();
    let mut pc = pc0;
    for index in 0..crate::block::MAX_LEN {
        let bytes = match bus.measured_fetch(pc) {
            Ok(bytes) => bytes,
            Err(_) if index != 0 => break,
            Err(_) => {
                return Err(TimingBlock {
                    claim_id: format!("fetch:{pc:08x}"),
                    tier_candidate: "unexplained".into(),
                    reason: "measured block cannot fetch its first instruction".into(),
                })
            }
        };
        let instruction = decode(pc, bytes);
        if index != 0
            && (crate::block::must_start_block(&instruction)
                || cpu.boundary_bloom & crate::block::pc_bit(pc) != 0)
        {
            break;
        }
        decoded.push((pc, instruction));
        pc = pc.wrapping_add(instruction.len as u32);
        if crate::block::ends_block(&instruction) {
            break;
        }
    }
    let last = decoded
        .last()
        .expect("first measured block fetch succeeded");
    let last_byte = last.0.wrapping_add(last.1.len.max(1) as u32 - 1);
    let first_version = bus.measured_code_page(pc0);
    let last_version = if last_byte >> 7 != pc0 >> 7 {
        bus.measured_code_page(last_byte)
    } else {
        first_version
    };
    let version_indices = [first_version, last_version];
    let versions = version_indices.map(|index| {
        bus.page_versions()
            .get(index as usize)
            .copied()
            .unwrap_or(0)
    });
    let mut prefix = 0u64;
    let mut payloads = Vec::with_capacity(decoded.len());
    for (index, (instruction_pc, _)) in decoded.into_iter().enumerate() {
        prefix = prefix.checked_add(base_cycles).ok_or_else(|| TimingBlock {
            claim_id: base_claims
                .first()
                .map_or_else(|| "block-base".into(), |claim| claim.id.clone()),
            tier_candidate: "exact".into(),
            reason: "measured block base-cost prefix overflows u64".into(),
        })?;
        payloads.push((
            instruction_pc,
            MeasuredBlockCostPayload {
                block_start: pc0,
                instruction_index: index as u8,
                base_cycles,
                base_prefix_cycles: prefix,
                base_claims: base_claims.clone(),
                version_indices,
                versions,
            },
        ));
    }
    cpu.measured_blocks.insert_block(payloads);
    Ok(cpu.measured_blocks.payload(pc0).cloned())
}

pub fn plan_instruction<B: MeasuredBus, T: TimingSource>(
    cpu: &mut Cpu,
    bus: &B,
    timing: &T,
    now: VirtualCycle,
) -> Result<PendingInstruction, PlanError> {
    let pc = cpu.pc;
    let bytes = bus.measured_fetch(pc).map_err(|_| PlanError::Fetch {
        pc,
        tier_candidate: "unexplained".into(),
    })?;
    let instruction = decode(pc, bytes);
    let access = access_shape(cpu, &instruction).map_err(PlanError::Timing)?;
    let loop_back_edge_residue = (cpu.lcount != 0
        && pc.wrapping_add(instruction.len as u32) == cpu.lend)
        .then_some((cpu.lbeg & 3) as u8);
    let observation = InstructionObservation {
        pc,
        bytes,
        instruction,
        fetch_memory: bus.measured_memory_class(pc),
        access_memory: access.map(|shape| bus.measured_memory_class(shape.address)),
        access,
        window_overflow_pair: predicts_window_overflow(cpu, &instruction),
        live_window_depth: cpu.windowstart.count_ones(),
        loop_back_edge_residue,
        block_base: measured_block_payload(cpu, bus, timing).map_err(PlanError::Timing)?,
    };
    let price = timing.price(&observation).map_err(PlanError::Timing)?;
    let completion = now
        .checked_add(price.cycles)
        .ok_or(PlanError::CompletionOverflow)?;
    Ok(PendingInstruction {
        start: now,
        completion,
        observation,
        claims: price.claims,
        staged_mutations: price.staged_mutations,
    })
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CompletionError {
    BeforeCompletion,
    Trap(Trap),
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
    let result = commit_decoded(cpu, bus, &pending.observation.instruction);
    timing.commit(&pending.staged_mutations);
    result.map_err(CompletionError::Trap)
}

#[cfg(test)]
mod tests {
    use super::*;
    use backend_api::test_claim;

    #[derive(Default)]
    struct FixedTiming {
        commits: usize,
    }

    impl TimingSource for FixedTiming {
        fn price(&self, _observation: &InstructionObservation) -> Result<Price, TimingBlock> {
            Ok(Price {
                cycles: 10,
                claims: vec![test_claim("fixed", 10)],
                staged_mutations: vec!["cache-state".into()],
            })
        }

        fn commit(&mut self, staged_mutations: &[String]) {
            assert_eq!(staged_mutations, ["cache-state"]);
            self.commits += 1;
        }
    }

    fn ram_with_instruction(bytes: &[u8]) -> FlatRam {
        let mut ram = FlatRam::new(0x4000_0000, 0x1000);
        let offset = 0x400;
        ram.mem[offset..offset + bytes.len()].copy_from_slice(bytes);
        ram
    }

    #[test]
    fn planning_and_partial_time_have_no_architectural_effect() {
        let mut cpu = Cpu::new(0);
        let ram = ram_with_instruction(&[0xf0, 0x20, 0x00]);
        let timing = FixedTiming::default();
        let pending = plan_instruction(&mut cpu, &ram, &timing, 7).unwrap();
        assert_eq!(pending.start, 7);
        assert_eq!(pending.completion, 17);
        assert_eq!(cpu.pc, 0x4000_0400);
        assert_eq!(cpu.insn_count, 0);
    }

    #[test]
    fn completion_commits_once_without_advancing_ccount() {
        let mut cpu = Cpu::new(0);
        let mut ram = ram_with_instruction(&[0xf0, 0x20, 0x00]);
        let mut timing = FixedTiming::default();
        let pending = plan_instruction(&mut cpu, &ram, &timing, 0).unwrap();
        assert_eq!(
            complete_instruction(&mut cpu, &mut ram, &mut timing, pending.clone(), 9),
            Err(CompletionError::BeforeCompletion)
        );
        complete_instruction(&mut cpu, &mut ram, &mut timing, pending, 10).unwrap();
        assert_eq!(cpu.pc, 0x4000_0403);
        assert_eq!(cpu.insn_count, 1);
        assert_eq!(cpu.ccount, 0);
        assert_eq!(timing.commits, 1);
    }

    #[test]
    fn load_access_is_deferred_to_completion() {
        let mut cpu = Cpu::new(0);
        cpu.set_ar(3, 0x4000_0800);
        let mut ram = ram_with_instruction(&[0x22, 0x23, 0x00]);
        ram.mem[0x800..0x804].copy_from_slice(&1u32.to_le_bytes());
        let mut timing = FixedTiming::default();
        let pending = plan_instruction(&mut cpu, &ram, &timing, 0).unwrap();
        assert_eq!(
            pending.observation.access,
            Some(AccessShape {
                kind: AccessKind::Load,
                address: 0x4000_0800,
                width: 4,
            })
        );
        ram.mem[0x800..0x804].copy_from_slice(&9u32.to_le_bytes());
        complete_instruction(&mut cpu, &mut ram, &mut timing, pending, 10).unwrap();
        assert_eq!(cpu.get_ar(2), 9);
    }

    #[test]
    fn code_write_during_pending_commits_originally_decoded_instruction() {
        let mut cpu = Cpu::new(0);
        let mut ram = ram_with_instruction(&[0xf0, 0x20, 0x00]);
        let mut timing = FixedTiming::default();
        let pending = plan_instruction(&mut cpu, &ram, &timing, 0).unwrap();
        ram.mem[0x400] = 0;
        complete_instruction(&mut cpu, &mut ram, &mut timing, pending, 10).unwrap();
        assert_eq!(cpu.pc, 0x4000_0403);
        assert_eq!(cpu.insn_count, 1);
        assert_eq!(timing.commits, 1);
    }

    #[test]
    fn trap_outcome_commits_staged_timing_once() {
        let mut cpu = Cpu::new(0);
        cpu.set_ar(3, 0x5000_0000);
        let mut ram = ram_with_instruction(&[0x22, 0x23, 0x00]);
        let mut timing = FixedTiming::default();
        let pending = plan_instruction(&mut cpu, &ram, &timing, 0).unwrap();
        assert!(matches!(
            complete_instruction(&mut cpu, &mut ram, &mut timing, pending, 10),
            Err(CompletionError::Trap(Trap::Exception(_)))
        ));
        assert_eq!(timing.commits, 1);
    }

    #[test]
    fn measured_fetch_rejects_a_trailing_partial_word() {
        let bus = FlatRam::new(0x1000, 5);
        assert_eq!(bus.measured_fetch(0x1002), Err(Fault::Unmapped));
        assert_eq!(bus.measured_fetch(0x1001), Ok([0; 4]));
    }
}
