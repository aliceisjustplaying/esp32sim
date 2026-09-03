//! ESP32-S3 memory map: internal SRAM (512 KiB, IRAM/DRAM aliases), mask ROM, RTC
//! memories, external flash + PSRAM through the 512-entry cache MMU, peripherals.
use crate::periph::{Peripherals, PERIPH_BASE, PERIPH_END};
use crate::board::Board;
use std::collections::HashSet;
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

const SPI2_DMA_DESCRIPTOR_STEP_BUDGET: usize = 1024;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DmaDescriptorWord {
    Control,
    Buffer,
    Next,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DmaDescriptorFault {
    Read { descriptor: u32, word: DmaDescriptorWord, fault: Fault },
    BufferRead { descriptor: u32, address: u32, fault: Fault },
    Writeback { descriptor: u32, fault: Fault },
    NotOwned { descriptor: u32 },
    LengthExceedsSize { descriptor: u32, size: u32, length: u32 },
    Cycle { descriptor: u32 },
    StepBudgetExceeded { budget: usize },
    PayloadTooShort { expected: usize, actual: usize },
}

struct Spi2DmaCompletion {
    channel: usize,
    final_channel: crate::periph::GdmaOutCh,
    descriptor_writebacks: Vec<(u32, u32)>,
    payload: Vec<u8>,
}

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
    pub spi2_dma_fault: Option<DmaDescriptorFault>,
    /// set by any peripheral write: interrupt lines must be re-evaluated before the next instruction
    pub irq_dirty: bool,
    /// GPIO edges for observers, while one wants them: (cycle, pin, level)
    pub gpio_events: Option<Vec<(u64, u8, bool)>>,
    pub debug: esp_soc::DebugFlags,
    /// Software TLB: the last resolved mapping per 64 KiB page, so loads, stores and fetches skip
    /// the address-range walk and the flash MMU. Cleared whenever the MMU changes.
    tlb: Vec<TlbEntry>,
    /// One version counter per `VPAGE`-byte page of every buffer, bumped by each write there. The
    /// decode and block caches store the versions they were built under, so a stale decode can
    /// never run. 256 bytes rather than 4 KiB because IRAM and DRAM are one SRAM: on the S3 the
    /// app's `.dram0.data` begins in the same 4 KiB as the end of IRAM text, and every global
    /// write was invalidating `_xt_context_save`.
    page_ver: Vec<u32>,
    /// first `page_ver` index of each buffer, by `SRC_*`
    ver_base: [u32; 7],
    /// Device time is advanced lazily: cycles accumulate here and the devices see them in one
    /// batch when a timer is due, a peripheral register is accessed, or MAX_TICK_DEFER cycles
    /// have passed — so guest-visible time is exact while idle rounds cost nothing.
    tick_pending: u32, tick_budget: u32,
}

/// Longest stretch of cycles device models may go without seeing time advance. Bounds the
/// latency of everything that has no computed deadline (DMA, USB, LCD, WiFi).
const MAX_TICK_DEFER: u32 = 256;

/// Buffer identifiers for resolved addresses.
pub const SRC_SRAM: u8 = 0; pub const SRC_IROM: u8 = 1; pub const SRC_FLASH: u8 = 2; pub const SRC_PSRAM: u8 = 3;
pub const SRC_DROM: u8 = 4; pub const SRC_RTC_FAST: u8 = 5; pub const SRC_RTC_SLOW: u8 = 6;
const TLB_SIZE: usize = xtensa_lx7::bus::TLB_ENTRIES;
/// Granularity of the write-version counters. Must exceed the longest block (`block::MAX_LEN` × 3 bytes).
const VPAGE_SHIFT: usize = xtensa_lx7::bus::VPAGE_SHIFT as usize;
const VPAGE_MASK: usize = (1 << VPAGE_SHIFT) - 1;
use xtensa_lx7::bus::{FastMem, TlbEntry};
#[inline(always)]
fn tlb_idx(addr: u32) -> usize { xtensa_lx7::bus::tlb_index(addr) }

impl SocBus {
    pub fn new(flash_size: usize, psram_size: usize, mac: [u8; 6]) -> Self { Self::with_sizes(flash_size, psram_size, mac) }
    pub fn with_sizes(flash_size: usize, psram_size: usize, mac: [u8; 6]) -> Self {
        let bus_uninit = SocBus {
            sram: vec![0; SRAM_SIZE], irom: vec![0; (IROM_MASK_HIGH - IROM_MASK_LOW) as usize], drom: vec![0; (DROM_MASK_HIGH - DROM_MASK_LOW) as usize],
            rtc_fast: vec![0; 8192], rtc_slow: vec![0; 8192], flash: vec![0xff; flash_size], psram: vec![0; psram_size],
            mmu: [MMU_INVALID; MMU_ENTRIES], periph: Peripherals::new(mac), board: Box::new(crate::board::Atech14::new()), cycles: 0, last_fault: None, spi2_dma_fault: None, irq_dirty: false, gpio_events: None, debug: Default::default(),
            tlb: vec![TlbEntry::EMPTY; TLB_SIZE], page_ver: Vec::new(), ver_base: [0; 7], tick_pending: 0, tick_budget: 0,
        };
        let mut b = bus_uninit;
        b.rebuild_page_table();
        b
    }

    /// Attach fresh peripheral-side devices and restore the levels driven by the persistent board.
    pub fn attach_board_devices(&mut self) {
        for (bus, address, device) in self.board.i2c_devices() {
            self.periph.i2c[bus as usize].attach(address, device);
        }
        for (pin, level) in self.board.input_levels() {
            self.periph.gpio.set_input(pin, level);
        }
    }

    /// Size the per-page version table to the buffers. Call after replacing `flash` or `psram`.
    pub fn rebuild_page_table(&mut self) {
        let sizes = [self.sram.len(), self.irom.len(), self.flash.len(), self.psram.len(), self.drom.len(), self.rtc_fast.len(), self.rtc_slow.len()];
        let mut base = 0u32;
        for (i, n) in sizes.iter().enumerate() { self.ver_base[i] = base; base += ((n + VPAGE_MASK) >> VPAGE_SHIFT) as u32; }
        self.page_ver = vec![0; base as usize + 1];
        self.invalidate_tlb();
    }

    /// Forget every cached mapping. Anything that re-points the flash MMU must call this.
    /// A remap changes which bytes a cache-window pc refers to without any write happening, so
    /// the flash and PSRAM page versions are bumped too: that is what invalidates decoded
    /// instructions and blocks that were built through the old mapping.
    pub fn invalidate_tlb(&mut self) {
        for e in self.tlb.iter_mut() { *e = TlbEntry::EMPTY; }
        let (a, b) = (self.ver_base[SRC_FLASH as usize] as usize, self.ver_base[SRC_DROM as usize] as usize);
        for v in &mut self.page_ver[a..b] { *v = v.wrapping_add(1); }          // flash then psram
    }

    #[inline(always)]
    fn buf(&self, src: u8) -> &Vec<u8> {
        match src { SRC_SRAM => &self.sram, SRC_IROM => &self.irom, SRC_FLASH => &self.flash, SRC_PSRAM => &self.psram,
                    SRC_DROM => &self.drom, SRC_RTC_FAST => &self.rtc_fast, _ => &self.rtc_slow }
    }
    #[inline(always)]
    fn buf_mut(&mut self, src: u8) -> &mut Vec<u8> {
        match src { SRC_SRAM => &mut self.sram, SRC_IROM => &mut self.irom, SRC_FLASH => &mut self.flash, SRC_PSRAM => &mut self.psram,
                    SRC_DROM => &mut self.drom, SRC_RTC_FAST => &mut self.rtc_fast, _ => &mut self.rtc_slow }
    }

