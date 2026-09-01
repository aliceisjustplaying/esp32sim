//! Memory bus between the core and the SoC.
//!
//! Deliberately the same shape as `xtensa_lx7::bus::Bus`: the two cores are separate crates so
//! that neither chip's quirks leak into the other, and the traits are small enough that sharing
//! them would couple more than it saves. If a third core ever appears, extract `emu-bus`.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fault {
    /// no memory / device at this address
    Unmapped,
    /// address exists but this kind of access is not allowed
    Prohibited,
    /// address is not aligned for this access width
    Misaligned,
}

pub trait Bus {
    fn read8(&mut self, addr: u32) -> Result<u8, Fault>;
    fn read16(&mut self, addr: u32) -> Result<u16, Fault>;
    fn read32(&mut self, addr: u32) -> Result<u32, Fault>;
    fn write8(&mut self, addr: u32, v: u8) -> Result<(), Fault>;
    fn write16(&mut self, addr: u32, v: u16) -> Result<(), Fault>;
    fn write32(&mut self, addr: u32, v: u32) -> Result<(), Fault>;
    /// Fetch up to 4 instruction bytes at `pc`. A 16-bit instruction at the very end of a mapped
    /// region is legal, so the upper bytes may read as zero rather than faulting.
    fn fetch(&mut self, pc: u32) -> Result<[u8; 4], Fault>;
    /// Advance device time by `cycles`; returns non-zero if interrupt lines may have changed.
    fn tick(&mut self, cycles: u32) -> u32 {
        let _ = cycles;
        0
    }
    /// The highest-priority CPU interrupt line the SoC wants taken, if any (C3 INTC).
    fn pending_interrupt(&mut self) -> Option<u32> {
        None
    }
    /// The pc about to execute, for buses that attribute accesses to code.
    #[inline(always)]
    fn note_pc(&mut self, pc: u32) {
        let _ = pc;
    }
}

/// Flat RAM for unit tests.
pub struct FlatRam {
    pub base: u32,
    pub mem: Vec<u8>,
}

impl FlatRam {
    pub fn new(base: u32, size: usize) -> Self {
        FlatRam {
            base,
            mem: vec![0; size],
        }
    }
    fn off(&self, a: u32, n: usize) -> Result<usize, Fault> {
        let o = a.wrapping_sub(self.base) as usize;
        if o + n <= self.mem.len() {
            Ok(o)
        } else {
            Err(Fault::Unmapped)
        }
    }
}

impl Bus for FlatRam {
    fn read8(&mut self, a: u32) -> Result<u8, Fault> {
        let o = self.off(a, 1)?;
        Ok(self.mem[o])
    }
    fn read16(&mut self, a: u32) -> Result<u16, Fault> {
        let o = self.off(a, 2)?;
        Ok(u16::from_le_bytes(self.mem[o..o + 2].try_into().expect(
            "the checked two-byte RAM range has the required width",
        )))
    }
    fn read32(&mut self, a: u32) -> Result<u32, Fault> {
        let o = self.off(a, 4)?;
        Ok(u32::from_le_bytes(self.mem[o..o + 4].try_into().expect(
            "the checked four-byte RAM range has the required width",
        )))
    }
    fn write8(&mut self, a: u32, v: u8) -> Result<(), Fault> {
        let o = self.off(a, 1)?;
        self.mem[o] = v;
        Ok(())
    }
    fn write16(&mut self, a: u32, v: u16) -> Result<(), Fault> {
        let o = self.off(a, 2)?;
        self.mem[o..o + 2].copy_from_slice(&v.to_le_bytes());
        Ok(())
    }
    fn write32(&mut self, a: u32, v: u32) -> Result<(), Fault> {
        let o = self.off(a, 4)?;
        self.mem[o..o + 4].copy_from_slice(&v.to_le_bytes());
        Ok(())
    }
    fn fetch(&mut self, pc: u32) -> Result<[u8; 4], Fault> {
        let o = self.off(pc, 2)?;
        let mut b = [0u8; 4];
        for (i, byte) in b.iter_mut().enumerate() {
            if o + i < self.mem.len() {
                *byte = self.mem[o + i];
            }
        }
        Ok(b)
    }
}
