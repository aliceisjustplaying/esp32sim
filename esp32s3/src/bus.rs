//! ESP32-S3 memory map: internal SRAM (512 KiB, IRAM/DRAM aliases), mask ROM, RTC
//! memories, external flash + PSRAM through the 512-entry cache MMU, peripherals.
use crate::periph::{Peripherals, PERIPH_BASE, PERIPH_END};
use crate::board::Board;
use xtensa_lx7::bus::{Bus, Fault};

pub const SRAM_SIZE: usize = 512 * 1024;
pub const IRAM_LOW: u32 = 0x4037_0000;
pub const IRAM_HIGH: u32 = 0x403E_0000;
pub const DRAM_LOW: u32 = 0x3FC8_8000;
pub const DRAM_HIGH: u32 = 0x3FD0_0000;
pub const IROM_MASK_LOW: u32 = 0x4000_0000;
pub const IROM_MASK_HIGH: u32 = 0x4006_0000;
pub const DROM_MASK_LOW: u32 = 0x3FF0_0000;
pub const DROM_MASK_HIGH: u32 = 0x3FF2_0000;
pub const RTC_FAST_LOW: u32 = 0x600F_E000;
pub const RTC_FAST_HIGH: u32 = 0x6010_0000;
pub const RTC_SLOW_LOW: u32 = 0x5000_0000;
pub const RTC_SLOW_HIGH: u32 = 0x5000_2000;
pub const DBUS_LOW: u32 = 0x3C00_0000;
pub const DBUS_HIGH: u32 = 0x3E00_0000;
pub const IBUS_LOW: u32 = 0x4200_0000;
pub const IBUS_HIGH: u32 = 0x4400_0000;
pub const MMU_TABLE: u32 = 0x600C_5000;
pub const MMU_ENTRIES: usize = 512;
pub const MMU_INVALID: u32 = 1 << 14;
pub const MMU_SPIRAM: u32 = 1 << 15;
pub const PAGE: u32 = 0x1_0000;

pub struct SocBus {
    pub sram: Vec<u8>,
    pub irom: Vec<u8>,
    pub drom: Vec<u8>,
    pub rtc_fast: Vec<u8>,
    pub rtc_slow: Vec<u8>,
    pub flash: Vec<u8>,
    pub psram: Vec<u8>,
    pub mmu: [u32; MMU_ENTRIES],
    pub periph: Peripherals,
    pub board: Board,
    pub cycles: u64,
    pub last_fault: Option<(u32, bool)>,
    /// set by any peripheral write: interrupt lines must be re-evaluated before the next instruction
    pub irq_dirty: bool,
}

impl SocBus {
    pub fn new(flash_size: usize, psram_size: usize, mac: [u8; 6]) -> Self { Self::with_sizes(flash_size, psram_size, mac) }
    pub fn with_sizes(flash_size: usize, psram_size: usize, mac: [u8; 6]) -> Self {
        SocBus {
            sram: vec![0; SRAM_SIZE], irom: vec![0; (IROM_MASK_HIGH - IROM_MASK_LOW) as usize], drom: vec![0; (DROM_MASK_HIGH - DROM_MASK_LOW) as usize],
            rtc_fast: vec![0; 8192], rtc_slow: vec![0; 8192], flash: vec![0xff; flash_size], psram: vec![0; psram_size],
            mmu: [MMU_INVALID; MMU_ENTRIES], periph: Peripherals::new(mac), board: Box::new(crate::board::Atech14::new()), cycles: 0, last_fault: None, irq_dirty: false,
        }
    }