    /// The mapping covering `addr`, from the TLB or by walking the address map.
    #[inline(always)]
    fn lookup(&mut self, addr: u32) -> Option<TlbEntry> {
        let e = self.tlb[tlb_idx(addr)];
        if addr >= e.lo && addr < e.hi { Some(e) } else { self.tlb_fill(addr) }
    }

    /// Walk the address map for the 64 KiB page holding `addr` and remember it.
    fn tlb_fill(&mut self, addr: u32) -> Option<TlbEntry> {
        let page = addr & !0xffff;
        let region = |lo: u32, hi: u32, src: u8, w: bool| -> TlbEntry {
            let lo_ = page.max(lo); let hi_ = (page + 0x10000).min(hi);
            TlbEntry { lo: lo_, hi: hi_, base: std::ptr::null_mut(), off: lo_ - lo, vbase: 0, src: src as u32, writable: w as u32 }
        };
        let mut e = match addr {
            DRAM_LOW..=0x3FCF_FFFF => { let mut e = region(DRAM_LOW, 0x3FD0_0000, SRC_SRAM, true); e.off += 0x8000; e }
            IRAM_LOW..=0x403D_FFFF => region(IRAM_LOW, 0x403E_0000, SRC_SRAM, true),
            IROM_MASK_LOW..=0x4005_FFFF => region(IROM_MASK_LOW, 0x4006_0000, SRC_IROM, false),
            DROM_MASK_LOW..=0x3FF1_FFFF => region(DROM_MASK_LOW, 0x3FF2_0000, SRC_DROM, false),
            RTC_FAST_LOW..=0x600F_FFFF => region(RTC_FAST_LOW, 0x6010_0000, SRC_RTC_FAST, true),
            RTC_SLOW_LOW..=0x5000_1FFF => region(RTC_SLOW_LOW, 0x5000_2000, SRC_RTC_SLOW, true),
            DBUS_LOW..=0x3DFF_FFFF | IBUS_LOW..=0x43FF_FFFF => {
                let linear = addr & 0x1FF_FFFF;
                let entry = self.mmu[(linear >> 16) as usize];
                if entry & MMU_INVALID != 0 { return None; }
                let off = (entry & 0x3fff) as usize * PAGE as usize;
                let (src, w) = if entry & MMU_SPIRAM != 0 { (SRC_PSRAM, true) } else { (SRC_FLASH, false) };
                if off + PAGE as usize > self.buf(src).len() { return None; }
                TlbEntry { lo: page, hi: page + 0x10000, base: std::ptr::null_mut(), off: off as u32, vbase: 0, src: src as u32, writable: w as u32 }
            }
            _ => return None,
        };
        e.vbase = self.ver_base[e.src as usize] + (e.off as usize >> VPAGE_SHIFT) as u32;
        let off = e.off as usize;
        e.base = unsafe { self.buf_mut(e.src as u8).as_mut_ptr().add(off) };
        self.tlb[tlb_idx(addr)] = e;
        Some(e)
    }

    /// Record that `len` bytes at `off` of the page group starting at `vbase` changed. An
    /// instruction can begin up to two bytes before a page boundary, so the previous page is
    /// bumped too when the write touches the first bytes of one.
    #[inline(always)]
    fn bump(&mut self, vbase: u32, off: usize, len: usize) {
        let p = vbase as usize + (off >> VPAGE_SHIFT);
        self.page_ver[p] = self.page_ver[p].wrapping_add(1);
        let last = vbase as usize + ((off + len - 1) >> VPAGE_SHIFT);
        if last != p { self.page_ver[last] = self.page_ver[last].wrapping_add(1); }
        if off & VPAGE_MASK < 3 && p > 0 { self.page_ver[p - 1] = self.page_ver[p - 1].wrapping_add(1); }
    }

    /// Record a write done behind the bus's back (image loaders, the SPI flash controller).
    pub fn note_written(&mut self, src: u8, off: usize, len: usize) {
        if len == 0 { return; }
        let vbase = self.ver_base[src as usize];
        let (first, last) = (off >> VPAGE_SHIFT, (off + len - 1) >> VPAGE_SHIFT);
        for p in first..=last { let i = vbase as usize + p; if i < self.page_ver.len() { self.page_ver[i] = self.page_ver[i].wrapping_add(1); } }
        if off & VPAGE_MASK < 3 && first > 0 { let i = vbase as usize + first - 1; self.page_ver[i] = self.page_ver[i].wrapping_add(1); }
    }

    #[inline]
    fn is_periph(addr: u32) -> bool { (PERIPH_BASE..PERIPH_END).contains(&addr) }

    fn periph_read(&mut self, addr: u32, size: u32) -> u32 {
        if (MMU_TABLE..MMU_TABLE + (MMU_ENTRIES as u32) * 4).contains(&addr) {
            return self.mmu[((addr - MMU_TABLE) >> 2) as usize];
        }
        self.flush_ticks();                                         // registers must show exact time
        let w = self.periph.read32(addr & !3);
        match size { 1 => (w >> ((addr & 3) * 8)) & 0xff, 2 => (w >> ((addr & 2) * 8)) & 0xffff, _ => w }
    }
    fn periph_write(&mut self, addr: u32, v: u32, size: u32) {
        self.periph_write_inner(addr, v, size);
        self.refresh_tick_budget();   // the write may have armed something
    }
    fn periph_write_inner(&mut self, addr: u32, v: u32, size: u32) {
        if (MMU_TABLE..MMU_TABLE + (MMU_ENTRIES as u32) * 4).contains(&addr) {
            let i = ((addr - MMU_TABLE) >> 2) as usize;
            if self.mmu[i] != v & 0xffff { self.mmu[i] = v & 0xffff; self.invalidate_tlb(); }
            return;
        }
        self.flush_ticks();
        let a = addr & !3;
        let w = match size {
            4 => v,
            2 => { let old = self.periph.read32(a); let sh = (addr & 2) * 8; (old & !(0xffff << sh)) | ((v & 0xffff) << sh) }
            _ => { let old = self.periph.read32(a); let sh = (addr & 3) * 8; (old & !(0xff << sh)) | ((v & 0xff) << sh) }
        };
        self.periph.write32(a, w);
        self.complete_spi2_dma();
        self.deliver_spi2_transfer();
        // GPIO output registers (OUT/W1TS/W1TC/OUT1...) are hammered by bit-banged SPI and never change an
        // interrupt line directly; the periodic 32-cycle poll still sees any indirect effect
        if !(0x6000_4004..=0x6000_4018).contains(&a) { self.irq_dirty = true; }
        if self.periph.spi_exec {
            self.periph.spi_exec = false;
            self.periph.spi1.execute(&mut self.flash, &mut self.psram);
            for (m, off, len) in std::mem::take(&mut self.periph.spi1.dirty) { self.note_written(match m { crate::periph::DirtyMem::Flash => SRC_FLASH, crate::periph::DirtyMem::Psram => SRC_PSRAM }, off, len); }
        }
    }

