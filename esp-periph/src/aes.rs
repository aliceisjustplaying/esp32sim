use crate::device::{Device, WriteEffect};
use crate::regram::RegRam;

// ------------------------------------------------------------------ AES accelerator (0x6003A000)
/// The block-mode AES accelerator. Firmware writes the key, the mode and one 16-byte block, pulses
/// TRIGGER and polls STATE until it reads idle again. Used by mbedTLS and — the reason it is here —
/// by the WPA supplicant to unwrap the group key during the four-way handshake.
pub struct Aes { pub key: [u32; 8], pub text_in: [u32; 4], pub text_out: [u32; 4], pub mode: u32, pub blocks: u64,
                 pub dma: bool, pub block_mode: u32, pub num_blocks: u32, pub iv: [u32; 4], pub state: u32,
                 pub dma_pending: bool, pub int_raw: u32, pub int_ena: u32, ram: RegRam }
impl Aes {
    pub fn new() -> Self { Aes { key: [0; 8], text_in: [0; 4], text_out: [0; 4], mode: 0, blocks: 0, dma: false, block_mode: 0,
                                num_blocks: 0, iv: [0; 4], state: 0, dma_pending: false, int_raw: 0, int_ena: 0, ram: RegRam::new() } }
    pub fn irq(&self) -> bool { self.int_raw & self.int_ena != 0 }
    /// key bytes selected by the mode register (0/1/2 = 128/192/256, +4 = decrypt)
    pub fn key_bytes(&self) -> Vec<u8> {
        let words = ((self.mode & 3) + 2) as usize * 2;
        let mut k = Vec::with_capacity(words * 4);
        for w in &self.key[..words.min(8)] { k.extend_from_slice(&w.to_le_bytes()); }
        k
    }
    pub fn decrypting(&self) -> bool { self.mode & 4 != 0 }
    pub fn read(&mut self, off: u32) -> u32 {
        match off {
            0x00..=0x1c => self.key[(off / 4) as usize],
            0x20..=0x2c => self.text_in[((off - 0x20) / 4) as usize],
            0x30..=0x3c => self.text_out[((off - 0x30) / 4) as usize],
            0x40 => self.mode,
            0x4c => self.state,                          // 0 idle, 2 done (DMA mode waits for done)
            0x50..=0x5c => self.iv[((off - 0x50) / 4) as usize],
            0x90 => self.dma as u32, 0x94 => self.block_mode, 0x98 => self.num_blocks,
            0xb0 => self.int_ena,
            _ => self.ram.read(off),
        }
    }
    pub fn write(&mut self, off: u32, v: u32) {
        match off {
            0x00..=0x1c => self.key[(off / 4) as usize] = v,
            0x20..=0x2c => self.text_in[((off - 0x20) / 4) as usize] = v,
            0x40 => self.mode = v,
            0x50..=0x5c => self.iv[((off - 0x50) / 4) as usize] = v,
            0x90 => self.dma = v & 1 != 0,
            0x94 => self.block_mode = v,
            0x98 => self.num_blocks = v,
            0xac => { if v & 1 != 0 { self.int_raw = 0; } }
            0xb0 => self.int_ena = v,
            0xb8 => { self.state = 0; self.dma_pending = false; }              // DMA_EXIT
            0x48 => if v & 1 != 0 {
                if self.dma { self.state = 1; self.dma_pending = true; }        // the bus walks the descriptors
                else { self.transform(); self.state = 0; }
            },
            _ => self.ram.write(off, v),
        }
    }
    fn transform(&mut self) {
        let decrypt = self.mode & 4 != 0;
        let key_words = ((self.mode & 3) + 2) as usize * 2;      // mode 0/1/2 -> 128/192/256 bits
        let mut key = Vec::with_capacity(key_words * 4);
        for w in &self.key[..key_words.min(8)] { key.extend_from_slice(&w.to_le_bytes()); }
        let mut block = [0u8; 16];
        for (i, w) in self.text_in.iter().enumerate() { block[4 * i..4 * i + 4].copy_from_slice(&w.to_le_bytes()); }
        let out = crate::crypto::aes_block(&key, &block, decrypt);
        for i in 0..4 { self.text_out[i] = u32::from_le_bytes([out[4 * i], out[4 * i + 1], out[4 * i + 2], out[4 * i + 3]]); }
        self.blocks += 1;
    }
}

impl Device for Aes {
    fn read(&mut self, off: u32) -> u32 { Aes::read(self, off) }
    fn write(&mut self, off: u32, v: u32) -> WriteEffect { Aes::write(self, off, v); WriteEffect::NONE }
    fn irq_sources(&self) -> u64 { self.irq() as u64 }
}
