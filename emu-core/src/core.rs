//! A CPU core as the machine sees it. The machine schedules cores, delivers interrupt state,
//! counts traps and drives tracing through this trait; everything architectural (register
//! windows, CSRs, vectors) stays inside the core crate.
use crate::bus::Bus;

/// Why `step`/`run` returned early. Architectural traps have already been taken (the pc points
/// at the handler); the emulator-level ones are reported so the machine can stop.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Trap {
    /// an architectural exception was taken (vectored); emulation continues normally
    Exception(u32),
    /// an interrupt was taken (Xtensa interrupt number / RISC-V line)
    Interrupt(u32),
    /// instruction not implemented by the emulator (pc, raw word)
    Unimplemented(u32, u32),
    /// `simcall` — Xtensa semihosting request
    Simcall,
    /// `ebreak` — a panic, an assert, or a debugger breakpoint in a RISC-V guest
    Ebreak(u32),
}

pub trait Core {
    /// Interrupt input as the SoC computes it: the Xtensa takes its 32 level lines as a mask,
    /// the RISC-V takes the one line its interrupt controller has arbitrated.
    type Irq: Copy + PartialEq + Default;

    fn reset(&mut self);
    fn pc(&self) -> u32;
    fn set_pc(&mut self, pc: u32);
    /// Halted by `waiti`/`wfi` until an interrupt arrives.
    fn waiting(&self) -> bool;
    fn insn_count(&self) -> u64;
    /// Present the SoC's interrupt state. Called after the machine re-derives it; must be cheap.
    fn set_irq(&mut self, irq: Self::Irq);
    /// An interrupt could be taken now (the idle skip is only allowed when this is false).
    fn irq_pending(&self) -> bool;
    /// Let cycles pass without executing anything (the core is in `waiti`/`wfi`): advances
    /// whatever counter the core keeps and raises its own timer interrupts if they fall due.
    fn idle_advance(&mut self, cycles: u32);
    /// Execute one instruction. `Ok` when it completed normally.
    fn step<B: Bus>(&mut self, bus: &mut B) -> Result<(), Trap>;
    /// Execute up to `budget` instructions the fast way (blocks, JIT). Returns the iterations a
    /// loop over `step` would have consumed — executed instructions, plus one for a trap taken
    /// before an instruction ran — and the trap that ended the run, if any.
    fn run<B: Bus>(&mut self, bus: &mut B, budget: u32) -> (u32, Option<Trap>) {
        for i in 0..budget { if let Err(t) = self.step(bus) { return (i + 1, Some(t)); } }
        (budget, None)
    }
    /// pcs the machine intercepts (stubs, probes) as a bloom over `pc_bit`: a fast path must
    /// stop at every one of them so the machine can look.
    fn set_boundaries(&mut self, _bloom: u64) {}
    /// Throw away decoded/compiled code (after loading an image or changing boundaries).
    fn flush_caches(&mut self) {}
    /// Registers worth printing in a trace line, in the core's conventional order.
    fn regs(&self, out: &mut Vec<(&'static str, u32)>);
    /// Argument `n` of the function about to be entered, per the core's calling convention
    /// (Xtensa windowed: a2 + n; RISC-V: a0 + n). For function probes and stubs.
    fn arg(&self, n: usize) -> u32;
    /// Return from the function about to be entered with `v`, as if it ran: the stub mechanism.
    fn return_from_stub(&mut self, v: u32);
    /// Disassemble the instruction bytes at `pc` for a trace line.
    fn disasm(&self, pc: u32, bytes: [u8; 4]) -> String;
}

/// Bloom bit for a pc; the machine's stub/probe tables and the cores' block boundaries agree on it.
#[inline(always)]
pub fn pc_bit(pc: u32) -> u64 { 1u64 << ((pc >> 2) & 63) }