    fn complete_spi2_dma(&mut self) {
        if self.periph.spi2.dma_tx_pending.is_none() {
            return;
        }
        match self.spi2_dma_completion() {
            Ok(Some(completion)) => {
                for (descriptor, control) in completion.descriptor_writebacks {
                    if let Err(fault) = self.write32(descriptor, control) {
                        self.spi2_dma_fault = Some(DmaDescriptorFault::Writeback { descriptor, fault });
                        self.periph.spi2.abort_transfer();
                        return;
                    }
                }
                self.periph.gdma.out[completion.channel] = completion.final_channel;
                self.periph.spi2.complete_dma_tx(&completion.payload);
                self.irq_dirty = true;
            }
            Ok(None) => {}
            Err(fault) => {
                self.spi2_dma_fault = Some(fault);
                self.periph.spi2.abort_transfer();
            }
        }
    }

    fn deliver_spi2_transfer(&mut self) {
        if self.periph.spi2.dma_tx_pending.is_some() {
            return;
        }
        let Some(transfer) = self.periph.spi2.take_transfer() else { return };
        // Chip select and command/data lines are GPIOs. The board must see their preceding edges
        // before it receives the transaction.
        if !self.periph.gpio.changes.is_empty() {
            let changes = std::mem::take(&mut self.periph.gpio.changes);
            if let Some(events) = &mut self.gpio_events {
                for &(pin, level) in &changes {
                    events.push((self.cycles, pin, level));
                }
            }
            self.board.gpio_changes(&changes);
        }
        let rx = self.board.spi_transfer(2, &transfer.tx, transfer.rx_len);
        self.periph.spi2.finish_transfer(transfer, &rx);
    }

