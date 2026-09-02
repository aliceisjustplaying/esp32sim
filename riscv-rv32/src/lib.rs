//! RV32IMC: the core in the ESP32-C3 (Espressif calls it a "RISC-V 32-bit single-core").
//!
//! Machine mode only, no MMU, no A/F/D extensions — which is exactly what the silicon has.
//! Interrupts are *not* the standard `mie`/`mip` external-interrupt model: the C3 routes 62
//! peripheral sources through its own interrupt matrix onto 31 CPU lines and vectors each one
//! separately (`mtvec` in vectored mode), so the SoC hands us the pending line and we take it.
pub mod bus {
    pub use emu_core::bus::*;
}
pub mod core;
pub mod decode;
pub mod disasm;
pub mod exec;
pub mod state;

pub use decode::{decode, Insn, Op};
pub use emu_core::{Bus, Core, Fault, FlatRam};
pub use exec::{step, Trap};
pub use state::Cpu;
