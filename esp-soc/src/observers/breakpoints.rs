//! `--break ADDR`: stop when a pc is about to execute (not on the very first instruction).
use crate::observe::{Ctx, Observer, Wants};
use crate::soc::{Soc, Stop};
use emu_core::Core;

pub struct Breakpoints {
    pub pcs: Vec<u32>,
}
impl<S: Soc> Observer<S> for Breakpoints {
    fn name(&self) -> &'static str {
        "breakpoints"
    }
    fn wants(&self) -> Wants {
        Wants::INSN
    }
    fn on_insn(
        &mut self,
        _cx: &Ctx,
        _core: usize,
        cpu: &S::Core,
        _bus: &mut S::Bus,
        pc: u32,
    ) -> Option<Stop> {
        if self.pcs.contains(&pc) && cpu.insn_count() > 0 {
            Some(Stop::Breakpoint(pc))
        } else {
            None
        }
    }
}