    /// Collect one GP-SPI2 data phase and its GDMA completion without partially committing a
    /// malformed descriptor chain.
    fn spi2_dma_completion(&mut self) -> Result<Option<Spi2DmaCompletion>, DmaDescriptorFault> {
        let Some(bits) = self.periph.spi2.dma_tx_pending else { return Ok(None) };
        let Some(channel_index) = self.periph.gdma.out_channel_for(0) else { return Ok(None) };
        let wanted = (bits as usize).div_ceil(8);
        let mut payload = Vec::with_capacity(wanted);
        let mut visited = HashSet::new();
        let mut channel = self.periph.gdma.out[channel_index];
        let mut descriptor_writebacks = Vec::new();
        let mut steps = 0;
        while payload.len() < wanted {
            let current = channel;
            if !current.running || current.desc == 0 {
                break;
            }
            if steps == SPI2_DMA_DESCRIPTOR_STEP_BUDGET {
                return Err(DmaDescriptorFault::StepBudgetExceeded { budget: SPI2_DMA_DESCRIPTOR_STEP_BUDGET });
            }
            steps += 1;
            if !visited.insert(current.desc) {
                return Err(DmaDescriptorFault::Cycle { descriptor: current.desc });
            }
            let control = self.read32(current.desc).map_err(|fault| DmaDescriptorFault::Read {
                descriptor: current.desc,
                word: DmaDescriptorWord::Control,
                fault,
            })?;
            let size = control & 0xfff;
            let length = (control >> 12) & 0xfff;
            if control & (1 << 31) == 0 {
                return Err(DmaDescriptorFault::NotOwned { descriptor: current.desc });
            }
            if length > size {
                return Err(DmaDescriptorFault::LengthExceedsSize { descriptor: current.desc, size, length });
            }
            let buffer = self.read32(current.desc.wrapping_add(4)).map_err(|fault| DmaDescriptorFault::Read {
                descriptor: current.desc,
                word: DmaDescriptorWord::Buffer,
                fault,
            })?;
            let next = self.read32(current.desc.wrapping_add(8)).map_err(|fault| DmaDescriptorFault::Read {
                descriptor: current.desc,
                word: DmaDescriptorWord::Next,
                fault,
            })?;
            let eof = control & (1 << 30) != 0;
            let remaining = length.saturating_sub(current.buf_pos) as usize;
            if remaining != 0 {
                let count = remaining.min(wanted - payload.len());
                for offset in 0..count {
                    let address = buffer.wrapping_add(current.buf_pos).wrapping_add(offset as u32);
                    payload.push(self.read8(address).map_err(|fault| DmaDescriptorFault::BufferRead {
                        descriptor: current.desc,
                        address,
                        fault,
                    })?);
                }
                channel.buf_pos += count as u32;
                if count < remaining {
                    continue;
                }
            }
            if channel.conf0 & (1 << 2) != 0 {
                let writable = current.desc.checked_add(4).is_some_and(|end| {
                    self.lookup(current.desc).is_some_and(|entry| entry.writable != 0 && end <= entry.hi)
                });
                if !writable {
                    return Err(DmaDescriptorFault::Writeback { descriptor: current.desc, fault: Fault::Prohibited });
                }
                descriptor_writebacks.push((current.desc, control & !(1 << 31)));
            }
            channel.int_raw |= 1 << 0;
            if eof {
                channel.int_raw |= 1 << 1;
                channel.eof_desc = current.desc;
            }
            if next == 0 {
                channel.running = false;
                channel.desc = 0;
                channel.int_raw |= 1 << 3;
            } else {
                channel.desc = next;
                channel.buf_pos = 0;
            }
            if eof {
                break;
            }
        }
        if payload.len() != wanted {
            return Err(DmaDescriptorFault::PayloadTooShort { expected: wanted, actual: payload.len() });
        }
        Ok(Some(Spi2DmaCompletion { channel: channel_index, final_channel: channel, descriptor_writebacks, payload }))
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

    /// LCD RGB output: consume the GDMA out-channel bound to LCD (trigger 5) at the panel's pixel rate,
    /// assemble frames, publish each completed frame to the board and raise LCD_VSYNC.
    /// LCD RGB output. The LCD engine's async FIFO (16 words) is kept full ahead of the pixel clock,
    /// so a DMA link restart mid-frame (the RGB driver skips LCD_FIFO_PRESERVE_SIZE_PX pixels then)
    /// behaves as on silicon. Frames are published to the board and raise LCD_VSYNC.
    fn dma_lcd_step(&mut self, cycles: u64) {
        if !self.periph.lcd_cam.lcd_running() { return; }
        let (ha, va, bpp, frame_cycles) = self.periph.lcd_cam.lcd_geometry();
        let frame_bytes = (ha * va * bpp) as usize;
        if frame_bytes == 0 { return; }
        const FIFO_BYTES: usize = 17 * 2;
        self.periph.lcd_cam.lcd_acc += cycles;
        let due = (self.periph.lcd_cam.lcd_acc as u128 * frame_bytes as u128 / frame_cycles as u128) as usize;
        if due < 512 { return; }
        self.periph.lcd_cam.lcd_acc = 0;
        let log = self.periph.lcd_cam.lcd_log;
        // 1) top the FIFO up from DMA so that it holds `due` + lookahead bytes
        if let Some(ch) = self.periph.gdma.out_channel_for(5) {
            let mut want = (due + FIFO_BYTES).saturating_sub(self.periph.lcd_cam.lcd_fifo.len());
            while want > 0 {
                let c = self.periph.gdma.out[ch];
                if !c.running || c.desc == 0 { break; }
                let dw0 = self.read32(c.desc).unwrap_or(0);
                let length = (dw0 >> 12) & 0xfff; let eof = dw0 & (1 << 30) != 0; let buf = self.read32(c.desc + 4).unwrap_or(0); let next = self.read32(c.desc + 8).unwrap_or(0);
                let remaining = length.saturating_sub(c.buf_pos) as usize;
                if remaining == 0 {
                    if log { eprintln!("[lcd] desc {:#010x} done (buf {:#010x} len {} eof {}) -> next {:#010x}", c.desc, buf, length, eof, next); }
                    let ch_ref = &mut self.periph.gdma.out[ch];
                    ch_ref.int_raw |= 1 << 0;
                    if eof { ch_ref.int_raw |= 1 << 1; ch_ref.eof_desc = c.desc; }
                    if next == 0 { ch_ref.running = false; ch_ref.desc = 0; ch_ref.int_raw |= 1 << 3; break; }
                    ch_ref.desc = next; ch_ref.buf_pos = 0;
                    self.irq_dirty = true;
                    continue;
                }
                let take = remaining.min(want);
                let start = buf + c.buf_pos;
                let mut i = 0usize;
                while i + 4 <= take && (start + i as u32) & 3 == 0 { let v = self.read32(start + i as u32).unwrap_or(0); self.periph.lcd_cam.lcd_fifo.extend(v.to_le_bytes()); i += 4; }
                while i < take { let b = self.read8(start + i as u32).unwrap_or(0); self.periph.lcd_cam.lcd_fifo.push_back(b); i += 1; }
                self.periph.gdma.out[ch].buf_pos += take as u32;
                want -= take;
            }
        }
        // 2) the panel consumes `due` bytes from the FIFO
        let n = due.min(self.periph.lcd_cam.lcd_fifo.len());
        for _ in 0..n { let b = self.periph.lcd_cam.lcd_fifo.pop_front().unwrap(); self.periph.lcd_cam.lcd_line.push(b); }
        while self.periph.lcd_cam.lcd_line.len() >= frame_bytes {
            let frame = std::mem::take(&mut self.periph.lcd_cam.lcd_line);
            self.board.lcd_frame(ha, va, &frame[..frame_bytes]);
            if frame.len() > frame_bytes { self.periph.lcd_cam.lcd_line.extend_from_slice(&frame[frame_bytes..]); }
            self.periph.lcd_cam.lcd_frames += 1;
            self.periph.lcd_cam.int_raw |= 1 << 0;                                    // LCD_VSYNC_INT
            self.irq_dirty = true;
        }
    }

    /// AES accelerator in DMA mode: pull the plaintext from the GDMA out-channel bound to AES,
    /// transform it block by block and write the result back through the in-channel.
    /// Feed the SHA engine from the GDMA out channel bound to it (peripheral 7). mbedTLS hashes
    /// anything bigger than a block this way, so certificate digests never touch the block path.
    fn sha_dma_step(&mut self) {
        self.periph.sha.dma_pending = false;
        let want = self.periph.sha.block_num as usize * self.periph.sha.block_bytes();
        let mut input = Vec::with_capacity(want);
        if let Some(out_ch) = self.periph.gdma.out_channel_for(7) {
            let mut desc = self.periph.gdma.out[out_ch].desc;
            while desc != 0 && input.len() < want {
                let dw0 = self.read32(desc).unwrap_or(0);
                let (len, buf, next) = (((dw0 >> 12) & 0xfff) as usize, self.read32(desc + 4).unwrap_or(0), self.read32(desc + 8).unwrap_or(0));
                for i in 0..len { input.push(self.read8(buf + i as u32).unwrap_or(0)); }
                let eof = dw0 & (1 << 30) != 0;
                let _ = self.write32(desc, dw0 & !(1 << 31));                   // hand the descriptor back
                if eof { self.periph.gdma.out[out_ch].int_raw |= (1 << 0) | (1 << 1); self.periph.gdma.out[out_ch].eof_desc = desc; break; }
                desc = next;
            }
        }
        input.resize(want, 0);
        let bs = self.periph.sha.block_bytes();
        let mut first = self.periph.sha.dma_first;
        for block in input.chunks(bs) {
            self.periph.sha.hash_block(block, first);
            first = false;
        }
        self.periph.sha.busy = false;
        self.irq_dirty = true;
    }

    fn aes_dma_step(&mut self) {
        self.periph.aes.dma_pending = false;
        let (Some(out_ch), Some(in_ch)) = (self.periph.gdma.out_channel_for(6), self.periph.gdma.in_channel_for(6)) else {
            self.periph.aes.state = 2; self.periph.aes.int_raw |= 1; self.irq_dirty = true; return;
        };
        // gather input
        let mut input = Vec::new();
        let mut desc = self.periph.gdma.out[out_ch].desc;
        while desc != 0 {
            let dw0 = self.read32(desc).unwrap_or(0);
            let (len, buf, next) = (((dw0 >> 12) & 0xfff) as usize, self.read32(desc + 4).unwrap_or(0), self.read32(desc + 8).unwrap_or(0));
            for i in 0..len { input.push(self.read8(buf + i as u32).unwrap_or(0)); }
            let eof = dw0 & (1 << 30) != 0;
            let _ = self.write32(desc, dw0 & !(1 << 31));                       // hand the descriptor back
            if eof { self.periph.gdma.out[out_ch].int_raw |= (1 << 0) | (1 << 1); self.periph.gdma.out[out_ch].eof_desc = desc; break; }
            desc = next;
        }
        if self.debug.has("aes") {
            eprintln!("[aes] dma block_mode={} num_blocks={} mode={} bytes={}", self.periph.aes.block_mode, self.periph.aes.num_blocks, self.periph.aes.mode, input.len());
        }
        // transform (ECB and CBC cover what the crypto libraries ask for here)
        let key = self.periph.aes.key_bytes();
        let decrypt = self.periph.aes.decrypting();
        let block_mode = self.periph.aes.block_mode;
        let mut iv = [0u8; 16];
        for (i, w) in self.periph.aes.iv.iter().enumerate() { iv[4 * i..4 * i + 4].copy_from_slice(&w.to_le_bytes()); }
        let mut output = Vec::with_capacity(input.len());
        for chunk in input.chunks(16) {
            let mut b = [0u8; 16];
            b[..chunk.len()].copy_from_slice(chunk);
            let cipher_in = b;
            let o = match block_mode {
                1 => {                                                          // CBC
                    if !decrypt { for i in 0..16 { b[i] ^= iv[i]; } }
                    let mut o = crate::crypto::aes_block(&key, &b, decrypt);
                    if decrypt { for i in 0..16 { o[i] ^= iv[i]; } iv = cipher_in; } else { iv = o; }
                    o
                }
                2 => {                                                          // OFB: keystream feeds itself
                    let ks = crate::crypto::aes_block(&key, &iv, false);
                    iv = ks;
                    let mut o = [0u8; 16];
                    for i in 0..16 { o[i] = b[i] ^ ks[i]; }
                    o
                }
                3 => {                                                          // CTR: encrypt the counter, then bump it
                    let ks = crate::crypto::aes_block(&key, &iv, false);
                    let mut o = [0u8; 16];
                    for i in 0..16 { o[i] = b[i] ^ ks[i]; }
                    for i in (0..16).rev() { iv[i] = iv[i].wrapping_add(1); if iv[i] != 0 { break; } }
                    o
                }
                _ => crate::crypto::aes_block(&key, &b, decrypt),                // ECB
            };
            output.extend_from_slice(&o);
            self.periph.aes.blocks += 1;
        }
        for (i, w) in iv.chunks(4).enumerate() { self.periph.aes.iv[i] = u32::from_le_bytes([w[0], w[1], w[2], w[3]]); }
        // scatter the result
        let mut pos = 0usize;
        let mut desc = self.periph.gdma.inp[in_ch].desc;
        while desc != 0 && pos < output.len() {
            let dw0 = self.read32(desc).unwrap_or(0);
            let (size, buf, next) = ((dw0 & 0xfff) as usize, self.read32(desc + 4).unwrap_or(0), self.read32(desc + 8).unwrap_or(0));
            let n = size.min(output.len() - pos);
            for i in 0..n { let _ = self.write8(buf + i as u32, output[pos + i]); }
            pos += n;
            let ndw0 = (dw0 & !(0xfff << 12) & !(1 << 31)) | ((n as u32) << 12) | (1 << 30);
            let _ = self.write32(desc, ndw0);
            self.periph.gdma.inp[in_ch].eof_desc = desc;
            self.periph.gdma.inp[in_ch].int_raw |= (1 << 0) | (1 << 1);
            if next == 0 { break; }
            desc = next;
        }
        self.periph.aes.state = 2;                                              // DONE
        self.periph.aes.int_raw |= 1;
        self.irq_dirty = true;
    }

    /// WiFi MAC transmit: fetch the queued frames from their DMA descriptors and complete them.
    fn wifi_tx_step(&mut self) {
        let pending = std::mem::take(&mut self.periph.wifi.tx_pending);
        for (slot, desc) in pending {
            let dw0 = self.read32(desc).unwrap_or(0); let pkt = self.read32(desc + 4).unwrap_or(0);
            let len = ((dw0 >> 12) & 0xfff) as usize;
            let mut frame = Vec::with_capacity(len);
            for i in 0..len { frame.push(self.read8(pkt + i as u32).unwrap_or(0)); }
            if self.periph.wifi.log || self.debug.has("wifi-frames") { eprintln!("[wifi] TX slot {} desc {:#010x} pkt {:#010x} {}", slot, desc, pkt, crate::wifi::describe(&frame)); }
            self.periph.wifi.tx_done(slot);
            self.irq_dirty = true;
            let now_us = self.cycles / (crate::periph::CPU_HZ / 1_000_000);
            if let Some(ap) = &mut self.periph.wifi.ap {
                if let Some(data) = ap.on_station_tx(&frame, now_us) {
                    if let Some(eth) = crate::wifi::data_to_eth(&data) { self.periph.wifi.eth_tx.push(eth); }
                }
            }
        }
    }

    /// The virtual air: beacons/responses from the AP and frames from the network backend land in the RX ring.
    fn wifi_air_step(&mut self) {
        let now_us = self.cycles / (crate::periph::CPU_HZ / 1_000_000);
        // The blob's RX path only *indicates* a frame up the 802.11 stack while the descriptor ring is
        // shallow; with several filled descriptors pending it switches to batch block-recycle and drops
        // them. So hold off until the previously delivered descriptor has been recycled by software
        // (has_data cleared) — that is what a real radio sees at low traffic — and never deliver two
        // frames closer than a frame's airtime.
        if now_us.wrapping_sub(self.periph.wifi.last_rx_us) < 400 { return; }
        // ... but if software stops recycling altogether, don't stall the air forever: after 50 ms
        // the frame is dropped, exactly as a real ring would overflow.
        let busy = { let d = self.periph.wifi.last_rx_desc; d != 0 && self.read32(d).unwrap_or(0) & (1 << 30) != 0 };
        if busy && now_us.wrapping_sub(self.periph.wifi.last_rx_us) < 50_000 { return; }
        let mut due = { let ap = self.periph.wifi.ap.as_mut().unwrap(); ap.step(now_us) };
        let eth_in = std::mem::take(&mut self.periph.wifi.eth_rx);
        for e in eth_in { if let Some(f) = self.periph.wifi.ap.as_mut().unwrap().data_from_ds(&e) { due.push(crate::wifi::AirFrame { at_us: now_us, frame: f }); } }
        if due.is_empty() { return; }
        // management responses (auth, assoc, probe) go before beacons: a connect exchange must not be
        // crowded out by beacon traffic
        due.sort_by_key(|a| (crate::wifi::is_beacon(&a.frame), a.at_us));
        let first = due.remove(0);
        self.wifi_rx_deliver(&first.frame, now_us);
        self.periph.wifi.last_rx_us = now_us;
        if let Some(ap) = &mut self.periph.wifi.ap { for a in due { ap.queue.push(a); } }
    }

    /// Write one received frame into the next RX descriptor (rx_ctrl header + frame + FCS) and raise the RX event.
    #[allow(clippy::identity_op, reason = "rx_state zero remains visible in the packed descriptor layout")]
    fn wifi_rx_deliver(&mut self, frame: &[u8], now_us: u64) {
        let desc = self.periph.wifi.rx_next | crate::periph::DMA_ADDR_BASE;
        if desc == 0 { self.periph.wifi.rx_dropped += 1; return; }
        let dw0 = self.read32(desc).unwrap_or(0); let buf = self.read32(desc + 4).unwrap_or(0); let next = self.read32(desc + 8).unwrap_or(0);
        let size = (dw0 & 0xfff) as usize;
        let total = 48 + frame.len() + 4;
        if dw0 & (1 << 31) == 0 || buf == 0 || size < total { self.periph.wifi.rx_dropped += 1; return; }
        let (chan, log) = { let ap = self.periph.wifi.ap.as_ref().unwrap(); (ap.cfg.channel as u32, ap.log) };
        let mut b = Vec::with_capacity(total);
        // rx_ctrl word 0 (silicon: a real broadcast beacon reads 0x111b20ad — bit 28 set, signed rssi in the low
        // byte). The MAC has already address-filtered, so every delivered frame is "for us"; use the same flags
        // for unicast and broadcast (an invented "filter_match" nibble made the blob discard unicast frames).
        // filter-match nibble (silicon: broadcast beacon reads bit 28). A frame the hardware accepted because
        // addr1 is our unicast MAC must carry the unicast-match bit (29), not the broadcast bit (28), or
        // wDev_IndicateFrame drops it as "not for me".
        let bcast = frame.len() >= 5 && frame[4] & 1 == 1;
        // filter-match nibble: bit 28 is the "accepted by the address filter" bit the blob's RX path
        // requires (silicon: a broadcast beacon reads 0x111b20ad); unicast frames add bit 29.
        let fm = if bcast { 1u32 << 28 } else { (1u32 << 28) | (1u32 << 29) };
        let w0: u32 = fm | (0xd8u32 & 0xff);   // rssi -40 dBm, 1 Mbps, legacy
        let w2: u32 = (chan << 16) | (chan << 20);                                        // channel, secondary
        let w5: u32 = 0xa6;                                                                // noise floor -90
        let w11: u32 = ((frame.len() + 4) as u32 & 0xfff) | (0 << 24);                    // sig_len (incl. FCS), rx_state OK
        for w in [w0, 0, w2, now_us as u32, 0, w5, 0, 0, 0, 0, 0, w11] { b.extend_from_slice(&w.to_le_bytes()); }
        b.extend_from_slice(frame); b.extend_from_slice(&crate::wifi::fcs(frame).to_le_bytes());
        let mut i = 0usize;
        while i + 4 <= b.len() { let v = u32::from_le_bytes([b[i], b[i + 1], b[i + 2], b[i + 3]]); let _ = self.write32(buf + i as u32, v); i += 4; }
        while i < b.len() { let _ = self.write8(buf + i as u32, b[i]); i += 1; }
        let ndw0 = (dw0 & !(0xfff << 12)) | ((total as u32) << 12) | (1 << 30) | (1 << 31);   // length; owner AND has_data set (verified on silicon 2026-08-25: dw0=0xc0..)
        let _ = self.write32(desc, ndw0);
        let w = &mut self.periph.wifi;
        w.rx_last = (desc & 0xf_ffff) | (1 << 24); w.rx_next = next & 0xf_ffff; w.last_rx_desc = desc; w.rx_frames += 1; w.events |= (1 << 14) | (1 << 24);   // RX data (wDev_ProcessFiq tests 0x1004000)   // registers hold masked descriptor addrs; rx_last has a 0x01 prefix (silicon)
        if log { let d = crate::wifi::describe(frame); if d.contains("auth")||d.contains("assoc") { eprintln!("[wifi] RX AUTH/ASSOC -> desc {:#010x} buf {:#010x} {}", desc, buf, d); } else { eprintln!("[wifi] RX -> desc {:#010x} {}", desc, d); } }
        self.irq_dirty = true;
    }

    pub fn load_bytes(&mut self, addr: u32, data: &[u8]) -> Result<(), String> {
        for (i, b) in data.iter().enumerate() {
            let a = addr.wrapping_add(i as u32);
            let Some(e) = self.lookup(a) else { return Err(format!("load: address {:#010x} not mapped", a)) };
            let o = e.off as usize + (a - e.lo) as usize;
            self.buf_mut(e.src as u8)[o] = *b;
            self.bump(e.vbase, o - e.off as usize, 1);
        }
        Ok(())
    }
}

impl Bus for SocBus {
    fn read8(&mut self, addr: u32) -> Result<u8, Fault> {
        if Self::is_periph(addr) { return Ok(self.periph_read(addr, 1) as u8); }
        let Some(e) = self.lookup(addr) else { self.last_fault = Some((addr, false)); return Err(Fault::Unmapped) };
        Ok(self.buf(e.src as u8)[e.off as usize + (addr - e.lo) as usize])
    }
    fn read16(&mut self, addr: u32) -> Result<u16, Fault> {
        if Self::is_periph(addr) { return Ok(self.periph_read(addr, 2) as u16); }
        match self.lookup(addr) {
            Some(e) if addr.wrapping_add(2) <= e.hi => { let o = e.off as usize + (addr - e.lo) as usize; Ok(u16::from_le_bytes(self.buf(e.src as u8)[o..o + 2].try_into().unwrap())) }
            Some(_) => Ok(u16::from_le_bytes([self.read8(addr)?, self.read8(addr + 1)?])),       // straddles a page
            None => { self.last_fault = Some((addr, false)); Err(Fault::Unmapped) }
        }
    }
    fn read32(&mut self, addr: u32) -> Result<u32, Fault> {
        if Self::is_periph(addr) { return Ok(self.periph_read(addr, 4)); }
        match self.lookup(addr) {
            Some(e) if addr.wrapping_add(4) <= e.hi => { let o = e.off as usize + (addr - e.lo) as usize; Ok(u32::from_le_bytes(self.buf(e.src as u8)[o..o + 4].try_into().unwrap())) }
            Some(_) => Ok(u32::from_le_bytes([self.read8(addr)?, self.read8(addr + 1)?, self.read8(addr + 2)?, self.read8(addr + 3)?])),
            None => { self.last_fault = Some((addr, false)); Err(Fault::Unmapped) }
        }
    }
    fn write8(&mut self, addr: u32, v: u8) -> Result<(), Fault> {
        if Self::is_periph(addr) { self.periph_write(addr, v as u32, 1); return Ok(()); }
        match self.lookup(addr) {
            Some(e) if e.writable != 0 => { let rel = (addr - e.lo) as usize; self.buf_mut(e.src as u8)[e.off as usize + rel] = v; self.bump(e.vbase, rel, 1); Ok(()) }
            _ => { self.last_fault = Some((addr, true)); Err(Fault::Prohibited) }
        }
    }
    fn write16(&mut self, addr: u32, v: u16) -> Result<(), Fault> {
        if Self::is_periph(addr) { self.periph_write(addr, v as u32, 2); return Ok(()); }
        match self.lookup(addr) {
            Some(e) if e.writable != 0 && addr.wrapping_add(2) <= e.hi => { let rel = (addr - e.lo) as usize; let o = e.off as usize + rel; self.buf_mut(e.src as u8)[o..o + 2].copy_from_slice(&v.to_le_bytes()); self.bump(e.vbase, rel, 2); Ok(()) }
            Some(e) if e.writable != 0 => { let b = v.to_le_bytes(); self.write8(addr, b[0])?; self.write8(addr + 1, b[1]) }
            _ => { self.last_fault = Some((addr, true)); Err(Fault::Prohibited) }
        }
    }
    fn write32(&mut self, addr: u32, v: u32) -> Result<(), Fault> {
        if Self::is_periph(addr) { self.periph_write(addr, v, 4); return Ok(()); }
        match self.lookup(addr) {
            Some(e) if e.writable != 0 && addr.wrapping_add(4) <= e.hi => { let rel = (addr - e.lo) as usize; let o = e.off as usize + rel; self.buf_mut(e.src as u8)[o..o + 4].copy_from_slice(&v.to_le_bytes()); self.bump(e.vbase, rel, 4); Ok(()) }
            Some(e) if e.writable != 0 => { let b = v.to_le_bytes(); for i in 0..4 { self.write8(addr + i, b[i as usize])?; } Ok(()) }
            _ => { self.last_fault = Some((addr, true)); Err(Fault::Prohibited) }
        }
    }
    fn fetch(&mut self, pc: u32) -> Result<[u8; 4], Fault> {
        let Some(e) = self.lookup(pc) else { self.last_fault = Some((pc, false)); return Err(Fault::Unmapped) };
        let o = e.off as usize + (pc - e.lo) as usize;
        let b = self.buf(e.src as u8);
        if let Some(w) = b.get(o..o + 4) { return Ok(w.try_into().unwrap()); }
        // last bytes of a buffer (or of a mapped page): what physical memory has, zero beyond
        let mut r = [0u8; 4];
        for (i, byte) in r.iter_mut().enumerate() { if let Some(x) = b.get(o + i) { *byte = *x; } }
        Ok(r)
    }
    #[inline(always)]
    fn page_versions(&self) -> &[u32] { &self.page_ver }
    #[inline(always)]
    fn note_pc(&mut self, pc: u32) { self.periph.misc.cur_pc = pc; }
    fn fast_mem(&mut self) -> Option<FastMem> { Some(FastMem { tlb: self.tlb.as_ptr(), page_ver: self.page_ver.as_mut_ptr() }) }
    #[inline(always)]
    fn block_break(&self) -> bool { self.irq_dirty }
    fn code_page(&mut self, pc: u32) -> u32 {
        match self.lookup(pc) { Some(e) => e.vbase + ((pc - e.lo) >> VPAGE_SHIFT), None => self.page_ver.len() as u32 - 1 }
    }
    /// Returns 1 when device models actually ran (so interrupt lines may have changed), else 0.
    fn tick(&mut self, cycles: u32) -> u32 {
        self.cycles += cycles as u64;
        self.tick_pending += cycles;
        if self.tick_pending < self.tick_budget { return 0; }
        self.flush_ticks();
        1
    }
}

impl SocBus {
    pub(crate) fn refresh_tick_budget(&mut self) {
        let mut budget = self.periph.cycles_until_timer().clamp(1, MAX_TICK_DEFER);
        if let Some(deadline) = self.board.next_deadline() {
            let until_deadline = u64::from(self.tick_pending)
                .saturating_add(deadline.saturating_sub(self.cycles))
                .clamp(1, u64::from(MAX_TICK_DEFER));
            budget = budget.min(until_deadline as u32);
        }
        self.tick_budget = budget;
    }

