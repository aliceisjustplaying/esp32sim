use crate::device::{Device, WriteEffect};
use crate::regram::RegRam;

// ------------------------------------------------------------------ efuse
pub struct Efuse { pub ram: RegRam }
impl Efuse {
    pub fn new(mac: [u8; 6]) -> Self {
        let mut e = Efuse { ram: RegRam::new() };
        e.ram.write(0x44, u32::from_be_bytes([mac[2], mac[3], mac[4], mac[5]]));
        e.ram.write(0x48, ((mac[0] as u32) << 8 | mac[1] as u32) | (2 << 18));   // wafer_version_minor_lo = 2 (rev v0.2)
        e.ram.write(0x6c, 1);                                                    // blk_version_major = 1
        e.ram.write(0x1cc, 0x8c);
        e.ram.write(0x1d0, 0);
        e
    }
    pub fn read(&self, off: u32) -> u32 { self.ram.read(off) }
    pub fn write(&mut self, off: u32, v: u32) { match off { 0x1d4 => {} /* CMD: read/pgm done immediately */ _ => self.ram.write(off, v) } }
}

impl Device for Efuse {
    fn read(&mut self, off: u32) -> u32 { Efuse::read(self, off) }
    fn write(&mut self, off: u32, v: u32) -> WriteEffect { Efuse::write(self, off, v); WriteEffect::NONE }
}
