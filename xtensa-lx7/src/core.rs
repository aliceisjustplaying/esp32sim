//! `emu_core::Core` for the LX7: the machine-facing surface over `Cpu`, `step` and the block
//! interpreter. Nothing here changes behaviour; each method is the line the S3 machine used to
//! write itself.
use crate::bus::Bus;
use crate::exec::Trap;
use crate::state::{Cpu, INTTYPE_LEVEL};

const AR: [&str; 16] = ["a0", "a1", "a2", "a3", "a4", "a5", "a6", "a7", "a8", "a9", "a10", "a11", "a12", "a13", "a14", "a15"];

impl emu_core::Core for Cpu {
    /// The 32 interrupt lines after the interrupt matrix; only the level-triggered ones are the
    /// SoC's to set, the timer/software/edge bits belong to the core.
    type Irq = u32;
    fn reset(&mut self) { Cpu::reset(self) }
    fn pc(&self) -> u32 { self.pc }
    fn set_pc(&mut self, pc: u32) { self.pc = pc; }
    fn waiting(&self) -> bool { self.waiting }
    fn insn_count(&self) -> u64 { self.insn_count }
    fn set_irq(&mut self, lines: u32) { self.interrupt = (self.interrupt & !INTTYPE_LEVEL) | (lines & INTTYPE_LEVEL); }
    fn irq_pending(&self) -> bool { self.check_interrupts_pending() != 0 }
    fn idle_advance(&mut self, cycles: u32) { self.advance_ccount(cycles) }
    fn step<B: Bus>(&mut self, bus: &mut B) -> Result<(), Trap> { crate::exec::step(self, bus) }
    fn run<B: Bus>(&mut self, bus: &mut B, budget: u32) -> (u32, Option<Trap>) { crate::block::run_block(self, bus, budget) }
    fn set_boundaries(&mut self, bloom: u64) { self.boundary_bloom = bloom; }
    fn flush_caches(&mut self) { self.blocks.flush(); }
    fn regs(&self, out: &mut Vec<(&'static str, u32)>) {
        for (i, n) in AR.iter().enumerate() { out.push((n, self.get_ar(i as u8))); }
        out.push(("ps", self.ps)); out.push(("wb", self.windowbase));
    }
    fn arg(&self, n: usize) -> u32 { self.get_ar(2 + n as u8) }
    /// Synthetic return from a windowed function entry whose `entry` has not executed: a0 holds
    /// the return address with the call increment in bits 31:30; no window rotation to undo.
    fn return_from_stub(&mut self, v: u32) {
        let a0 = self.get_ar(0);
        self.set_ar(2, v);
        self.pc = (a0 & 0x3fff_ffff) | (self.pc & 0xc000_0000);
        self.insn_count += 1; self.advance_ccount(1);
    }
    fn disasm(&self, pc: u32, bytes: [u8; 4]) -> String { crate::disasm::format(&crate::decode::decode(pc, bytes)) }
}

#[cfg(test)]
mod tests {
    use emu_core::{Core, FlatRam};
    /// `movi a2, 5; j .` through the trait, on the block path and the step path.
    #[test]
    fn core_runs_a_block() {
        let mut ram = FlatRam::new(0x4037_0000, 64);
        ram.mem[..6].copy_from_slice(&[0x22, 0xa0, 0x05, 0x06, 0xff, 0xff]);   // movi a2,5 ; j -4 (to itself)
        let mut cpu = crate::Cpu::new(0);
        cpu.pc = 0x4037_0000; cpu.ps = 0;
        let (used, trap) = cpu.run(&mut ram, 8);
        assert_eq!(trap, None); assert!(used >= 2, "{}", used);
        assert_eq!(cpu.get_ar(2), 5); assert_eq!(Core::pc(&cpu), 0x4037_0003);
        let mut cpu2 = crate::Cpu::new(0); cpu2.pc = 0x4037_0000; cpu2.ps = 0;
        assert_eq!(cpu2.step(&mut ram), Ok(())); assert_eq!(cpu2.get_ar(2), 5);
        let mut r = Vec::new(); cpu2.regs(&mut r); assert_eq!(r[2], ("a2", 5));
    }
}
