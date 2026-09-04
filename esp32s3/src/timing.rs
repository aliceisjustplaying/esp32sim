//! Receipt-backed timing for the ESP32-S3 shared-time machine.
//!
//! This first slice prices only straight-line instructions fetched from internal SRAM. Aligned
//! 32-bit internal-SRAM loads are accepted when their result is not consumed by the next
//! instruction. Every other event is refused by name.

use crate::bus::{DRAM_HIGH, DRAM_LOW, IRAM_HIGH, IRAM_LOW};
use emu_core::{
    CostModel, ExecutionFacts, LifecycleFacts, LifecycleKind, MemoryAccess, MemoryAccessKind,
    StepKind,
};
use std::cell::RefCell;
use std::rc::Rc;
use xtensa_lx7::Op;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CostTier {
    Exact,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReceiptId {
    /// IDF 6.1 issue measurement summarized in `tests/fixtures/sram-cost-receipt.json`.
    Idf61StraightLineIssue,
    /// IDF 6.1 SRAM load measurement summarized in `tests/fixtures/sram-cost-receipt.json`.
    Idf61IndependentSramAccess,
}

impl ReceiptId {
    pub const fn file(self) -> &'static str {
        "esp32s3/tests/fixtures/sram-cost-receipt.json"
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CostClass {
    InstructionIssue,
    IndependentSramAccess,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CostComponent {
    pub class: CostClass,
    pub tier: CostTier,
    pub cycles: u32,
    pub receipt: ReceiptId,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LedgerEntry {
    pub core: usize,
    pub pc: u32,
    pub cycles: u32,
    pub components: Vec<CostComponent>,
}

#[derive(Clone, Debug, Default)]
struct State {
    ledger: Vec<LedgerEntry>,
    previous_load: [Option<u8>; 2],
}

#[derive(Clone, Debug, Default)]
pub struct Esp32S3SramCostModel {
    state: Rc<RefCell<State>>,
}

impl Esp32S3SramCostModel {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn ledger(&self) -> Vec<LedgerEntry> {
        self.state.borrow().ledger.clone()
    }
}

impl CostModel for Esp32S3SramCostModel {
    fn lifecycle(&mut self, facts: &LifecycleFacts) -> Result<(), String> {
        if facts.chip != "esp32s3" || facts.cores != 2 || facts.cpu_hz != 240_000_000 {
            return Err(format!(
                "UnsupportedChipConfig: {} with {} cores at {} Hz (unexplained tier)",
                facts.chip, facts.cores, facts.cpu_hz
            ));
        }
        let mut state = self.state.borrow_mut();
        match facts.kind {
            LifecycleKind::Attach | LifecycleKind::ChipReset => *state = State::default(),
            LifecycleKind::CoreReset(core) if core < state.previous_load.len() => {
                state.previous_load[core] = None;
            }
            LifecycleKind::CoreReset(core) => {
                return Err(format!(
                    "UnsupportedCore({core}): ESP32-S3 has two cores (unexplained tier)"
                ));
            }
        }
        Ok(())
    }

    fn cycles(&mut self, facts: &ExecutionFacts<'_>) -> Result<u32, String> {
        if facts.core >= 2 {
            return Err(format!(
                "UnsupportedCore({}): ESP32-S3 has two cores (unexplained tier)",
                facts.core
            ));
        }
        match facts.outcome.kind {
            StepKind::Retired => {}
            StepKind::Idle => return Err("Idle: cost not adopted (unexplained tier)".into()),
            StepKind::TrapBefore(trap) | StepKind::TrapDuring(trap) => {
                return Err(format!(
                    "Trap::{trap:?}: cost not adopted (unexplained tier)"
                ));
            }
        }
        if let Some(control) = facts.outcome.control {
            return Err(format!(
                "Control::{:?}: cost not adopted (unexplained tier)",
                control.kind
            ));
        }
        let bytes = facts
            .outcome
            .bytes
            .ok_or_else(|| "InstructionBytes: unavailable (unexplained tier)".to_string())?;
        if !(IRAM_LOW..IRAM_HIGH).contains(&facts.outcome.pc) {
            return Err(format!(
                "InstructionFetch({:#010x}): non-SRAM cost not adopted (unexplained tier)",
                facts.outcome.pc
            ));
        }
        let data_accesses = validate_fetch(facts, bytes)?;

        let instruction = xtensa_lx7::decode(facts.outcome.pc, bytes);
        if instruction.len != facts.outcome.length {
            return Err(format!(
                "InstructionLength({:?}): decoded {} but observed {} (unexplained tier)",
                instruction.op, instruction.len, facts.outcome.length
            ));
        }
        let sequential_pc = facts.outcome.pc.wrapping_add(u32::from(instruction.len));
        if facts.outcome.next_pc != sequential_pc {
            return Err(format!(
                "ControlFlow({:?}): cost not adopted (unexplained tier)",
                instruction.op
            ));
        }

        let (load_destination, read_registers, expects_load) =
            classify(instruction.op, instruction.s, instruction.t)?;
        let mut components = vec![CostComponent {
            class: CostClass::InstructionIssue,
            tier: CostTier::Exact,
            cycles: 1,
            receipt: ReceiptId::Idf61StraightLineIssue,
        }];
        if expects_load {
            let access = exactly_one_load(data_accesses, instruction.op)?;
            validate_sram_load(access, instruction.op)?;
            components.push(CostComponent {
                class: CostClass::IndependentSramAccess,
                tier: CostTier::Exact,
                cycles: 0,
                receipt: ReceiptId::Idf61IndependentSramAccess,
            });
        } else if !data_accesses.is_empty() {
            return Err(format!(
                "UnexpectedMemoryAccess({:?}): cost not adopted (unexplained tier)",
                instruction.op
            ));
        }

        let mut state = self.state.borrow_mut();
        if let Some(previous) = state.previous_load[facts.core] {
            if read_registers & (1 << previous) != 0 {
                return Err(format!(
                    "LoadUse({:?}): additive cost not in this receipt slice (exact tier candidate)",
                    instruction.op
                ));
            }
        }
        let cycles = components.iter().map(|component| component.cycles).sum();
        state.ledger.push(LedgerEntry {
            core: facts.core,
            pc: facts.outcome.pc,
            cycles,
            components,
        });
        state.previous_load[facts.core] = load_destination;
        Ok(cycles)
    }
}

fn validate_fetch<'a>(
    facts: &'a ExecutionFacts<'_>,
    bytes: [u8; 4],
) -> Result<&'a [MemoryAccess], String> {
    let Some(fetch) = facts.accesses.first() else {
        return Err("InstructionFetch: missing conceptual fetch (unexplained tier)".into());
    };
    if fetch.kind != MemoryAccessKind::Fetch
        || fetch.address != facts.outcome.pc
        || fetch.width != 4
        || fetch.value != u32::from_le_bytes(bytes)
        || fetch.fault.is_some()
    {
        return Err(
            "InstructionFetch: malformed or faulting conceptual fetch (unexplained tier)".into(),
        );
    }
    let data_accesses = &facts.accesses[1..];
    if data_accesses
        .iter()
        .any(|access| access.kind == MemoryAccessKind::Fetch)
    {
        return Err("InstructionFetch: multiple conceptual fetches (unexplained tier)".into());
    }
    Ok(data_accesses)
}

fn classify(op: Op, s: u8, t: u8) -> Result<(Option<u8>, u16, bool), String> {
    use Op::*;
    match op {
        L32i | L32iN => Ok((Some(t), 1 << s, true)),
        MoviN | Memw => Ok((None, 0, false)),
        Sub | Saltu => Ok((None, (1 << s) | (1 << t), false)),
        _ => Err(format!(
            "Instruction::{op:?}: cost not adopted (unexplained tier)"
        )),
    }
}

fn exactly_one_load(accesses: &[MemoryAccess], op: Op) -> Result<&MemoryAccess, String> {
    match accesses {
        [access] if access.kind == MemoryAccessKind::Read => Ok(access),
        _ => Err(format!(
            "MemoryShape({op:?}): expected one load (unexplained tier)"
        )),
    }
}

fn validate_sram_load(access: &MemoryAccess, op: Op) -> Result<(), String> {
    if access.fault.is_some() {
        return Err(format!(
            "MemoryFault({op:?}): cost not adopted (unexplained tier)"
        ));
    }
    if access.width != 4 || access.address & 3 != 0 {
        return Err(format!(
            "UnalignedOrNonWordSramLoad({op:?}): cost not adopted (unexplained tier)"
        ));
    }
    if !(DRAM_LOW..DRAM_HIGH).contains(&access.address) {
        return Err(format!(
            "DataAccess({:#010x}): non-SRAM cost not adopted (unexplained tier)",
            access.address
        ));
    }
    Ok(())
}
