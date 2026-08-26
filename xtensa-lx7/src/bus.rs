//! Memory bus abstraction between the core and the SoC.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fault {
    /// no memory / device at this address
    Unmapped,
    /// address exists but access of this kind is not allowed (e.g. exec from data-only)
    Prohibited,
}

pub trait Bus {
    fn read8(&mut self, addr: u32) -> Result<u8, Fault>;
    fn read16(&mut self, addr: u32) -> Result<u16, Fault>;
    fn read32(&mut self, addr: u32) -> Result<u32, Fault>;
    fn write8(&mut self, addr: u32, v: u8) -> Result<(), Fault>;
    fn write16(&mut self, addr: u32, v: u16) -> Result<(), Fault>;
    fn write32(&mut self, addr: u32, v: u32) -> Result<(), Fault>;
    /// Fetch up to 4 instruction bytes at `pc` (any alignment). Bytes past the
    /// end of a mapped region may be zero.
    fn fetch(&mut self, pc: u32) -> Result<[u8; 4], Fault>;
    /// Write-version counters, one per page of guest memory: `page_versions()[code_page(pc)]`
    /// changes whenever a write could have altered an instruction at `pc`. A decode-cache entry
    /// remembers its page index and the version it saw, so the common path is one indexed load
    /// instead of re-fetching and comparing the bytes; `code_page` is only called on a miss.
    fn page_versions(&self) -> &[u32] { &[] }
    fn code_page(&mut self, pc: u32) -> u32 { let _ = pc; 0 }
    /// Called after every executed instruction with the cycle estimate; lets the
    /// SoC advance timers and DMA. Return pending external level-interrupt lines.
    fn tick(&mut self, cycles: u32) -> u32 { let _ = cycles; 0 }
}

/// Simple flat RAM for unit tests.
pub struct FlatRam {
    pub base: u32,
    pub mem: Vec<u8>,
    /// bumped on every write: coarse, but this RAM only backs unit tests
    pub ver: u32,
}

impl FlatRam {
    pub fn new(base: u32, size: usize) -> Self { FlatRam { base, mem: vec![0; size], ver: 0 } }
    fn off(&self, addr: u32, n: usize) -> Result<usize, Fault> {
        let o = addr.wrapping_sub(self.base) as usize;
        if o + n <= self.mem.len() { Ok(o) } else { Err(Fault::Unmapped) }
    }
}

impl Bus for FlatRam {
    fn read8(&mut self, a: u32) -> Result<u8, Fault> { let o = self.off(a, 1)?; Ok(self.mem[o]) }
    fn read16(&mut self, a: u32) -> Result<u16, Fault> { let o = self.off(a, 2)?; Ok(u16::from_le_bytes([self.mem[o], self.mem[o + 1]])) }
    fn read32(&mut self, a: u32) -> Result<u32, Fault> { let o = self.off(a, 4)?; Ok(u32::from_le_bytes(self.mem[o..o + 4].try_into().unwrap())) }
    fn write8(&mut self, a: u32, v: u8) -> Result<(), Fault> { let o = self.off(a, 1)?; self.mem[o] = v; self.ver += 1; Ok(()) }
    fn write16(&mut self, a: u32, v: u16) -> Result<(), Fault> { let o = self.off(a, 2)?; self.mem[o..o + 2].copy_from_slice(&v.to_le_bytes()); self.ver += 1; Ok(()) }
    fn write32(&mut self, a: u32, v: u32) -> Result<(), Fault> { let o = self.off(a, 4)?; self.mem[o..o + 4].copy_from_slice(&v.to_le_bytes()); self.ver += 1; Ok(()) }
    fn page_versions(&self) -> &[u32] { std::slice::from_ref(&self.ver) }
    fn fetch(&mut self, pc: u32) -> Result<[u8; 4], Fault> {
        let o = self.off(pc, 1)?;
        let mut b = [0u8; 4];
        for i in 0..4 { if o + i < self.mem.len() { b[i] = self.mem[o + i]; } }
        Ok(b)
    }
}