    /// Deliver the deferred cycles to the device models now.
    pub fn flush_ticks(&mut self) {
        let c = std::mem::take(&mut self.tick_pending);
        if c == 0 { return; }
        self.tick_impl(c);
        self.refresh_tick_budget();
    }

    fn tick_impl(&mut self, cycles: u32) -> u32 {
        self.periph.tick(cycles as u64);
        self.board.advance_to(self.cycles);
        for edge in self.board.take_edges() {
            if let Some(events) = &mut self.gpio_events { events.push((edge.cycle, edge.pin, edge.level)); }
            if self.periph.gpio.set_input(edge.pin, edge.level) { self.irq_dirty = true; }
        }
        self.complete_spi2_dma();
        self.dma_i2s_step(cycles as u64);
        self.dma_cam_step(cycles as u64);
        self.dma_lcd_step(cycles as u64);
        if !self.periph.wifi.tx_pending.is_empty() { self.wifi_tx_step(); }
        if self.periph.aes.dma_pending { self.aes_dma_step(); }
        if self.periph.sha.dma_pending { self.sha_dma_step(); }
        if self.periph.wifi.ap.is_some() { self.wifi_air_step(); }
        if let Some(net) = &mut self.periph.wifi.net {
            let now_us = self.cycles / (crate::periph::CPU_HZ / 1_000_000);
            let out = std::mem::take(&mut self.periph.wifi.eth_tx);
            // Frames from the station are handled the moment they are sent, but reading the host
            // sockets means syscalls: doing that every scheduling round costs more than emulating
            // the CPU. NET_POLL_US is well under any timeout the guest's TCP stack cares about.
            const NET_POLL_US: u64 = 500;
            let due = now_us.wrapping_sub(self.periph.wifi.net_polled_us) >= NET_POLL_US;
            if !out.is_empty() || due {
                if due { self.periph.wifi.net_polled_us = now_us; }
                let mut replies = Vec::new();
                for e in out { replies.extend(net.handle(&e, now_us)); }
                replies.extend(net.poll(now_us));
                self.periph.wifi.eth_rx.extend(replies);
            }
        }
        if !self.periph.gpio.changes.is_empty() {
            let ch = std::mem::take(&mut self.periph.gpio.changes);
            if let Some(ev) = &mut self.gpio_events { for &(pin, level) in &ch { ev.push((self.cycles, pin, level)); } }
            self.board.gpio_changes(&ch);
        }
        self.deliver_spi2_transfer();
        if !self.periph.rmt.done.is_empty() { for (ch, bits) in std::mem::take(&mut self.periph.rmt.done) { self.board.rmt_frame(ch, &bits); } self.irq_dirty = true; }
        0
    }
}
#[cfg(test)]
mod gp_spi_board_tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    const SPI2: u32 = 0x6002_4000;
    const FIRST_DESC: u32 = 0x3fc9_0100;