    /// Resolve an address to (buffer, offset, writable). Cache-mapped regions go through the MMU.
    #[inline]
    fn resolve(&mut self, addr: u32) -> Option<(&mut Vec<u8>, usize, bool)> {
        match addr {
            DRAM_LOW..=0x3FCF_FFFF => Some((&mut self.sram, (addr - DRAM_LOW + 0x8000) as usize, true)),
            IRAM_LOW..=0x403D_FFFF => Some((&mut self.sram, (addr - IRAM_LOW) as usize, true)),
            IROM_MASK_LOW..=0x4005_FFFF => Some((&mut self.irom, (addr - IROM_MASK_LOW) as usize, false)),
            DROM_MASK_LOW..=0x3FF1_FFFF => Some((&mut self.drom, (addr - DROM_MASK_LOW) as usize, false)),
            RTC_FAST_LOW..=0x600F_FFFF => Some((&mut self.rtc_fast, (addr - RTC_FAST_LOW) as usize, true)),
            RTC_SLOW_LOW..=0x5000_1FFF => Some((&mut self.rtc_slow, (addr - RTC_SLOW_LOW) as usize, true)),
            DBUS_LOW..=0x3DFF_FFFF | IBUS_LOW..=0x43FF_FFFF => {
                let linear = addr & 0x1FF_FFFF;
                let entry = self.mmu[(linear >> 16) as usize];
                if entry & MMU_INVALID != 0 { return None; }
                let page = (entry & 0x3fff) as usize;
                let off = page * PAGE as usize + (linear & 0xffff) as usize;
                if entry & MMU_SPIRAM != 0 { if off < self.psram.len() { Some((&mut self.psram, off, true)) } else { None } }
                else if off < self.flash.len() { Some((&mut self.flash, off, false)) } else { None }
            }
            _ => None,
        }
    }

    #[inline]
    fn is_periph(addr: u32) -> bool { (PERIPH_BASE..PERIPH_END).contains(&addr) }

    fn periph_read(&mut self, addr: u32, size: u32) -> u32 {
        if (MMU_TABLE..MMU_TABLE + (MMU_ENTRIES as u32) * 4).contains(&addr) {
            return self.mmu[((addr - MMU_TABLE) >> 2) as usize];
        }
        let w = self.periph.read32(addr & !3);
        match size { 1 => (w >> ((addr & 3) * 8)) & 0xff, 2 => (w >> ((addr & 2) * 8)) & 0xffff, _ => w }
    }
    fn periph_write(&mut self, addr: u32, v: u32, size: u32) {
        if (MMU_TABLE..MMU_TABLE + (MMU_ENTRIES as u32) * 4).contains(&addr) {
            self.mmu[((addr - MMU_TABLE) >> 2) as usize] = v & 0xffff;
            return;
        }
        let a = addr & !3;
        let w = match size {
            4 => v,
            2 => { let old = self.periph.read32(a); let sh = (addr & 2) * 8; (old & !(0xffff << sh)) | ((v & 0xffff) << sh) }
            _ => { let old = self.periph.read32(a); let sh = (addr & 3) * 8; (old & !(0xff << sh)) | ((v & 0xff) << sh) }
        };
        self.periph.write32(a, w);
        // GPIO output registers (OUT/W1TS/W1TC/OUT1...) are hammered by bit-banged SPI and never change an
        // interrupt line directly; the periodic 32-cycle poll still sees any indirect effect
        if !(0x6000_4004..=0x6000_4018).contains(&a) { self.irq_dirty = true; }
        if self.periph.spi_exec { self.periph.spi_exec = false; self.periph.spi1.execute(&mut self.flash, &mut self.psram); }
    }

    /// Move I2S TX data out of DMA descriptors at the sample rate.
    fn dma_i2s_step(&mut self, cycles: u64) {
        self.dma_i2s_one(cycles, 0);
        self.dma_i2s_one(cycles, 1);
    }

