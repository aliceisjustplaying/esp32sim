//! The interpreter. One instruction per `step()`; 1 instruction = 1 cycle, as in the Xtensa core.

use crate::bus::{Bus, Fault};
use crate::decode::{decode, Insn, Op};
use crate::state::{exc, Cpu};

pub use emu_core::Trap;

macro_rules! ld {
    ($cpu:expr, $bus:expr, $f:ident, $addr:expr, $pc:expr) => {
        match $bus.$f($addr) {
            Ok(v) => v,
            Err(_) => {
                $cpu.trap(exc::LOAD_ACCESS_FAULT, $addr, $pc);
                return Err(Trap::Exception(exc::LOAD_ACCESS_FAULT));
            }
        }
    };
}
macro_rules! st {
    ($cpu:expr, $bus:expr, $f:ident, $addr:expr, $v:expr, $pc:expr) => {
        if $bus.$f($addr, $v).is_err() {
            $cpu.trap(exc::STORE_ACCESS_FAULT, $addr, $pc);
            return Err(Trap::Exception(exc::STORE_ACCESS_FAULT));
        }
    };
}

/// Execute one instruction.
pub fn step<B: Bus>(cpu: &mut Cpu, bus: &mut B) -> Result<(), Trap> {
    // The SoC's INTC decides enable/priority and hands us the line (`Core::set_irq`); the CPU
    // only gates on mstatus.MIE.
    if cpu.waiting || cpu.mie_enabled() {
        if let Some(line) = cpu.irq {
            cpu.waiting = false;
            if cpu.mie_enabled() {
                let pc = cpu.pc;
                cpu.insn_count += 1;
                cpu.trap(0x8000_0000 | line, 0, pc);
                return Err(Trap::Interrupt(line));
            }
        }
    }
    if cpu.waiting {
        cpu.insn_count += 1;
        return Ok(());
    }

    let pc = cpu.pc;
    bus.note_pc(pc);
    let bytes = match bus.fetch(pc) {
        Ok(b) => b,
        Err(_) => {
            cpu.insn_count += 1;
            cpu.trap(exc::INSN_ACCESS_FAULT, pc, pc);
            return Err(Trap::Exception(exc::INSN_ACCESS_FAULT));
        }
    };
    let i = decode(pc, bytes);
    cpu.insn_count += 1;
    exec_insn(cpu, bus, &i, pc)
}

