//! UART: TX to the host console; the FIFO counters read as idle.
use crate::device::{Device, WriteEffect};
use crate::regram::RegRam;

// ------------------------------------------------------------------ UART
pub struct Uart { pub tx_out: Vec<u8>, pub int_raw: u32, pub int_ena: u32, ram: RegRam }
impl Uart {
    pub fn new() -> Self { Uart { tx_out: Vec::new(), int_raw: (1 << 1) | (1 << 14), int_ena: 0, ram: RegRam::new() } }
    pub fn read(&mut self, off: u32) -> u32 {
        match off {
            0x0 => 0,
            0x4 => self.int_raw,
            0x8 => self.int_raw & self.int_ena,
            0xc => self.int_ena,
            0x1c => 0xe000_c000,                    // STATUS: fifo counts 0, TXD/RTSN/DSRN idle levels as on silicon
            0x98 => 0,                              // REG_UPDATE (C6 and later): the driver sets it and spins until hardware clears it
            _ => self.ram.read(off),
        }
    }
    pub fn write(&mut self, off: u32, v: u32) {
        match off {
            0x0 => self.tx_out.push(v as u8),
            0xc => self.int_ena = v,
            0x10 => self.int_raw &= !v | (1 << 1) | (1 << 14),
            _ => self.ram.write(off, v),
        }
    }
    pub fn irq(&self) -> bool { self.int_raw & self.int_ena != 0 }
}
impl Default for Uart { fn default() -> Self { Self::new() } }

impl Device for Uart {
    fn read(&mut self, off: u32) -> u32 { Uart::read(self, off) }
    fn write(&mut self, off: u32, v: u32) -> WriteEffect { Uart::write(self, off, v); WriteEffect::NONE }
    fn irq_sources(&self) -> u64 { self.irq() as u64 }
}
