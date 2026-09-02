//! Hermetic tests of the RV32IMC core on a flat RAM: arithmetic and memory through a loop,
//! `step` against `Core::run`, vectored interrupts through the C3's `mtvec` mode, `ecall`,
//! `ebreak`, `mret`. Programs assembled with `riscv32-esp-elf-as -march=rv32imc_zicsr`.
use emu_core::Core;
use riscv_rv32::state::csr;
use riscv_rv32::{decode, disasm, step, Cpu, FlatRam, Trap};

const BASE: u32 = 0x4038_0000;

/// objdump prints each RISC-V instruction as one little-endian word (or half-word) in hex.
fn asm(listing: &str) -> Vec<u8> {
    let mut out = Vec::new();
    for g in listing.split_whitespace() { let w = u32::from_str_radix(g, 16).unwrap(); out.extend_from_slice(&w.to_le_bytes()[..g.len() / 2]); }
    out
}

fn machine(prog: &[u8], first: &str) -> (Cpu, FlatRam) {
    let mut ram = FlatRam::new(BASE, 64 * 1024);
    ram.mem[..prog.len()].copy_from_slice(prog);
    let mut cpu = Cpu::new(); cpu.pc = BASE;
    let mut b = [0u8; 4]; b.copy_from_slice(&ram.mem[..4]);
    assert_eq!(disasm::format(&decode(BASE, b)).replace('\t', " "), first, "the pasted program does not start with the expected instruction");
    (cpu, ram)
}

const MIX: &str = "40380537 10050513 4581 2bc00613 c10c 4114 059d 068a 00b6c733 00e51223 00455783 00f50423 00854803 98c2 fec591e3 006000ef a019 02a5 8082 a001";

#[test]
fn step_and_run_agree_and_the_loop_computes() {
    let (mut a, mut ra) = machine(&asm(MIX), "lui a0,0x40380");
    for _ in 0..1500 { step(&mut a, &mut ra).unwrap(); }
    let (mut b, mut rb) = machine(&asm(MIX), "lui a0,0x40380");
    let mut left = 1500;
    while left > 0 { let (used, t) = b.run(&mut rb, left); assert_eq!(t, None); left -= used; }
    assert_eq!(a.x, b.x); assert_eq!(a.pc, b.pc); assert_eq!(a.insn_count, b.insn_count); assert_eq!(ra.mem[0x100..0x110], rb.mem[0x100..0x110]);
    assert_eq!(a.x[5], 9, "the call after the loop ran (t0 += 9)");
    assert_eq!(a.x[11], 700, "a1 counted 100 iterations of 7");
    assert_eq!(a.pc, BASE + asm(MIX).len() as u32 - 2, "parked on the final j");
}

/// A pending line the SoC hands over is taken through `mtvec` vectored mode at `base + 4*line`,
/// with mcause/mepc/mstatus as the handler expects; `mret` returns with MIE restored.
#[test]
fn vectored_interrupt_and_mret() {
    let (mut cpu, mut ram) = machine(&asm("403802b7 20028293 0012e293 30529073 4321 30431073 30046073 4501 0505 bffd"), "lui t0,0x40380");
    let handler = asm("342023f3 34102e73 0e11 341e1073 30200073");   // csrr t2,mcause; csrr t3,mepc; addi t3,t3,4; csrw mepc,t3; mret
    let vector = 0x200 + 4 * 3;
    ram.mem[vector..vector + handler.len()].copy_from_slice(&handler);
    for _ in 0..9 { step(&mut cpu, &mut ram).unwrap(); }
    assert!(cpu.mie_enabled());
    assert!(!cpu.irq_pending());
    cpu.set_irq(Some(3));
    assert!(cpu.irq_pending());
    let pc_before = cpu.pc;
    assert_eq!(step(&mut cpu, &mut ram), Err(Trap::Interrupt(3)));
    assert_eq!(cpu.pc, BASE + vector as u32);
    assert_eq!(cpu.mcause, 0x8000_0003); assert_eq!(cpu.mepc, pc_before);
    assert!(!cpu.mie_enabled(), "MIE cleared on entry");
    cpu.set_irq(None);                                     // the handler acknowledged the source
    for _ in 0..5 { step(&mut cpu, &mut ram).unwrap(); }   // ... mret
    assert_eq!(cpu.x[7], 0x8000_0003, "the handler read mcause");
    assert_eq!(cpu.pc, pc_before + 4);
    assert!(cpu.mie_enabled(), "MIE restored by mret");
    assert_eq!(cpu.read_csr(csr::MSTATUS) & (1 << 7), 1 << 7, "MPIE set");
}

#[test]
fn ecall_and_ebreak_trap() {
    let (mut cpu, mut ram) = machine(&asm("00000073 00100073"), "ecall");
    let mut mt = FlatRam::new(0, 16); let _ = &mut mt;
    cpu.write_csr(csr::MTVEC, BASE + 0x100);
    assert_eq!(step(&mut cpu, &mut ram), Err(Trap::Exception(11)));
    assert_eq!(cpu.mcause, 11); assert_eq!(cpu.mepc, BASE); assert_eq!(cpu.pc, BASE + 0x100);
    cpu.pc = BASE + 4;
    assert_eq!(step(&mut cpu, &mut ram), Err(Trap::Ebreak(BASE + 4)));
    assert_eq!(cpu.mcause, 3);
}

#[test]
fn compressed_and_full_lengths() {
    assert_eq!(Cpu::insn_len([0x21, 0x43, 0, 0]), 2);          // c.li
    assert_eq!(Cpu::insn_len([0xb7, 0x02, 0x38, 0x40]), 4);    // lui
}