    struct ProbeBoard {
        events: Arc<Mutex<Vec<String>>>,
    }

    impl crate::board::BoardModel for ProbeBoard {
        fn name(&self) -> &'static str { "probe" }
        fn gpio_changes(&mut self, changes: &[(u8, bool)]) {
            self.events.lock().expect("probe mutex poisoned").push(format!("gpio:{changes:?}"));
        }
        fn spi_transfer(&mut self, host: u8, tx: &[u8], rx_len: usize) -> Vec<u8> {
            self.events.lock().expect("probe mutex poisoned").push(format!("spi:{host}:{tx:02x?}:{rx_len}"));
            vec![0x5a; rx_len]
        }
    }

    struct FixedDeadlineBoard {
        deadline: u64,
    }

    impl crate::board::BoardModel for FixedDeadlineBoard {
        fn name(&self) -> &'static str { "fixed-deadline-test" }
        fn next_deadline(&self) -> Option<u64> { Some(self.deadline) }
    }

    fn dma_bus() -> SocBus {
        let mut bus = SocBus::new(1024, 1024, [0; 6]);
        bus.periph.gdma.out[0].peri_sel = 0;
        bus.periph.gdma.out[0].desc = FIRST_DESC;
        bus.periph.gdma.out[0].running = true;
        bus
    }

