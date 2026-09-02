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
    /// One instruction at a time, but like a block: stop when the bus reports that an interrupt
    /// line may have moved, so the machine re-derives the core's input before the next instruction.
    fn run<B: Bus>(&mut self, bus: &mut B, budget: u32) -> (u32, Option<Trap>) {
        for i in 0..budget {
            if let Err(t) = crate::exec::step(self, bus) { return (i + 1, Some(t)); }
            if bus.block_break() { return (i + 1, None); }
        }
        (budget, None)
    }
    fn regs(&self, out: &mut Vec<(&'static str, u32)>) { for i in 1..32 { out.push((X[i], self.x[i])); } out.push(("mstatus", self.mstatus)); }
    fn arg(&self, n: usize) -> u32 { self.x[10 + n] }
    fn return_from_stub(&mut self, v: u32) { self.x[10] = v; self.pc = self.x[1]; self.insn_count += 1; }
    fn disasm(&self, pc: u32, bytes: [u8; 4]) -> String { crate::disasm::format(&crate::decode::decode(pc, bytes)).replace('\t', " ") }
    fn insn_len(bytes: [u8; 4]) -> u32 { crate::decode::decode(0, bytes).len as u32 }
    const TRACE_WIDTH: usize = 28;
    fn trace_regs(&self) -> String { format!("ra={:08x} sp={:08x} a0={:08x} a1={:08x}", self.x[1], self.x[2], self.x[10], self.x[11]) }
    fn regtrace_line(&self, pc: u32) -> String {
        let mut s = format!("{:08x}", pc);
        for i in 1..32 { s += &format!(" {:08x}", self.x[i]); }
        s += &format!(" {:08x}", self.mstatus);
        s
    }
    fn dump(&self, core: usize, sym: &dyn Fn(u32) -> String) -> String {
        format!("core{}: pc={:#010x} {}  mtvec={:#010x} mcause={:#010x} mepc={:#010x} mstatus={:#010x} insns={}\n", core, self.pc, sym(self.pc), self.mtvec, self.mcause, self.mepc, self.mstatus, self.insn_count)
    }
    fn has_trap_handler(&self) -> bool { self.mtvec != 0 }
    fn probe_args(&self) -> String { format!("a0={:#x} a1={:#x} a2={:#x}", self.x[10], self.x[11], self.x[12]) }
    fn return_address(&self) -> u32 { self.x[1] }
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
