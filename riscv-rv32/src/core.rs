//! `emu_core::Core` for the RV32IMC: one instruction per `step`, no fast path yet (the default
//! `run` loops `step`). The interrupt input is the single line the C3's interrupt controller has
//! already arbitrated.
use crate::bus::Bus;
use crate::exec::Trap;
use crate::state::Cpu;

const X: [&str; 32] = ["zero", "ra", "sp", "gp", "tp", "t0", "t1", "t2", "s0", "s1", "a0", "a1", "a2", "a3", "a4", "a5",
                       "a6", "a7", "s2", "s3", "s4", "s5", "s6", "s7", "s8", "s9", "s10", "s11", "t3", "t4", "t5", "t6"];

impl emu_core::Core for Cpu {
    type Irq = Option<u32>;
    fn reset(&mut self) { Cpu::reset(self) }
    fn pc(&self) -> u32 { self.pc }
    fn set_pc(&mut self, pc: u32) { self.pc = pc; }
    fn waiting(&self) -> bool { self.waiting }
    fn insn_count(&self) -> u64 { self.insn_count }
    fn set_irq(&mut self, line: Option<u32>) { self.irq = line; }
    fn irq_pending(&self) -> bool { self.irq.is_some() }
    fn idle_advance(&mut self, cycles: u32) { self.insn_count += cycles as u64; }
    fn step<B: Bus>(&mut self, bus: &mut B) -> Result<(), Trap> { crate::exec::step(self, bus) }
    fn regs(&self, out: &mut Vec<(&'static str, u32)>) { for i in 1..32 { out.push((X[i], self.x[i])); } out.push(("mstatus", self.mstatus)); }
    fn arg(&self, n: usize) -> u32 { self.x[10 + n] }
    fn return_from_stub(&mut self, v: u32) { self.x[10] = v; self.pc = self.x[1]; self.insn_count += 1; }
    fn disasm(&self, pc: u32, bytes: [u8; 4]) -> String { crate::disasm::format(&crate::decode::decode(pc, bytes)).replace('\t', " ") }
}

#[cfg(test)]
mod tests {
    use emu_core::{Core, FlatRam};
    /// `addi x1, x0, 5; j .` through the trait.
    #[test]
    fn core_runs_steps() {
        let mut ram = FlatRam::new(0x4038_0000, 64);
        ram.mem[..8].copy_from_slice(&[0x93, 0x00, 0x50, 0x00, 0x6f, 0x00, 0x00, 0x00]);   // addi ra,zero,5 ; j 0
        let mut cpu = crate::Cpu::new(); cpu.pc = 0x4038_0000;
        let (used, trap) = cpu.run(&mut ram, 4);
        assert_eq!((used, trap), (4, None)); assert_eq!(cpu.x[1], 5); assert_eq!(Core::pc(&cpu), 0x4038_0004);
        let mut r = Vec::new(); cpu.regs(&mut r); assert_eq!(r[0], ("ra", 5));
    }
}
