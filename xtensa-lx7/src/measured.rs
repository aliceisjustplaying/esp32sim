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

#[derive(Clone, Debug)]
pub struct InstructionObservation {
    pub pc: u32,
    pub bytes: [u8; 4],
    pub instruction: Insn,
    pub access: Option<AccessShape>,
    pub window_overflow_pair: bool,
    pub loop_back_edge_residue: Option<u8>,
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
}

/// Fetch used only during planning. Implementations must not mutate guest,
/// device, cache, fault, or timing state.
pub trait MeasuredBus: Bus {
    fn measured_fetch(&self, pc: u32) -> Result<[u8; 4], Fault>;
}

impl MeasuredBus for FlatRam {
    fn measured_fetch(&self, pc: u32) -> Result<[u8; 4], Fault> {
        let offset = pc.wrapping_sub(self.base) as usize;
        if offset >= self.mem.len() {
            return Err(Fault::Unmapped);
        }
        let mut bytes = [0; 4];
        for (index, byte) in bytes.iter_mut().enumerate() {
            if let Some(value) = self.mem.get(offset + index) {
                *byte = *value;
            }
        }
        Ok(bytes)
    }
}

#[derive(Clone, Debug)]
pub struct PendingInstruction {
    pub start: VirtualCycle,
    pub completion: VirtualCycle,
    pub observation: InstructionObservation,
    pub claims: Vec<CostClaim>,
    staged_mutations: Vec<String>,
    terminally_blocked: bool,
}

impl PendingInstruction {
    pub fn summary(&self) -> backend_api::PendingInstructionSummary {
        backend_api::PendingInstructionSummary {
            pc: self.observation.pc,
            start: self.start,
            completion: self.completion,
        }
    }

    pub fn is_terminally_blocked(&self) -> bool {
        self.terminally_blocked
    }

    pub fn block_for_intervening_impact(&mut self) -> TimingBlock {
        self.terminally_blocked = true;
        TimingBlock {
            claim_id: format!("instruction:{:08x}", self.observation.pc),
            tier_candidate: "unexplained".into(),
            reason: "intervening event changed an unresolved access dependency".into(),
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

pub fn plan_instruction<B: MeasuredBus, T: TimingSource>(
    cpu: &Cpu,
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
        access,
        window_overflow_pair: predicts_window_overflow(cpu, &instruction),
        loop_back_edge_residue,
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
        terminally_blocked: false,
    })
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CompletionError {
    BeforeCompletion,
    TerminallyBlocked,
    InterveningCodeWrite(TimingBlock),
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
    if pending.terminally_blocked {
        return Err(CompletionError::TerminallyBlocked);
    }
    if bus.measured_fetch(pending.observation.pc).ok() != Some(pending.observation.bytes) {
        return Err(CompletionError::InterveningCodeWrite(TimingBlock {
            claim_id: format!("instruction:{:08x}", pending.observation.pc),
            tier_candidate: "unexplained".into(),
            reason: "instruction bytes changed while the instruction was pending".into(),
        }));
    }
    commit_decoded(cpu, bus, &pending.observation.instruction).map_err(CompletionError::Trap)?;
    timing.commit(&pending.staged_mutations);
    Ok(())
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
        let cpu = Cpu::new(0);
        let ram = ram_with_instruction(&[0xf0, 0x20, 0x00]);
        let timing = FixedTiming::default();
        let pending = plan_instruction(&cpu, &ram, &timing, 7).unwrap();
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
        let pending = plan_instruction(&cpu, &ram, &timing, 0).unwrap();
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
        let pending = plan_instruction(&cpu, &ram, &timing, 0).unwrap();
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
    fn code_write_during_pending_blocks_before_commit() {
        let mut cpu = Cpu::new(0);
        let mut ram = ram_with_instruction(&[0xf0, 0x20, 0x00]);
        let mut timing = FixedTiming::default();
        let pending = plan_instruction(&cpu, &ram, &timing, 0).unwrap();
        ram.mem[0x400] = 0;
        assert!(matches!(
            complete_instruction(&mut cpu, &mut ram, &mut timing, pending, 10),
            Err(CompletionError::InterveningCodeWrite(_))
        ));
        assert_eq!(cpu.insn_count, 0);
    }
}