    fn start_dma(bus: &mut SocBus, bits: u32) {
        bus.write32(SPI2 + 0x30, 1 << 28).expect("SPI DMA configuration failed");
        bus.write32(SPI2 + 0x10, 1 << 27).expect("SPI user configuration failed");
        bus.write32(SPI2 + 0x1c, bits - 1).expect("SPI data length failed");
        bus.write32(SPI2, 1 << 24).expect("SPI command failed");
    }

    #[test]
    fn board_answers_before_usr_write_returns_and_after_pending_gpio_edges() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let mut bus = SocBus::new(1024, 1024, [0; 6]);
        bus.board = Box::new(ProbeBoard { events: events.clone() });
        bus.gpio_events = Some(Vec::new());
        bus.periph.gpio.changes.push((12, false));

        bus.write32(SPI2 + 0x10, (1 << 31) | (1 << 28)).expect("SPI setup failed");
        bus.write32(SPI2 + 0x18, (7 << 28) | 0x9f).expect("SPI command phase failed");
        bus.write32(SPI2 + 0x20, 7).expect("SPI response length failed");
        bus.write32(SPI2, 1 << 24).expect("SPI command failed");

        assert_eq!(bus.periph.spi2.w[0] & 0xff, 0x5a);
        assert_ne!(bus.periph.spi2.int_raw & (1 << 12), 0);
        assert_eq!(&*events.lock().expect("probe mutex poisoned"), &["gpio:[(12, false)]", "spi:2:[9f]:1"]);
        assert_eq!(bus.gpio_events.as_deref(), Some(&[(0, 12, false)][..]));
    }

    #[test]
    fn spi2_data_phase_comes_from_gdma_descriptor() {
        const DATA: u32 = 0x3fc9_0200;
        let events = Arc::new(Mutex::new(Vec::new()));
        let mut bus = dma_bus();
        bus.board = Box::new(ProbeBoard { events: events.clone() });
        bus.write32(DATA, 0x4433_2211).expect("test data write failed");
        bus.write32(FIRST_DESC, 4 | (4 << 12) | (1 << 30) | (1 << 31)).expect("descriptor write failed");
        bus.write32(FIRST_DESC + 4, DATA).expect("descriptor buffer write failed");
        bus.write32(FIRST_DESC + 8, 0).expect("descriptor link write failed");
        bus.periph.gdma.out[0].conf0 = 1 << 2;

        start_dma(&mut bus, 32);

        assert_eq!(&*events.lock().expect("probe mutex poisoned"), &["spi:2:[11, 22, 33, 44]:0"]);
        assert_eq!(bus.read32(FIRST_DESC).expect("descriptor read failed") >> 31, 0);
        assert_eq!(bus.periph.gdma.out[0].int_raw & 0xb, 0xb);
    }

    #[test]
    fn descriptor_control_read_failure_is_typed() {
        let mut bus = dma_bus();
        bus.periph.gdma.out[0].desc = DRAM_HIGH;

        start_dma(&mut bus, 8);

        assert_eq!(bus.spi2_dma_fault, Some(DmaDescriptorFault::Read {
            descriptor: DRAM_HIGH,
            word: DmaDescriptorWord::Control,
            fault: Fault::Unmapped,
        }));
    }

    #[test]
    fn descriptor_buffer_read_failure_is_typed() {
        let mut bus = dma_bus();
        bus.write32(FIRST_DESC, 1 | (1 << 12) | (1 << 30) | (1 << 31)).expect("descriptor write failed");
        bus.write32(FIRST_DESC + 4, 0).expect("descriptor buffer write failed");
        bus.write32(FIRST_DESC + 8, 0).expect("descriptor link write failed");

        start_dma(&mut bus, 8);

        assert_eq!(bus.spi2_dma_fault, Some(DmaDescriptorFault::BufferRead {
            descriptor: FIRST_DESC,
            address: 0,
            fault: Fault::Unmapped,
        }));
    }

    #[test]
    fn descriptor_cycle_is_typed() {
        let mut bus = dma_bus();
        bus.write32(FIRST_DESC, 1 << 31).expect("descriptor write failed");
        bus.write32(FIRST_DESC + 4, 0).expect("descriptor buffer write failed");
        bus.write32(FIRST_DESC + 8, FIRST_DESC).expect("descriptor link write failed");

        start_dma(&mut bus, 8);

        assert_eq!(bus.spi2_dma_fault, Some(DmaDescriptorFault::Cycle { descriptor: FIRST_DESC }));
    }

    #[test]
    fn cpu_owned_descriptor_is_typed() {
        let mut bus = dma_bus();
        bus.write32(FIRST_DESC, 1 | (1 << 12)).expect("descriptor write failed");

        start_dma(&mut bus, 8);

        assert_eq!(bus.spi2_dma_fault, Some(DmaDescriptorFault::NotOwned { descriptor: FIRST_DESC }));
    }

    #[test]
    fn descriptor_length_larger_than_size_is_typed() {
        let mut bus = dma_bus();
        bus.write32(FIRST_DESC, 1 | (2 << 12) | (1 << 31)).expect("descriptor write failed");

        start_dma(&mut bus, 8);

        assert_eq!(bus.spi2_dma_fault, Some(DmaDescriptorFault::LengthExceedsSize {
            descriptor: FIRST_DESC,
            size: 1,
            length: 2,
        }));
    }

    #[test]
    fn short_descriptor_chain_is_typed() {
        const DATA: u32 = 0x3fc9_0200;
        let mut bus = dma_bus();
        bus.write8(DATA, 0xaa).expect("test data write failed");
        bus.write32(FIRST_DESC, 1 | (1 << 12) | (1 << 30) | (1 << 31)).expect("descriptor write failed");
        bus.write32(FIRST_DESC + 4, DATA).expect("descriptor buffer write failed");
        bus.write32(FIRST_DESC + 8, 0).expect("descriptor link write failed");

        start_dma(&mut bus, 16);

        assert_eq!(bus.spi2_dma_fault, Some(DmaDescriptorFault::PayloadTooShort { expected: 2, actual: 1 }));
    }

    #[test]
    fn read_only_auto_writeback_descriptor_is_typed() {
        const DATA: u32 = 0x3fc9_0200;
        let mut bus = dma_bus();
        bus.periph.gdma.out[0].desc = IROM_MASK_LOW;
        bus.periph.gdma.out[0].conf0 = 1 << 2;
        bus.write8(DATA, 0xaa).expect("test data write failed");
        bus.irom[0..4].copy_from_slice(&(1u32 | (1 << 12) | (1 << 30) | (1 << 31)).to_le_bytes());
        bus.irom[4..8].copy_from_slice(&DATA.to_le_bytes());
        bus.irom[8..12].copy_from_slice(&0u32.to_le_bytes());

        start_dma(&mut bus, 8);

        assert_eq!(bus.spi2_dma_fault, Some(DmaDescriptorFault::Writeback {
            descriptor: IROM_MASK_LOW,
            fault: Fault::Prohibited,
        }));
    }

    #[test]
    fn descriptor_step_budget_is_typed() {
        let mut bus = dma_bus();
        for step in 0..=SPI2_DMA_DESCRIPTOR_STEP_BUDGET {
            let descriptor = FIRST_DESC + step as u32 * 12;
            bus.write32(descriptor, 1 << 31).expect("descriptor write failed");
            bus.write32(descriptor + 4, 0).expect("descriptor buffer write failed");
            bus.write32(descriptor + 8, descriptor + 12).expect("descriptor link write failed");
        }

        start_dma(&mut bus, 0x40000);

        assert_eq!(bus.spi2_dma_fault, Some(DmaDescriptorFault::StepBudgetExceeded {
            budget: SPI2_DMA_DESCRIPTOR_STEP_BUDGET,
        }));
    }

    #[test]
    fn host_touch_uses_the_current_bus_horizon_and_keeps_its_edge_timestamp() {
        let mut bus = SocBus::new(1024, 1024, [0; 6]);
        bus.board = Box::new(crate::board::WaveshareAmoled18V2::new());
        bus.gpio_events = Some(Vec::new());
        bus.periph.gpio.pin[crate::board::PIN_AMOLED_TOUCH_INT as usize] = (2 << 7) | (1 << 13);
        bus.tick_budget = MAX_TICK_DEFER;

        assert_eq!(Bus::tick(&mut bus, 37), 0);
        assert_eq!(bus.tick_pending, 37);
        esp_soc::SocBus::touch_input(&mut bus, 100, 200, true);
        assert_eq!(bus.tick_pending, 37);
        assert_eq!(bus.tick_budget, 38);
        assert!(bus.gpio_events.as_deref().is_some_and(<[_]>::is_empty));

        Bus::tick(&mut bus, 64);

        assert!(!bus.periph.gpio.level(crate::board::PIN_AMOLED_TOUCH_INT));
        assert!(bus.periph.gpio.irq());
        assert!(bus.irq_dirty);
        assert_eq!(bus.gpio_events.as_deref(), Some(&[(38, crate::board::PIN_AMOLED_TOUCH_INT, false)][..]));
    }

    #[test]
    fn no_edge_touch_keeps_pending_cycles_in_the_deadline_threshold() {
        let mut bus = SocBus::new(1024, 1024, [0; 6]);
        bus.board = Box::new(FixedDeadlineBoard { deadline: 300 });
        bus.tick_budget = MAX_TICK_DEFER;

        assert_eq!(Bus::tick(&mut bus, 100), 0);
        esp_soc::SocBus::touch_input(&mut bus, 0, 0, false);
        assert_eq!((bus.cycles, bus.tick_pending, bus.tick_budget), (100, 100, MAX_TICK_DEFER));
        assert_eq!(Bus::tick(&mut bus, 155), 0);
        assert_eq!(Bus::tick(&mut bus, 1), 1);
    }

    #[test]
    fn reboot_reattaches_amoled_i2c_devices_and_restores_board_input_levels() {
        let mut bus = SocBus::new(1024, 1024, [0; 6]);
        bus.board = Box::new(crate::board::WaveshareAmoled18V2::new());
        bus.attach_board_devices();
        for address in [0x15, 0x20, 0x34, 0x51, 0x6b] {
            assert!(bus.periph.i2c[0].has_device(address));
        }

        Bus::tick(&mut bus, (crate::periph::CPU_HZ / 120) as u32);
        esp_soc::SocBus::touch_input(&mut bus, 100, 200, true);
        Bus::tick(&mut bus, 64);
        assert!(!bus.periph.gpio.level(crate::board::PIN_AMOLED_TE));
        assert!(!bus.periph.gpio.level(crate::board::PIN_AMOLED_TOUCH_INT));

        esp_soc::SocBus::reboot(&mut bus, [0; 6]);

        for address in [0x15, 0x20, 0x34, 0x51, 0x6b] {
            assert!(bus.periph.i2c[0].has_device(address));
        }
        assert!(!bus.periph.gpio.level(crate::board::PIN_AMOLED_TE));
        assert!(!bus.periph.gpio.level(crate::board::PIN_AMOLED_TOUCH_INT));
    }
}
