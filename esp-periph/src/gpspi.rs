//! GP-SPI2/3 master. The same register block on the S3, C3 and C6.
use crate::device::{Device, WriteEffect};
use crate::regram::RegRam;

/// General-purpose SPI master as the Arduino HAL / IDF `spi_master` use it. The CPU fills
/// W0..W15 (or, with `DMA_CONF.DMA_TX_ENA`, points a GDMA out-channel at the data), sets the phase
/// enables and lengths, writes CMD.UPDATE then CMD.USR, and waits for the transfer. Transfers
/// complete instantly; the bytes that went out on MOSI are queued in `tx` for the board (the
/// display), MISO reads back as 0xFF. A DMA data phase is left in `dma_tx_pending` for the chip's
/// bus, which owns the memory the descriptors point at, to finish with `complete_dma_tx`.
pub struct GpSpi { pub regs: RegRam, pub w: [u32; 16], pub int_raw: u32, pub int_ena: u32, pub tx: Vec<u8>, pub transfers: u64, pub log: bool,
                   /// bit length of a MOSI phase that DMA must supply, until the bus completes it
                   pub dma_tx_pending: Option<u32> }
impl Default for GpSpi { fn default() -> Self { Self::new() } }

impl GpSpi {
    pub fn new() -> Self { GpSpi { regs: RegRam::new(), w: [0; 16], int_raw: 0, int_ena: 0, tx: Vec::new(), transfers: 0, log: false, dma_tx_pending: None } }
    pub fn irq(&self) -> bool { self.int_raw & self.int_ena != 0 }
    pub fn read(&self, off: u32) -> u32 {
        match off {
            0x00 => self.regs.read(0) & !((1 << 23) | (1 << 24)),      // CMD: UPDATE and USR self-clear
            0x34 => self.int_ena, 0x3c => self.int_raw, 0x40 => self.int_raw & self.int_ena,
            0x98..=0xd4 => self.w[((off - 0x98) / 4) as usize],
            0xf0 => 0x2101_0100,
            _ => self.regs.read(off),
        }
    }
    pub fn write(&mut self, off: u32, v: u32) {
        match off {
            0x00 => { self.regs.write(0, v & !((1 << 23) | (1 << 24))); if v & (1 << 24) != 0 { self.transfer(); } }
            0x34 => self.int_ena = v, 0x38 => self.int_raw &= !v,
            0x98..=0xd4 => self.w[((off - 0x98) / 4) as usize] = v,
            _ => self.regs.write(off, v),
        }
    }
    fn transfer(&mut self) {
        let user = self.regs.read(0x10); let user1 = self.regs.read(0x14); let user2 = self.regs.read(0x18);
        let start = self.tx.len();
        if user & (1 << 31) != 0 {                                        // command phase, LSB byte first
            let n = (((user2 >> 28) & 0xf) + 1).div_ceil(8); let c = user2 & 0xffff;
            for i in 0..n { self.tx.push((c >> (8 * i)) as u8); }
        }
        if user & (1 << 30) != 0 {                                        // address phase, MSB first from the top of ADDR
            let bits = (user1 >> 27) + 1; let n = bits.div_ceil(8); let a = self.regs.read(0x04);
            for i in 0..n { self.tx.push((a >> (24 - 8 * i)) as u8); }
        }
        if user & (1 << 27) != 0 {                                        // MOSI data phase from W0.. (or W8.. with HIGHPART)
            let bits = (self.regs.read(0x1c) & 0x3ffff) + 1; let n = bits.div_ceil(8) as usize;
            if self.regs.read(0x30) & (1 << 28) != 0 {                    // DMA_TX_ENA: the bus fetches the data through GDMA
                self.dma_tx_pending = Some(bits);
                if self.log { eprintln!("[spi2] transfer {} header bytes, {} data bits by DMA", self.tx.len() - start, bits); }
                return;
            }
            let base = if user & (1 << 25) != 0 { 8 } else { 0 };
            for i in 0..n.min((16 - base) * 4) { self.tx.push((self.w[base + i / 4] >> (8 * (i % 4))) as u8); }
        }
        if user & (1 << 28) != 0 {                                        // MISO: nothing answers
            let base = if user & (1 << 24) != 0 { 8 } else { 0 };
            for k in base..16 { self.w[k] = 0xffff_ffff; }
        }
        if self.log { eprintln!("[spi2] transfer {} bytes: {:02x?}", self.tx.len() - start, &self.tx[start..(start + 16).min(self.tx.len())]); }
        self.transfers += 1;
        self.int_raw |= 1 << 12;                                          // TRANS_DONE
    }
    /// The bus delivered the DMA data phase: finish the transfer.
    pub fn complete_dma_tx(&mut self, data: &[u8]) {
        self.dma_tx_pending = None;
        self.tx.extend_from_slice(data);
        if self.log { eprintln!("[spi2] dma data {} bytes: {:02x?}", data.len(), &data[..data.len().min(16)]); }
        self.transfers += 1;
        self.int_raw |= 1 << 12;                                          // TRANS_DONE
    }
}

impl Device for GpSpi {
    fn read(&mut self, off: u32) -> u32 { GpSpi::read(self, off) }
    fn write(&mut self, off: u32, v: u32) -> WriteEffect { GpSpi::write(self, off, v); WriteEffect::NONE }
    fn irq_sources(&self) -> u64 { self.irq() as u64 }
    fn debug(&mut self, on: bool) { self.log = on; }
}