#[inline]
pub fn exec_insn<B: Bus>(cpu: &mut Cpu, bus: &mut B, i: &Insn, pc: u32) -> Result<(), Trap> {
    use Op::*;
    let next = pc.wrapping_add(i.len as u32);
    let a = cpu.get(i.rs1);
    let b = cpu.get(i.rs2);
    let imm = i.imm;
    let immu = imm as u32;
    let mut new_pc = next;

    match i.op {
        Illegal => {
            cpu.trap(exc::ILLEGAL_INSN, i.raw, pc);
            return Err(Trap::Exception(exc::ILLEGAL_INSN));
        }

        Lui => cpu.set(i.rd, immu),
        Auipc => cpu.set(i.rd, pc.wrapping_add(immu)),
        Jal => {
            cpu.set(i.rd, next);
            new_pc = immu;
        }
        Jalr => {
            let t = a.wrapping_add(immu) & !1;
            cpu.set(i.rd, next);
            new_pc = t;
        }

        Beq => {
            if a == b {
                new_pc = immu
            }
        }
        Bne => {
            if a != b {
                new_pc = immu
            }
        }
        Blt => {
            if (a as i32) < (b as i32) {
                new_pc = immu
            }
        }
        Bge => {
            if (a as i32) >= (b as i32) {
                new_pc = immu
            }
        }
        Bltu => {
            if a < b {
                new_pc = immu
            }
        }
        Bgeu => {
            if a >= b {
                new_pc = immu
            }
        }

        Lb => {
            let ad = a.wrapping_add(immu);
            let v = ld!(cpu, bus, read8, ad, pc);
            cpu.set(i.rd, v as i8 as i32 as u32);
        }
        Lh => {
            let ad = a.wrapping_add(immu);
            let v = ld!(cpu, bus, read16, ad, pc);
            cpu.set(i.rd, v as i16 as i32 as u32);
        }
        Lw => {
            let ad = a.wrapping_add(immu);
            let v = ld!(cpu, bus, read32, ad, pc);
            cpu.set(i.rd, v);
        }
        Lbu => {
            let ad = a.wrapping_add(immu);
            let v = ld!(cpu, bus, read8, ad, pc);
            cpu.set(i.rd, v as u32);
        }
        Lhu => {
            let ad = a.wrapping_add(immu);
            let v = ld!(cpu, bus, read16, ad, pc);
            cpu.set(i.rd, v as u32);
        }
        Sb => {
            let ad = a.wrapping_add(immu);
            st!(cpu, bus, write8, ad, b as u8, pc);
        }
        Sh => {
            let ad = a.wrapping_add(immu);
            st!(cpu, bus, write16, ad, b as u16, pc);
        }
        Sw => {
            let ad = a.wrapping_add(immu);
            st!(cpu, bus, write32, ad, b, pc);
        }

        Addi => cpu.set(i.rd, a.wrapping_add(immu)),
        Slti => cpu.set(i.rd, ((a as i32) < imm) as u32),
        Sltiu => cpu.set(i.rd, (a < immu) as u32),
        Xori => cpu.set(i.rd, a ^ immu),
        Ori => cpu.set(i.rd, a | immu),
        Andi => cpu.set(i.rd, a & immu),
        Slli => cpu.set(i.rd, a << (immu & 31)),
        Srli => cpu.set(i.rd, a >> (immu & 31)),
        Srai => cpu.set(i.rd, ((a as i32) >> (immu & 31)) as u32),

        Add => cpu.set(i.rd, a.wrapping_add(b)),
        Sub => cpu.set(i.rd, a.wrapping_sub(b)),
        Sll => cpu.set(i.rd, a << (b & 31)),
        Slt => cpu.set(i.rd, (((a as i32) < (b as i32)) as u32)),
        Sltu => cpu.set(i.rd, (a < b) as u32),
        Xor => cpu.set(i.rd, a ^ b),
        Srl => cpu.set(i.rd, a >> (b & 31)),
        Sra => cpu.set(i.rd, ((a as i32) >> (b & 31)) as u32),
        Or => cpu.set(i.rd, a | b),
        And => cpu.set(i.rd, a & b),

        Mul => cpu.set(i.rd, a.wrapping_mul(b)),
        Mulh => cpu.set(i.rd, (((a as i32 as i64) * (b as i32 as i64)) >> 32) as u32),
        Mulhsu => cpu.set(i.rd, (((a as i32 as i64) * (b as u64 as i64)) >> 32) as u32),
        Mulhu => cpu.set(i.rd, (((a as u64) * (b as u64)) >> 32) as u32),
        // RISC-V defines division by zero and the signed overflow case, rather than trapping
        Div => cpu.set(
            i.rd,
            if b == 0 {
                u32::MAX
            } else if a == 0x8000_0000 && b == u32::MAX {
                a
            } else {
                ((a as i32).wrapping_div(b as i32)) as u32
            },
        ),
        Divu => cpu.set(i.rd, if b == 0 { u32::MAX } else { a / b }),
        Rem => cpu.set(
            i.rd,
            if b == 0 {
                a
            } else if a == 0x8000_0000 && b == u32::MAX {
                0
            } else {
                ((a as i32).wrapping_rem(b as i32)) as u32
            },
        ),
        Remu => cpu.set(i.rd, if b == 0 { a } else { a % b }),

        Fence | FenceI => {}

        Ecall => {
            cpu.trap(exc::ECALL_M, 0, pc);
            return Err(Trap::Exception(exc::ECALL_M));
        }
        Ebreak => {
            cpu.trap(exc::BREAKPOINT, pc, pc);
            return Err(Trap::Ebreak(pc));
        }
        Mret => {
            cpu.mret();
            return Ok(());
        }
        Wfi => {
            cpu.waiting = true;
        }

        Csrrw | Csrrs | Csrrc | Csrrwi | Csrrsi | Csrrci => {
            let n = immu;
            let src = if matches!(i.op, Csrrwi | Csrrsi | Csrrci) {
                i.rs1 as u32
            } else {
                a
            };
            // A read is only skipped for csrrw with rd = x0; a write only for set/clear with a
            // zero source operand — which is how firmware reads a CSR without disturbing it.
            let old = if matches!(i.op, Csrrw | Csrrwi) && i.rd == 0 {
                0
            } else {
                cpu.read_csr(n)
            };
            let write = match i.op {
                Csrrw | Csrrwi => Some(src),
                Csrrs | Csrrsi => {
                    if i.rs1 != 0 {
                        Some(old | src)
                    } else {
                        None
                    }
                }
                _ => {
                    if i.rs1 != 0 {
                        Some(old & !src)
                    } else {
                        None
                    }
                }
            };
            if let Some(v) = write {
                cpu.write_csr(n, v);
            }
            cpu.set(i.rd, old);
        }
    }
    cpu.pc = new_pc;
    Ok(())
}

/// So the SoC can report a bus error without knowing the trap encoding.
pub fn fault_cause(f: Fault, store: bool) -> u32 {
    match f {
        Fault::Misaligned => {
            if store {
                exc::STORE_MISALIGNED
            } else {
                exc::LOAD_MISALIGNED
            }
        }
        _ => {
            if store {
                exc::STORE_ACCESS_FAULT
            } else {
                exc::LOAD_ACCESS_FAULT
            }
        }
    }
}