    /// Move I2S TX data for controller `which` (0 = I2S0 on GDMA trigger 3, 1 = I2S1 on trigger 4).
    fn dma_i2s_one(&mut self, cycles: u64, which: usize) {
        let (frames, bpf) = { let i2s = if which == 0 { &mut self.periph.i2s0 } else { &mut self.periph.i2s1 }; (i2s.frames_due(cycles), i2s.bytes_per_frame as usize) };
        if frames == 0 { return; }
        let Some(ch) = self.periph.gdma.out_channel_for(if which == 0 { 3 } else { 4 }) else { return };
        let mut need = frames as usize * bpf;
        let mut samples: Vec<i16> = Vec::new();
        while need > 0 {
            let c = self.periph.gdma.out[ch];
            if !c.running || c.desc == 0 { break; }
            let dw0 = self.read32(c.desc).unwrap_or(0);
            let d = crate::periph::DmaDesc { addr: c.desc, size: dw0 & 0xfff, length: (dw0 >> 12) & 0xfff, eof: dw0 & (1 << 30) != 0, owner_dma: dw0 & (1 << 31) != 0, buf: self.read32(c.desc + 4).unwrap_or(0), next: self.read32(c.desc + 8).unwrap_or(0) };
            let remaining = d.length.saturating_sub(c.buf_pos) as usize;
            if remaining == 0 {
                // descriptor complete: hand back to software, raise EOF/DONE, advance
                let ch_ref = &mut self.periph.gdma.out[ch];
                if ch_ref.conf0 & (1 << 2) != 0 { let dw0 = self.read32(d.addr).unwrap_or(0) & !(1 << 31); let _ = self.write32(d.addr, dw0); }   // AUTO_WRBACK: owner -> cpu
                let ch_ref = &mut self.periph.gdma.out[ch];
                ch_ref.int_raw |= 1 << 0;                                                     // OUT_DONE
                if d.eof { ch_ref.int_raw |= 1 << 1; ch_ref.eof_desc = d.addr; }             // OUT_EOF
                if d.next == 0 { ch_ref.running = false; ch_ref.desc = 0; ch_ref.int_raw |= 1 << 3; break; }   // OUT_TOTAL_EOF
                ch_ref.desc = d.next; ch_ref.buf_pos = 0;
                continue;
            }
            let take = remaining.min(need);
            let start = d.buf + c.buf_pos;
            // decode 16-bit stereo frames: keep the left channel
            let mut i = 0usize;
            while i + bpf <= take {
                samples.push(self.read16(start + i as u32).unwrap_or(0) as i16);
                i += bpf;
            }
            self.periph.gdma.out[ch].buf_pos += take as u32;
            need -= take;
        }
        if !samples.is_empty() { let i2s = if which == 0 { &mut self.periph.i2s0 } else { &mut self.periph.i2s1 }; i2s.frames_out += samples.len() as u64; i2s.pcm.extend_from_slice(&samples); }
    }

    /// Camera engine: when a sensor frame is due, push it through the GDMA IN channel bound to CAM (trigger 5).
    fn dma_cam_step(&mut self, cycles: u64) {
        if !self.periph.lcd_cam.frame_due(cycles) { return; }
        let Some(ch) = self.periph.gdma.in_channel_for(5) else { self.periph.lcd_cam.dropped += 1; return };
        let Some((_w, _h, frame)) = self.board.camera_frame() else { self.periph.lcd_cam.dropped += 1; return };
        let mut pos = 0usize;
        let mut desc = self.periph.gdma.inp[ch].desc;
        let mut last = desc;
        while desc != 0 && pos < frame.len() {
            let dw0 = self.read32(desc).unwrap_or(0);
            let size = (dw0 & 0xfff) as usize; let buf = self.read32(desc + 4).unwrap_or(0); let next = self.read32(desc + 8).unwrap_or(0);
            if dw0 & (1 << 31) == 0 || size == 0 { break; }                   // descriptor not owned by DMA
            let n = size.min(frame.len() - pos);
            let mut i = 0;
            while i + 4 <= n { let v = u32::from_le_bytes([frame[pos + i], frame[pos + i + 1], frame[pos + i + 2], frame[pos + i + 3]]); let _ = self.write32(buf + i as u32, v); i += 4; }
            while i < n { let _ = self.write8(buf + i as u32, frame[pos + i]); i += 1; }
            pos += n;
            let eof = pos >= frame.len();
            let ndw0 = (dw0 & !(0xfff << 12) & !(1 << 31) & !(1 << 30)) | ((n as u32) << 12) | if eof { 1 << 30 } else { 0 };   // length, owner=cpu, suc_eof
            let _ = self.write32(desc, ndw0);
            last = desc; desc = next;
        }
        let r = &mut self.periph.gdma.inp[ch];
        r.eof_desc = last; r.desc = desc; r.int_raw |= (1 << 0) | (1 << 1);                    // IN_DONE | IN_SUC_EOF
        if desc == 0 { r.running = false; }
        self.periph.lcd_cam.int_raw |= 1 << 2;                                                  // CAM_VSYNC_INT
        self.periph.lcd_cam.frames += 1;
        self.irq_dirty = true;
    }

