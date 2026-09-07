//! Rough memory service layered over the whole-firmware instruction model.
//! MMU writes are observed from firmware; use ROM boot, not synthetic app boot.
use crate::{bus::*, rough_memory::*, ApproximateCostModel};
use emu_core::{CostModel, ExecutionFacts, LifecycleFacts, LifecycleKind, MemoryAccessKind};
use std::{cell::RefCell, rc::Rc};

#[derive(Clone, Debug)]
pub struct MemoryCostModel {
    pub cpu: ApproximateCostModel,
    pub memory: Rc<RefCell<MemoryTiming>>,
    mmu: [u32; MMU_ENTRIES],
}
impl MemoryCostModel {
    pub fn new(cpu: ApproximateCostModel, config: MemoryConfig) -> Self {
        Self { cpu, memory: Rc::new(RefCell::new(MemoryTiming::new(config))),
            mmu: [MMU_INVALID; MMU_ENTRIES] }
    }
}
impl CostModel for MemoryCostModel {
    fn lifecycle(&mut self, facts: &LifecycleFacts) -> Result<(), String> {
        self.cpu.lifecycle(facts)?;
        if matches!(facts.kind, LifecycleKind::Attach | LifecycleKind::ChipReset) {
            self.mmu.fill(MMU_INVALID);
            self.memory.borrow_mut().reset();
        }
        Ok(())
    }
    fn cycles(&mut self, _: &ExecutionFacts<'_>) -> Result<u32, String> {
        Err("memory contention requires shared time: call cycles_at".into())
    }
    fn cycles_at(&mut self, facts: &ExecutionFacts<'_>, now: u64) -> Result<u32, String> {
        let cpu_cycles = self.cpu.cycles_at(facts, now)?;
        let mut cursor = now;
        let mut memory = self.memory.borrow_mut();
        for access in facts.accesses {
            if access.fault.is_some() { continue; }
            if access.kind == MemoryAccessKind::Write && access.width == 4 &&
                (MMU_TABLE..MMU_TABLE + MMU_ENTRIES as u32 * 4).contains(&access.address) {
                self.mmu[((access.address - MMU_TABLE) >> 2) as usize] = access.value & 0xffff;
            }
            if let Some(kind) = memory_kind(access.address, &self.mmu) {
                cursor = memory.access(cursor, kind, u32::from(access.width)).finish;
            }
        }
        Ok(u64::from(cpu_cycles).saturating_add(cursor - now).min(u64::from(u32::MAX)) as u32)
    }
}