    pub fn load_bytes(&mut self, addr: u32, data: &[u8]) -> Result<(), String> {
        for (i, b) in data.iter().enumerate() {
            let a = addr.wrapping_add(i as u32);
            match self.resolve(a) { Some((buf, off, _)) => buf[off] = *b, None => return Err(format!("load: address {:#010x} not mapped", a)) }
        }
        Ok(())
    }
}

impl Bus for SocBus {
    fn read8(&mut self, addr: u32) -> Result<u8, Fault> {
        if Self::is_periph(addr) { return Ok(self.periph_read(addr, 1) as u8); }
        match self.resolve(addr) { Some((b, o, _)) => Ok(b[o]), None => { self.last_fault = Some((addr, false)); Err(Fault::Unmapped) } }
    }
    fn read16(&mut self, addr: u32) -> Result<u16, Fault> {
        if Self::is_periph(addr) { return Ok(self.periph_read(addr, 2) as u16); }
        match self.resolve(addr) { Some((b, o, _)) if o + 2 <= b.len() => Ok(u16::from_le_bytes([b[o], b[o + 1]])), _ => { self.last_fault = Some((addr, false)); Err(Fault::Unmapped) } }
    }
    fn read32(&mut self, addr: u32) -> Result<u32, Fault> {
        if Self::is_periph(addr) { return Ok(self.periph_read(addr, 4)); }
        match self.resolve(addr) { Some((b, o, _)) if o + 4 <= b.len() => Ok(u32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]])), _ => { self.last_fault = Some((addr, false)); Err(Fault::Unmapped) } }
    }
    fn write8(&mut self, addr: u32, v: u8) -> Result<(), Fault> {
        if Self::is_periph(addr) { self.periph_write(addr, v as u32, 1); return Ok(()); }
        match self.resolve(addr) { Some((b, o, true)) => { b[o] = v; Ok(()) } _ => { self.last_fault = Some((addr, true)); Err(Fault::Prohibited) } }
    }
    fn write16(&mut self, addr: u32, v: u16) -> Result<(), Fault> {
        if Self::is_periph(addr) { self.periph_write(addr, v as u32, 2); return Ok(()); }
        match self.resolve(addr) { Some((b, o, true)) if o + 2 <= b.len() => { b[o..o + 2].copy_from_slice(&v.to_le_bytes()); Ok(()) } _ => { self.last_fault = Some((addr, true)); Err(Fault::Prohibited) } }
    }
    fn write32(&mut self, addr: u32, v: u32) -> Result<(), Fault> {
        if Self::is_periph(addr) { self.periph_write(addr, v, 4); return Ok(()); }
        match self.resolve(addr) { Some((b, o, true)) if o + 4 <= b.len() => { b[o..o + 4].copy_from_slice(&v.to_le_bytes()); Ok(()) } _ => { self.last_fault = Some((addr, true)); Err(Fault::Prohibited) } }
    }
    fn fetch(&mut self, pc: u32) -> Result<[u8; 4], Fault> {
        match self.resolve(pc) {
            Some((b, o, _)) => { if let Some(w) = b.get(o..o + 4) { Ok([w[0], w[1], w[2], w[3]]) } else { let mut r = [0u8; 4]; for i in 0..4 { if o + i < b.len() { r[i] = b[o + i]; } } Ok(r) } }
            None => { self.last_fault = Some((pc, false)); Err(Fault::Unmapped) }
        }
    }
    fn tick(&mut self, cycles: u32) -> u32 {
        self.cycles += cycles as u64;
        self.periph.tick(cycles as u64);
        self.dma_i2s_step(cycles as u64);
        self.dma_cam_step(cycles as u64);
        if !self.periph.gpio.changes.is_empty() { let ch = std::mem::take(&mut self.periph.gpio.changes); self.board.gpio_changes(&ch); }
        if !self.periph.rmt.done.is_empty() { for (ch, bits) in std::mem::take(&mut self.periph.rmt.done) { self.board.rmt_frame(ch, &bits); } self.irq_dirty = true; }
        0
    }
}
