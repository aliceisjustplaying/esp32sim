//! ESP32-S3 memory map: internal SRAM (512 KiB, IRAM/DRAM aliases), mask ROM, RTC
//! memories, external flash + PSRAM through the 512-entry cache MMU, peripherals.
use crate::board::{Board, Spi2DmaRequest, Spi2DmaTiming, Spi2DmaTimingRefusal, Spi2Mode};
use crate::periph::{Peripherals, PERIPH_BASE, PERIPH_END};
use std::collections::HashSet;
use xtensa_lx7::bus::{Bus, Fault};
use xtensa_lx7::measured::{MeasuredBus, MemoryClass};

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
    Read {
        descriptor: u32,
        word: DmaDescriptorWord,
        fault: Fault,
    },
    BufferRead {
        descriptor: u32,
        address: u32,
        fault: Fault,
    },
    Cycle {
        descriptor: u32,
    },
    StepBudgetExceeded {
        budget: usize,
    },
}

struct Spi2DmaCompletionPlan {
    channel: usize,
    final_channel: crate::periph::GdmaOutCh,
    descriptor_writebacks: Vec<(u32, u32)>,
}

struct PendingSpi2Dma {
    transfer: crate::periph::GpSpiTransfer,
    completion: Spi2DmaCompletionPlan,
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
    pub spi2_dma_timing_fault: Option<Spi2DmaTimingRefusal>,
    pub last_spi2_dma_timing: Option<Spi2DmaTiming>,
    pub board_deadline_fault: Option<crate::board::BoardDeadlineError>,
    /// set by any peripheral write: interrupt lines must be re-evaluated before the next instruction
    pub irq_dirty: bool,
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
    /// have passed, so guest-visible time is exact while idle rounds cost nothing.
    tick_pending: u32,
    tick_budget: u32,
    pending_spi2_dma: Option<PendingSpi2Dma>,
}

/// Longest stretch of cycles device models may go without seeing time advance. Bounds the
/// latency of everything that has no computed deadline (DMA, USB, LCD, WiFi).
const MAX_TICK_DEFER: u32 = 256;

/// Buffer identifiers for resolved addresses.
pub const SRC_SRAM: u8 = 0;
pub const SRC_IROM: u8 = 1;
pub const SRC_FLASH: u8 = 2;
pub const SRC_PSRAM: u8 = 3;
pub const SRC_DROM: u8 = 4;
pub const SRC_RTC_FAST: u8 = 5;
pub const SRC_RTC_SLOW: u8 = 6;
const TLB_SIZE: usize = xtensa_lx7::bus::TLB_ENTRIES;
/// Granularity of the write-version counters. Must exceed the longest block (`block::MAX_LEN` × 3 bytes).
const VPAGE_SHIFT: usize = xtensa_lx7::bus::VPAGE_SHIFT as usize;
const VPAGE_MASK: usize = (1 << VPAGE_SHIFT) - 1;
use xtensa_lx7::bus::{FastMem, TlbEntry};
#[inline(always)]
fn tlb_idx(addr: u32) -> usize {
    xtensa_lx7::bus::tlb_index(addr)
}

impl SocBus {
    pub fn new(flash_size: usize, psram_size: usize, mac: [u8; 6]) -> Self {
        Self::with_sizes(flash_size, psram_size, mac)
    }
    pub fn with_sizes(flash_size: usize, psram_size: usize, mac: [u8; 6]) -> Self {
        let bus_uninit = SocBus {
            sram: vec![0; SRAM_SIZE],
            irom: vec![0; (IROM_MASK_HIGH - IROM_MASK_LOW) as usize],
            drom: vec![0; (DROM_MASK_HIGH - DROM_MASK_LOW) as usize],
            rtc_fast: vec![0; 8192],
            rtc_slow: vec![0; 8192],
            flash: vec![0xff; flash_size],
            psram: vec![0; psram_size],
            mmu: [MMU_INVALID; MMU_ENTRIES],
            periph: Peripherals::new(mac),
            board: Box::new(crate::board::Atech14::new()),
            cycles: 0,
            last_fault: None,
            spi2_dma_fault: None,
            spi2_dma_timing_fault: None,
            last_spi2_dma_timing: None,
            board_deadline_fault: None,
            irq_dirty: false,
            tlb: vec![TlbEntry::EMPTY; TLB_SIZE],
            page_ver: Vec::new(),
            ver_base: [0; 7],
            tick_pending: 0,
            tick_budget: 0,
            pending_spi2_dma: None,
        };
        let mut b = bus_uninit;
        b.rebuild_page_table();
        b
    }

    fn measured_mapping(&self, address: u32) -> Option<(u8, usize)> {
        let direct = match address {
            DRAM_LOW..=0x3fcf_ffff => Some((SRC_SRAM, (address - DRAM_LOW + 0x8000) as usize)),
            IRAM_LOW..=0x403d_ffff => Some((SRC_SRAM, (address - IRAM_LOW) as usize)),
            IROM_MASK_LOW..=0x4005_ffff => Some((SRC_IROM, (address - IROM_MASK_LOW) as usize)),
            DROM_MASK_LOW..=0x3ff1_ffff => Some((SRC_DROM, (address - DROM_MASK_LOW) as usize)),
            RTC_FAST_LOW..=0x600f_ffff => Some((SRC_RTC_FAST, (address - RTC_FAST_LOW) as usize)),
            RTC_SLOW_LOW..=0x5000_1fff => Some((SRC_RTC_SLOW, (address - RTC_SLOW_LOW) as usize)),
            _ => None,
        };
        if direct.is_some() {
            return direct;
        }
        if !(DBUS_LOW..DBUS_HIGH).contains(&address) && !(IBUS_LOW..IBUS_HIGH).contains(&address) {
            return None;
        }
        let linear = address & 0x1ff_ffff;
        let entry = self.mmu[(linear >> 16) as usize];
        if entry & MMU_INVALID != 0 {
            return None;
        }
        let offset = (entry & 0x3fff) as usize * PAGE as usize + (address & 0xffff) as usize;
        let source = if entry & MMU_SPIRAM != 0 {
            SRC_PSRAM
        } else {
            SRC_FLASH
        };
        (offset < self.buf(source).len()).then_some((source, offset))
    }

    fn deliver_board_edges(&mut self) {
        for edge in self.board.take_edges() {
            if self.periph.gpio.set_input(edge.pin, edge.level) {
                self.irq_dirty = true;
            }
        }
    }

    /// Advance every device and board model to an absolute measured cycle.
    pub fn advance_measured_to(
        &mut self,
        target: backend_api::VirtualCycle,
    ) -> Result<(), crate::board::BoardDeadlineError> {
        if target < self.cycles {
            return Err(crate::board::BoardDeadlineError::TimeReversed {
                current: self.cycles,
                requested: target,
            });
        }
        self.board_deadline_fault = None;
        while self.cycles < target {
            let step = (target - self.cycles).min(u64::from(u32::MAX)) as u32;
            <Self as Bus>::tick(self, step);
            self.flush_ticks();
            if let Some(fault) = self.board_deadline_fault.take() {
                return Err(fault);
            }
        }
        self.board.advance_to(target)?;
        self.deliver_board_edges();
        self.deliver_spi2_dma_completion();
        self.refresh_tick_budget();
        Ok(())
    }

    /// Size the per-page version table to the buffers. Call after replacing `flash` or `psram`.
    pub fn rebuild_page_table(&mut self) {
        let sizes = [
            self.sram.len(),
            self.irom.len(),
            self.flash.len(),
            self.psram.len(),
            self.drom.len(),
            self.rtc_fast.len(),
            self.rtc_slow.len(),
        ];
        let mut base = 0u32;
        for (i, n) in sizes.iter().enumerate() {
            self.ver_base[i] = base;
            base += ((n + VPAGE_MASK) >> VPAGE_SHIFT) as u32;
        }
        self.page_ver = vec![0; base as usize + 1];
        self.invalidate_tlb();
    }

    /// Forget every cached mapping. Anything that re-points the flash MMU must call this.
    /// A remap changes which bytes a cache-window pc refers to without any write happening, so
    /// the flash and PSRAM page versions are bumped too: that is what invalidates decoded
    /// instructions and blocks that were built through the old mapping.
    pub fn invalidate_tlb(&mut self) {
        for e in self.tlb.iter_mut() {
            *e = TlbEntry::EMPTY;
        }
        let (a, b) = (
            self.ver_base[SRC_FLASH as usize] as usize,
            self.ver_base[SRC_DROM as usize] as usize,
        );
        for v in &mut self.page_ver[a..b] {
            *v = v.wrapping_add(1);
        } // flash then psram
    }

    #[inline(always)]
    fn buf(&self, src: u8) -> &Vec<u8> {
        match src {
            SRC_SRAM => &self.sram,
            SRC_IROM => &self.irom,
            SRC_FLASH => &self.flash,
            SRC_PSRAM => &self.psram,
            SRC_DROM => &self.drom,
            SRC_RTC_FAST => &self.rtc_fast,
            _ => &self.rtc_slow,
        }
    }
    #[inline(always)]
    fn buf_mut(&mut self, src: u8) -> &mut Vec<u8> {
        match src {
            SRC_SRAM => &mut self.sram,
            SRC_IROM => &mut self.irom,
            SRC_FLASH => &mut self.flash,
            SRC_PSRAM => &mut self.psram,
            SRC_DROM => &mut self.drom,
            SRC_RTC_FAST => &mut self.rtc_fast,
            _ => &mut self.rtc_slow,
        }
    }

    /// The mapping covering `addr`, from the TLB or by walking the address map.
    #[inline(always)]
    fn lookup(&mut self, addr: u32) -> Option<TlbEntry> {
        let e = self.tlb[tlb_idx(addr)];
        if addr >= e.lo && addr < e.hi {
            Some(e)
        } else {
            self.tlb_fill(addr)
        }
    }

    /// Walk the address map for the 64 KiB page holding `addr` and remember it.
    #[expect(
        unsafe_code,
        reason = "the software TLB caches a pointer into an owned buffer"
    )]
    fn tlb_fill(&mut self, addr: u32) -> Option<TlbEntry> {
        let page = addr & !0xffff;
        let region = |lo: u32, hi: u32, src: u8, w: bool| -> TlbEntry {
            let lo_ = page.max(lo);
            let hi_ = (page + 0x10000).min(hi);
            TlbEntry {
                lo: lo_,
                hi: hi_,
                base: std::ptr::null_mut(),
                off: lo_ - lo,
                vbase: 0,
                src: src as u32,
                writable: w as u32,
            }
        };
        let mut e = match addr {
            DRAM_LOW..=0x3FCF_FFFF => {
                let mut e = region(DRAM_LOW, 0x3FD0_0000, SRC_SRAM, true);
                e.off += 0x8000;
                e
            }
            IRAM_LOW..=0x403D_FFFF => region(IRAM_LOW, 0x403E_0000, SRC_SRAM, true),
            IROM_MASK_LOW..=0x4005_FFFF => region(IROM_MASK_LOW, 0x4006_0000, SRC_IROM, false),
            DROM_MASK_LOW..=0x3FF1_FFFF => region(DROM_MASK_LOW, 0x3FF2_0000, SRC_DROM, false),
            RTC_FAST_LOW..=0x600F_FFFF => region(RTC_FAST_LOW, 0x6010_0000, SRC_RTC_FAST, true),
            RTC_SLOW_LOW..=0x5000_1FFF => region(RTC_SLOW_LOW, 0x5000_2000, SRC_RTC_SLOW, true),
            DBUS_LOW..=0x3DFF_FFFF | IBUS_LOW..=0x43FF_FFFF => {
                let linear = addr & 0x1FF_FFFF;
                let entry = self.mmu[(linear >> 16) as usize];
                if entry & MMU_INVALID != 0 {
                    return None;
                }
                let off = (entry & 0x3fff) as usize * PAGE as usize;
                let (src, w) = if entry & MMU_SPIRAM != 0 {
                    (SRC_PSRAM, true)
                } else {
                    (SRC_FLASH, false)
                };
                if off + PAGE as usize > self.buf(src).len() {
                    return None;
                }
                TlbEntry {
                    lo: page,
                    hi: page + 0x10000,
                    base: std::ptr::null_mut(),
                    off: off as u32,
                    vbase: 0,
                    src: src as u32,
                    writable: w as u32,
                }
            }
            _ => return None,
        };
        e.vbase = self.ver_base[e.src as usize] + (e.off as usize >> VPAGE_SHIFT) as u32;
        let off = e.off as usize;
        // SAFETY: The region construction above bounds `off` within the selected owned buffer.
        // The buffer is not resized while the TLB entry is live.
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
        if last != p {
            self.page_ver[last] = self.page_ver[last].wrapping_add(1);
        }
        if off & VPAGE_MASK < 3 && p > 0 {
            self.page_ver[p - 1] = self.page_ver[p - 1].wrapping_add(1);
        }
    }

    /// Record a write done behind the bus's back (image loaders, the SPI flash controller).
    pub fn note_written(&mut self, src: u8, off: usize, len: usize) {
        if len == 0 {
            return;
        }
        let vbase = self.ver_base[src as usize];
        let (first, last) = (off >> VPAGE_SHIFT, (off + len - 1) >> VPAGE_SHIFT);
        for p in first..=last {
            let i = vbase as usize + p;
            if i < self.page_ver.len() {
                self.page_ver[i] = self.page_ver[i].wrapping_add(1);
            }
        }
        if off & VPAGE_MASK < 3 && first > 0 {
            let i = vbase as usize + first - 1;
            self.page_ver[i] = self.page_ver[i].wrapping_add(1);
        }
    }

    #[inline]
    fn is_periph(addr: u32) -> bool {
        (PERIPH_BASE..PERIPH_END).contains(&addr)
    }

    fn periph_read(&mut self, addr: u32, size: u32) -> u32 {
        if (MMU_TABLE..MMU_TABLE + (MMU_ENTRIES as u32) * 4).contains(&addr) {
            return self.mmu[((addr - MMU_TABLE) >> 2) as usize];
        }
        self.flush_ticks(); // registers must show exact time
        let w = self.periph.read32(addr & !3);
        match size {
            1 => (w >> ((addr & 3) * 8)) & 0xff,
            2 => (w >> ((addr & 2) * 8)) & 0xffff,
            _ => w,
        }
    }
    fn periph_write(&mut self, addr: u32, v: u32, size: u32) {
        self.periph_write_inner(addr, v, size);
        self.refresh_tick_budget();
        // the write may have armed something
    }
    fn periph_write_inner(&mut self, addr: u32, v: u32, size: u32) {
        if (MMU_TABLE..MMU_TABLE + (MMU_ENTRIES as u32) * 4).contains(&addr) {
            let i = ((addr - MMU_TABLE) >> 2) as usize;
            if self.mmu[i] != v & 0xffff {
                self.mmu[i] = v & 0xffff;
                self.invalidate_tlb();
            }
            return;
        }
        self.flush_ticks();
        let a = addr & !3;
        let w = match size {
            4 => v,
            2 => {
                let old = self.periph.read32(a);
                let sh = (addr & 2) * 8;
                (old & !(0xffff << sh)) | ((v & 0xffff) << sh)
            }
            _ => {
                let old = self.periph.read32(a);
                let sh = (addr & 3) * 8;
                (old & !(0xff << sh)) | ((v & 0xff) << sh)
            }
        };
        self.periph.write32(a, w);
        if let Some(mut transfer) = self.periph.spi2.take_transfer() {
            let dma_completion = match self.spi2_dma_payload(&mut transfer) {
                Ok(completion) => completion,
                Err(fault) => {
                    self.spi2_dma_fault = Some(fault);
                    return;
                }
            };
            // GPIO carries chip select and command/data pins for existing boards. Deliver every
            // preceding edge before the synchronous SPI transaction so the board observes bus order.
            if !self.periph.gpio.changes.is_empty() {
                let changes = std::mem::take(&mut self.periph.gpio.changes);
                self.board.gpio_changes(&changes);
            }
            match dma_completion {
                Some(completion) if self.board.name() == "waveshare-amoled18-v2" => {
                    let request = Spi2DmaRequest {
                        submitted_at: self.cycles,
                        bytes: transfer.data_len,
                        clock_hz: self.spi2_clock_hz(),
                        mode: self.spi2_mode(),
                    };
                    match self.board.schedule_spi2_dma(request) {
                        Ok(timing) => {
                            self.last_spi2_dma_timing = Some(timing);
                            self.pending_spi2_dma = Some(PendingSpi2Dma {
                                transfer,
                                completion,
                            });
                        }
                        Err(fault) => {
                            self.spi2_dma_timing_fault = Some(fault);
                            return;
                        }
                    }
                }
                completion => {
                    if let Some(completion) = completion {
                        self.apply_spi2_dma_completion(completion);
                    }
                    let rx = self.board.spi_transfer(2, &transfer.tx, transfer.rx_len);
                    self.periph.spi2.finish_transfer(transfer, &rx);
                }
            }
        }
        // GPIO output registers (OUT/W1TS/W1TC/OUT1...) are hammered by bit-banged SPI and never change an
        // interrupt line directly; the periodic 32-cycle poll still sees any indirect effect
        if !(0x6000_4004..=0x6000_4018).contains(&a) {
            self.irq_dirty = true;
        }
        if self.periph.spi_exec {
            self.periph.spi_exec = false;
            self.periph.spi1.execute(&mut self.flash, &mut self.psram);
            for (src, off, len) in std::mem::take(&mut self.periph.spi1.dirty) {
                self.note_written(src, off, len);
            }
        }
    }

    /// Replace the GP-SPI2 data phase with bytes from its active GDMA TX descriptor chain.
    /// ESP-IDF uses this path for both small panel commands and full color transfers.
    fn spi2_dma_payload(
        &mut self,
        transfer: &mut crate::periph::GpSpiTransfer,
    ) -> Result<Option<Spi2DmaCompletionPlan>, DmaDescriptorFault> {
        let Some(ch) = self.periph.gdma.out_channel_for(0) else {
            return Ok(None);
        };
        let mut payload = Vec::with_capacity(transfer.data_len);
        let mut visited = HashSet::new();
        let mut channel = self.periph.gdma.out[ch];
        let mut descriptor_writebacks = Vec::new();
        let mut steps = 0;
        while payload.len() < transfer.data_len {
            let c = channel;
            if !c.running || c.desc == 0 {
                break;
            }
            if steps == SPI2_DMA_DESCRIPTOR_STEP_BUDGET {
                return Err(DmaDescriptorFault::StepBudgetExceeded {
                    budget: SPI2_DMA_DESCRIPTOR_STEP_BUDGET,
                });
            }
            steps += 1;
            if !visited.insert(c.desc) {
                return Err(DmaDescriptorFault::Cycle { descriptor: c.desc });
            }
            let dw0 = self
                .read32(c.desc)
                .map_err(|fault| DmaDescriptorFault::Read {
                    descriptor: c.desc,
                    word: DmaDescriptorWord::Control,
                    fault,
                })?;
            let length = (dw0 >> 12) & 0xfff;
            let eof = dw0 & (1 << 30) != 0;
            let buf = self
                .read32(c.desc + 4)
                .map_err(|fault| DmaDescriptorFault::Read {
                    descriptor: c.desc,
                    word: DmaDescriptorWord::Buffer,
                    fault,
                })?;
            let next = self
                .read32(c.desc + 8)
                .map_err(|fault| DmaDescriptorFault::Read {
                    descriptor: c.desc,
                    word: DmaDescriptorWord::Next,
                    fault,
                })?;
            let remaining = length.saturating_sub(c.buf_pos) as usize;
            if remaining != 0 {
                let take = remaining.min(transfer.data_len - payload.len());
                for i in 0..take {
                    let address = buf + c.buf_pos + i as u32;
                    payload.push(self.read8(address).map_err(|fault| {
                        DmaDescriptorFault::BufferRead {
                            descriptor: c.desc,
                            address,
                            fault,
                        }
                    })?);
                }
                channel.buf_pos += take as u32;
                if take < remaining {
                    continue;
                }
            }
            if channel.conf0 & (1 << 2) != 0 {
                descriptor_writebacks.push((c.desc, dw0 & !(1 << 31)));
            }
            channel.int_raw |= 1 << 0;
            if eof {
                channel.int_raw |= 1 << 1;
                channel.eof_desc = c.desc;
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
        if !payload.is_empty() && transfer.tx.len() < transfer.data_offset + transfer.data_len {
            transfer
                .tx
                .resize(transfer.data_offset + transfer.data_len, 0);
        }
        let n = payload
            .len()
            .min(transfer.tx.len().saturating_sub(transfer.data_offset));
        transfer.tx[transfer.data_offset..transfer.data_offset + n].copy_from_slice(&payload[..n]);
        Ok(Some(Spi2DmaCompletionPlan {
            channel: ch,
            final_channel: channel,
            descriptor_writebacks,
        }))
    }

    fn spi2_mode(&self) -> Spi2Mode {
        let user = self.periph.spi2.regs.read(0x10);
        if user & (1 << 14) != 0 {
            Spi2Mode::Octal
        } else if user & (1 << 13) != 0 {
            Spi2Mode::Quad
        } else if user & (1 << 12) != 0 {
            Spi2Mode::Dual
        } else {
            Spi2Mode::Single
        }
    }

    fn spi2_clock_hz(&self) -> u32 {
        let clock = self.periph.spi2.regs.read(0x0c);
        if clock & (1 << 31) != 0 {
            return crate::periph::APB_HZ as u32;
        }
        let predivider = ((clock >> 18) & 0xf) + 1;
        let divider = predivider * (((clock >> 12) & 0x3f) + 1);
        crate::periph::APB_HZ as u32 / divider
    }

    fn deliver_spi2_dma_completion(&mut self) {
        if !self.board.take_spi2_dma_completion() {
            return;
        }
        let Some(pending) = self.pending_spi2_dma.take() else {
            return;
        };
        self.apply_spi2_dma_completion(pending.completion);
        let rx = self
            .board
            .spi_transfer(2, &pending.transfer.tx, pending.transfer.rx_len);
        self.periph.spi2.finish_transfer(pending.transfer, &rx);
    }

    fn apply_spi2_dma_completion(&mut self, completion: Spi2DmaCompletionPlan) {
        for (descriptor, control) in completion.descriptor_writebacks {
            let _ = self.write32(descriptor, control);
        }
        self.periph.gdma.out[completion.channel] = completion.final_channel;
        self.irq_dirty = true;
    }

    /// Move I2S TX data out of DMA descriptors at the sample rate.
    fn dma_i2s_step(&mut self, cycles: u64) {
        self.dma_i2s_one(cycles, 0);
        self.dma_i2s_one(cycles, 1);
    }

    /// Move I2S TX data for controller `which` (0 = I2S0 on GDMA trigger 3, 1 = I2S1 on trigger 4).
    fn dma_i2s_one(&mut self, cycles: u64, which: usize) {
        let (frames, bpf) = {
            let i2s = if which == 0 {
                &mut self.periph.i2s0
            } else {
                &mut self.periph.i2s1
            };
            (i2s.frames_due(cycles), i2s.bytes_per_frame as usize)
        };
        if frames == 0 {
            return;
        }
        let Some(ch) = self
            .periph
            .gdma
            .out_channel_for(if which == 0 { 3 } else { 4 })
        else {
            return;
        };
        let mut need = frames as usize * bpf;
        let mut samples: Vec<i16> = Vec::new();
        while need > 0 {
            let c = self.periph.gdma.out[ch];
            if !c.running || c.desc == 0 {
                break;
            }
            let dw0 = self.read32(c.desc).unwrap_or(0);
            let d = crate::periph::DmaDesc {
                addr: c.desc,
                size: dw0 & 0xfff,
                length: (dw0 >> 12) & 0xfff,
                eof: dw0 & (1 << 30) != 0,
                owner_dma: dw0 & (1 << 31) != 0,
                buf: self.read32(c.desc + 4).unwrap_or(0),
                next: self.read32(c.desc + 8).unwrap_or(0),
            };
            let remaining = d.length.saturating_sub(c.buf_pos) as usize;
            if remaining == 0 {
                // descriptor complete: hand back to software, raise EOF/DONE, advance
                let ch_ref = &mut self.periph.gdma.out[ch];
                if ch_ref.conf0 & (1 << 2) != 0 {
                    let dw0 = self.read32(d.addr).unwrap_or(0) & !(1 << 31);
                    let _ = self.write32(d.addr, dw0);
                } // AUTO_WRBACK: owner -> cpu
                let ch_ref = &mut self.periph.gdma.out[ch];
                ch_ref.int_raw |= 1 << 0; // OUT_DONE
                if d.eof {
                    ch_ref.int_raw |= 1 << 1;
                    ch_ref.eof_desc = d.addr;
                } // OUT_EOF
                if d.next == 0 {
                    ch_ref.running = false;
                    ch_ref.desc = 0;
                    ch_ref.int_raw |= 1 << 3;
                    break;
                } // OUT_TOTAL_EOF
                ch_ref.desc = d.next;
                ch_ref.buf_pos = 0;
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
        if !samples.is_empty() {
            let i2s = if which == 0 {
                &mut self.periph.i2s0
            } else {
                &mut self.periph.i2s1
            };
            i2s.frames_out += samples.len() as u64;
            i2s.pcm.extend_from_slice(&samples);
        }
    }

    /// Camera engine: when a sensor frame is due, push it through the GDMA IN channel bound to CAM (trigger 5).
    fn dma_cam_step(&mut self, cycles: u64) {
        if !self.periph.lcd_cam.frame_due(cycles) {
            return;
        }
        let Some(ch) = self.periph.gdma.in_channel_for(5) else {
            self.periph.lcd_cam.dropped += 1;
            return;
        };
        let Some((_w, _h, frame)) = self.board.camera_frame() else {
            self.periph.lcd_cam.dropped += 1;
            return;
        };
        let mut pos = 0usize;
        let mut desc = self.periph.gdma.inp[ch].desc;
        let mut last = desc;
        while desc != 0 && pos < frame.len() {
            let dw0 = self.read32(desc).unwrap_or(0);
            let size = (dw0 & 0xfff) as usize;
            let buf = self.read32(desc + 4).unwrap_or(0);
            let next = self.read32(desc + 8).unwrap_or(0);
            if dw0 & (1 << 31) == 0 || size == 0 {
                break;
            } // descriptor not owned by DMA
            let n = size.min(frame.len() - pos);
            let mut i = 0;
            while i + 4 <= n {
                let v = u32::from_le_bytes([
                    frame[pos + i],
                    frame[pos + i + 1],
                    frame[pos + i + 2],
                    frame[pos + i + 3],
                ]);
                let _ = self.write32(buf + i as u32, v);
                i += 4;
            }
            while i < n {
                let _ = self.write8(buf + i as u32, frame[pos + i]);
                i += 1;
            }
            pos += n;
            let eof = pos >= frame.len();
            let ndw0 = (dw0 & !(0xfff << 12) & !(1 << 31) & !(1 << 30))
                | ((n as u32) << 12)
                | if eof { 1 << 30 } else { 0 }; // length, owner=cpu, suc_eof
            let _ = self.write32(desc, ndw0);
            last = desc;
            desc = next;
        }
        let r = &mut self.periph.gdma.inp[ch];
        r.eof_desc = last;
        r.desc = desc;
        r.int_raw |= (1 << 0) | (1 << 1); // IN_DONE | IN_SUC_EOF
        if desc == 0 {
            r.running = false;
        }
        self.periph.lcd_cam.int_raw |= 1 << 2; // CAM_VSYNC_INT
        self.periph.lcd_cam.frames += 1;
        self.irq_dirty = true;
    }

    /// LCD RGB output: consume the GDMA out-channel bound to LCD (trigger 5) at the panel's pixel rate,
    /// assemble frames, publish each completed frame to the board and raise LCD_VSYNC.
    /// LCD RGB output. The LCD engine's async FIFO (16 words) is kept full ahead of the pixel clock,
    /// so a DMA link restart mid-frame (the RGB driver skips LCD_FIFO_PRESERVE_SIZE_PX pixels then)
    /// behaves as on silicon. Frames are published to the board and raise LCD_VSYNC.
    fn dma_lcd_step(&mut self, cycles: u64) {
        if !self.periph.lcd_cam.lcd_running() {
            return;
        }
        let (ha, va, bpp, frame_cycles) = self.periph.lcd_cam.lcd_geometry();
        let frame_bytes = (ha * va * bpp) as usize;
        if frame_bytes == 0 {
            return;
        }
        const FIFO_BYTES: usize = 17 * 2;
        self.periph.lcd_cam.lcd_acc += cycles;
        let due = (self.periph.lcd_cam.lcd_acc as u128 * frame_bytes as u128 / frame_cycles as u128)
            as usize;
        if due < 512 {
            return;
        }
        self.periph.lcd_cam.lcd_acc = 0;
        let log = self.periph.lcd_cam.lcd_log;
        // 1) top the FIFO up from DMA so that it holds `due` + lookahead bytes
        if let Some(ch) = self.periph.gdma.out_channel_for(5) {
            let mut want = (due + FIFO_BYTES).saturating_sub(self.periph.lcd_cam.lcd_fifo.len());
            while want > 0 {
                let c = self.periph.gdma.out[ch];
                if !c.running || c.desc == 0 {
                    break;
                }
                let dw0 = self.read32(c.desc).unwrap_or(0);
                let length = (dw0 >> 12) & 0xfff;
                let eof = dw0 & (1 << 30) != 0;
                let buf = self.read32(c.desc + 4).unwrap_or(0);
                let next = self.read32(c.desc + 8).unwrap_or(0);
                let remaining = length.saturating_sub(c.buf_pos) as usize;
                if remaining == 0 {
                    if log {
                        eprintln!("[lcd] desc {:#010x} done (buf {:#010x} len {} eof {}) -> next {:#010x}", c.desc, buf, length, eof, next);
                    }
                    let ch_ref = &mut self.periph.gdma.out[ch];
                    ch_ref.int_raw |= 1 << 0;
                    if eof {
                        ch_ref.int_raw |= 1 << 1;
                        ch_ref.eof_desc = c.desc;
                    }
                    if next == 0 {
                        ch_ref.running = false;
                        ch_ref.desc = 0;
                        ch_ref.int_raw |= 1 << 3;
                        break;
                    }
                    ch_ref.desc = next;
                    ch_ref.buf_pos = 0;
                    self.irq_dirty = true;
                    continue;
                }
                let take = remaining.min(want);
                let start = buf + c.buf_pos;
                let mut i = 0usize;
                while i + 4 <= take && (start + i as u32) & 3 == 0 {
                    let v = self.read32(start + i as u32).unwrap_or(0);
                    self.periph.lcd_cam.lcd_fifo.extend(v.to_le_bytes());
                    i += 4;
                }
                while i < take {
                    let b = self.read8(start + i as u32).unwrap_or(0);
                    self.periph.lcd_cam.lcd_fifo.push_back(b);
                    i += 1;
                }
                self.periph.gdma.out[ch].buf_pos += take as u32;
                want -= take;
            }
        }
        // 2) the panel consumes `due` bytes from the FIFO
        let n = due.min(self.periph.lcd_cam.lcd_fifo.len());
        for _ in 0..n {
            let b = self
                .periph
                .lcd_cam
                .lcd_fifo
                .pop_front()
                .expect("the bounded LCD FIFO drain count guarantees an available byte");
            self.periph.lcd_cam.lcd_line.push(b);
        }
        while self.periph.lcd_cam.lcd_line.len() >= frame_bytes {
            let frame = std::mem::take(&mut self.periph.lcd_cam.lcd_line);
            self.board.lcd_frame(ha, va, &frame[..frame_bytes]);
            if frame.len() > frame_bytes {
                self.periph
                    .lcd_cam
                    .lcd_line
                    .extend_from_slice(&frame[frame_bytes..]);
            }
            self.periph.lcd_cam.lcd_frames += 1;
            self.periph.lcd_cam.int_raw |= 1 << 0; // LCD_VSYNC_INT
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
                let (len, buf, next) = (
                    ((dw0 >> 12) & 0xfff) as usize,
                    self.read32(desc + 4).unwrap_or(0),
                    self.read32(desc + 8).unwrap_or(0),
                );
                for i in 0..len {
                    input.push(self.read8(buf + i as u32).unwrap_or(0));
                }
                let eof = dw0 & (1 << 30) != 0;
                let _ = self.write32(desc, dw0 & !(1 << 31)); // hand the descriptor back
                if eof {
                    self.periph.gdma.out[out_ch].int_raw |= (1 << 0) | (1 << 1);
                    self.periph.gdma.out[out_ch].eof_desc = desc;
                    break;
                }
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
        let (Some(out_ch), Some(in_ch)) = (
            self.periph.gdma.out_channel_for(6),
            self.periph.gdma.in_channel_for(6),
        ) else {
            self.periph.aes.state = 2;
            self.periph.aes.int_raw |= 1;
            self.irq_dirty = true;
            return;
        };
        // gather input
        let mut input = Vec::new();
        let mut desc = self.periph.gdma.out[out_ch].desc;
        while desc != 0 {
            let dw0 = self.read32(desc).unwrap_or(0);
            let (len, buf, next) = (
                ((dw0 >> 12) & 0xfff) as usize,
                self.read32(desc + 4).unwrap_or(0),
                self.read32(desc + 8).unwrap_or(0),
            );
            for i in 0..len {
                input.push(self.read8(buf + i as u32).unwrap_or(0));
            }
            let eof = dw0 & (1 << 30) != 0;
            let _ = self.write32(desc, dw0 & !(1 << 31)); // hand the descriptor back
            if eof {
                self.periph.gdma.out[out_ch].int_raw |= (1 << 0) | (1 << 1);
                self.periph.gdma.out[out_ch].eof_desc = desc;
                break;
            }
            desc = next;
        }
        if std::env::var("ESP_EMU_DEBUG_AES").is_ok() {
            eprintln!(
                "[aes] dma block_mode={} num_blocks={} mode={} bytes={}",
                self.periph.aes.block_mode,
                self.periph.aes.num_blocks,
                self.periph.aes.mode,
                input.len()
            );
        }
        // transform (ECB and CBC cover what the crypto libraries ask for here)
        let key = self.periph.aes.key_bytes();
        let decrypt = self.periph.aes.decrypting();
        let block_mode = self.periph.aes.block_mode;
        let mut iv = [0u8; 16];
        for (i, w) in self.periph.aes.iv.iter().enumerate() {
            iv[4 * i..4 * i + 4].copy_from_slice(&w.to_le_bytes());
        }
        let mut output = Vec::with_capacity(input.len());
        for chunk in input.chunks(16) {
            let mut b = [0u8; 16];
            b[..chunk.len()].copy_from_slice(chunk);
            let cipher_in = b;
            let o = match block_mode {
                1 => {
                    // CBC
                    if !decrypt {
                        for i in 0..16 {
                            b[i] ^= iv[i];
                        }
                    }
                    let mut o = crate::crypto::aes_block(&key, &b, decrypt);
                    if decrypt {
                        for i in 0..16 {
                            o[i] ^= iv[i];
                        }
                        iv = cipher_in;
                    } else {
                        iv = o;
                    }
                    o
                }
                2 => {
                    // OFB: keystream feeds itself
                    let ks = crate::crypto::aes_block(&key, &iv, false);
                    iv = ks;
                    let mut o = [0u8; 16];
                    for i in 0..16 {
                        o[i] = b[i] ^ ks[i];
                    }
                    o
                }
                3 => {
                    // CTR: encrypt the counter, then bump it
                    let ks = crate::crypto::aes_block(&key, &iv, false);
                    let mut o = [0u8; 16];
                    for i in 0..16 {
                        o[i] = b[i] ^ ks[i];
                    }
                    for i in (0..16).rev() {
                        iv[i] = iv[i].wrapping_add(1);
                        if iv[i] != 0 {
                            break;
                        }
                    }
                    o
                }
                _ => crate::crypto::aes_block(&key, &b, decrypt), // ECB
            };
            output.extend_from_slice(&o);
            self.periph.aes.blocks += 1;
        }
        for (i, w) in iv.chunks(4).enumerate() {
            self.periph.aes.iv[i] = u32::from_le_bytes([w[0], w[1], w[2], w[3]]);
        }
        // scatter the result
        let mut pos = 0usize;
        let mut desc = self.periph.gdma.inp[in_ch].desc;
        while desc != 0 && pos < output.len() {
            let dw0 = self.read32(desc).unwrap_or(0);
            let (size, buf, next) = (
                (dw0 & 0xfff) as usize,
                self.read32(desc + 4).unwrap_or(0),
                self.read32(desc + 8).unwrap_or(0),
            );
            let n = size.min(output.len() - pos);
            for i in 0..n {
                let _ = self.write8(buf + i as u32, output[pos + i]);
            }
            pos += n;
            let ndw0 = (dw0 & !(0xfff << 12) & !(1 << 31)) | ((n as u32) << 12) | (1 << 30);
            let _ = self.write32(desc, ndw0);
            self.periph.gdma.inp[in_ch].eof_desc = desc;
            self.periph.gdma.inp[in_ch].int_raw |= (1 << 0) | (1 << 1);
            if next == 0 {
                break;
            }
            desc = next;
        }
        self.periph.aes.state = 2; // DONE
        self.periph.aes.int_raw |= 1;
        self.irq_dirty = true;
    }

    /// WiFi MAC transmit: fetch the queued frames from their DMA descriptors and complete them.
    fn wifi_tx_step(&mut self) {
        let pending = std::mem::take(&mut self.periph.wifi.tx_pending);
        for (slot, desc) in pending {
            let dw0 = self.read32(desc).unwrap_or(0);
            let pkt = self.read32(desc + 4).unwrap_or(0);
            let len = ((dw0 >> 12) & 0xfff) as usize;
            let mut frame = Vec::with_capacity(len);
            for i in 0..len {
                frame.push(self.read8(pkt + i as u32).unwrap_or(0));
            }
            if self.periph.wifi.log || std::env::var("ESP_EMU_DEBUG_WIFI_FRAMES").is_ok() {
                eprintln!(
                    "[wifi] TX slot {} desc {:#010x} pkt {:#010x} {}",
                    slot,
                    desc,
                    pkt,
                    crate::wifi::describe(&frame)
                );
            }
            self.periph.wifi.tx_done(slot);
            self.irq_dirty = true;
            let now_us = self.cycles / (crate::periph::CPU_HZ / 1_000_000);
            if let Some(ap) = &mut self.periph.wifi.ap {
                if let Some(data) = ap.on_station_tx(&frame, now_us) {
                    if let Some(eth) = crate::wifi::data_to_eth(&data) {
                        self.periph.wifi.eth_tx.push(eth);
                    }
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
        // (has_data cleared), which is what a real radio sees at low traffic, and never deliver two
        // frames closer than a frame's airtime.
        if now_us.wrapping_sub(self.periph.wifi.last_rx_us) < 400 {
            return;
        }
        // ... but if software stops recycling altogether, don't stall the air forever: after 50 ms
        // the frame is dropped, exactly as a real ring would overflow.
        let busy = {
            let d = self.periph.wifi.last_rx_desc;
            d != 0 && self.read32(d).unwrap_or(0) & (1 << 30) != 0
        };
        if busy && now_us.wrapping_sub(self.periph.wifi.last_rx_us) < 50_000 {
            return;
        }
        let mut due = match self.periph.wifi.ap.as_mut() {
            Some(ap) => ap.step(now_us),
            None => return,
        };
        let eth_in = std::mem::take(&mut self.periph.wifi.eth_rx);
        for e in eth_in {
            if let Some(ap) = self.periph.wifi.ap.as_mut() {
                if let Some(f) = ap.data_from_ds(&e) {
                    due.push(crate::wifi::AirFrame {
                        at_us: now_us,
                        frame: f,
                    });
                }
            }
        }
        if due.is_empty() {
            return;
        }
        // management responses (auth, assoc, probe) go before beacons: a connect exchange must not be
        // crowded out by beacon traffic
        due.sort_by_key(|a| (crate::wifi::is_beacon(&a.frame), a.at_us));
        let first = due.remove(0);
        self.wifi_rx_deliver(&first.frame, now_us);
        self.periph.wifi.last_rx_us = now_us;
        if let Some(ap) = &mut self.periph.wifi.ap {
            for a in due {
                ap.queue.push(a);
            }
        }
    }

    /// Write one received frame into the next RX descriptor (rx_ctrl header + frame + FCS) and raise the RX event.
    fn wifi_rx_deliver(&mut self, frame: &[u8], now_us: u64) {
        let desc = self.periph.wifi.rx_next | crate::periph::DMA_ADDR_BASE;
        if desc == 0 {
            self.periph.wifi.rx_dropped += 1;
            return;
        }
        let dw0 = self.read32(desc).unwrap_or(0);
        let buf = self.read32(desc + 4).unwrap_or(0);
        let next = self.read32(desc + 8).unwrap_or(0);
        let size = (dw0 & 0xfff) as usize;
        let total = 48 + frame.len() + 4;
        if dw0 & (1 << 31) == 0 || buf == 0 || size < total {
            self.periph.wifi.rx_dropped += 1;
            return;
        }
        let (chan, log) = match self.periph.wifi.ap.as_ref() {
            Some(ap) => (ap.cfg.channel as u32, ap.log),
            None => return,
        };
        let mut b = Vec::with_capacity(total);
        // rx_ctrl word 0 (silicon: a real broadcast beacon reads 0x111b20ad, bit 28 set, signed rssi in the low
        // byte). The MAC has already address-filtered, so every delivered frame is "for us"; use the same flags
        // for unicast and broadcast (an invented "filter_match" nibble made the blob discard unicast frames).
        // filter-match nibble (silicon: broadcast beacon reads bit 28). A frame the hardware accepted because
        // addr1 is our unicast MAC must carry the unicast-match bit (29), not the broadcast bit (28), or
        // wDev_IndicateFrame drops it as "not for me".
        let bcast = frame.len() >= 5 && frame[4] & 1 == 1;
        // filter-match nibble: bit 28 is the "accepted by the address filter" bit the blob's RX path
        // requires (silicon: a broadcast beacon reads 0x111b20ad); unicast frames add bit 29.
        let fm = if bcast {
            1u32 << 28
        } else {
            (1u32 << 28) | (1u32 << 29)
        };
        let w0: u32 = fm | (0xd8u32 & 0xff); // rssi -40 dBm, 1 Mbps, legacy
        let w2: u32 = (chan << 16) | (chan << 20); // channel, secondary
        let w5: u32 = 0xa6; // noise floor -90
        let w11: u32 = (frame.len() + 4) as u32 & 0xfff; // sig_len (incl. FCS), rx_state OK
        for w in [w0, 0, w2, now_us as u32, 0, w5, 0, 0, 0, 0, 0, w11] {
            b.extend_from_slice(&w.to_le_bytes());
        }
        b.extend_from_slice(frame);
        b.extend_from_slice(&crate::wifi::fcs(frame).to_le_bytes());
        let mut i = 0usize;
        while i + 4 <= b.len() {
            let v = u32::from_le_bytes([b[i], b[i + 1], b[i + 2], b[i + 3]]);
            let _ = self.write32(buf + i as u32, v);
            i += 4;
        }
        while i < b.len() {
            let _ = self.write8(buf + i as u32, b[i]);
            i += 1;
        }
        let ndw0 = (dw0 & !(0xfff << 12)) | ((total as u32) << 12) | (1 << 30) | (1 << 31); // length; owner AND has_data set (verified on silicon 2026-08-25: dw0=0xc0..)
        let _ = self.write32(desc, ndw0);
        let w = &mut self.periph.wifi;
        w.rx_last = (desc & 0xf_ffff) | (1 << 24);
        w.rx_next = next & 0xf_ffff;
        w.last_rx_desc = desc;
        w.rx_frames += 1;
        w.events |= (1 << 14) | (1 << 24); // RX data (wDev_ProcessFiq tests 0x1004000)   // registers hold masked descriptor addrs; rx_last has a 0x01 prefix (silicon)
        if log {
            let d = crate::wifi::describe(frame);
            if d.contains("auth") || d.contains("assoc") {
                eprintln!(
                    "[wifi] RX AUTH/ASSOC -> desc {:#010x} buf {:#010x} {}",
                    desc, buf, d
                );
            } else {
                eprintln!("[wifi] RX -> desc {:#010x} {}", desc, d);
            }
        }
        self.irq_dirty = true;
    }

    pub fn load_bytes(&mut self, addr: u32, data: &[u8]) -> Result<(), String> {
        for (i, b) in data.iter().enumerate() {
            let a = addr.wrapping_add(i as u32);
            let Some(e) = self.lookup(a) else {
                return Err(format!("load: address {:#010x} not mapped", a));
            };
            let o = e.off as usize + (a - e.lo) as usize;
            self.buf_mut(e.src as u8)[o] = *b;
            self.bump(e.vbase, o - e.off as usize, 1);
        }
        Ok(())
    }
}

impl Bus for SocBus {
    fn read8(&mut self, addr: u32) -> Result<u8, Fault> {
        if Self::is_periph(addr) {
            return Ok(self.periph_read(addr, 1) as u8);
        }
        let Some(e) = self.lookup(addr) else {
            self.last_fault = Some((addr, false));
            return Err(Fault::Unmapped);
        };
        Ok(self.buf(e.src as u8)[e.off as usize + (addr - e.lo) as usize])
    }
    fn read16(&mut self, addr: u32) -> Result<u16, Fault> {
        if Self::is_periph(addr) {
            return Ok(self.periph_read(addr, 2) as u16);
        }
        match self.lookup(addr) {
            Some(e) if addr.wrapping_add(2) <= e.hi => {
                let o = e.off as usize + (addr - e.lo) as usize;
                Ok(u16::from_le_bytes(
                    self.buf(e.src as u8)[o..o + 2]
                        .try_into()
                        .expect("the mapped two-byte read has the required width"),
                ))
            }
            Some(_) => Ok(u16::from_le_bytes([
                self.read8(addr)?,
                self.read8(addr + 1)?,
            ])), // straddles a page
            None => {
                self.last_fault = Some((addr, false));
                Err(Fault::Unmapped)
            }
        }
    }
    fn read32(&mut self, addr: u32) -> Result<u32, Fault> {
        if Self::is_periph(addr) {
            return Ok(self.periph_read(addr, 4));
        }
        match self.lookup(addr) {
            Some(e) if addr.wrapping_add(4) <= e.hi => {
                let o = e.off as usize + (addr - e.lo) as usize;
                Ok(u32::from_le_bytes(
                    self.buf(e.src as u8)[o..o + 4]
                        .try_into()
                        .expect("the mapped four-byte read has the required width"),
                ))
            }
            Some(_) => Ok(u32::from_le_bytes([
                self.read8(addr)?,
                self.read8(addr + 1)?,
                self.read8(addr + 2)?,
                self.read8(addr + 3)?,
            ])),
            None => {
                self.last_fault = Some((addr, false));
                Err(Fault::Unmapped)
            }
        }
    }
    fn write8(&mut self, addr: u32, v: u8) -> Result<(), Fault> {
        if Self::is_periph(addr) {
            self.periph_write(addr, v as u32, 1);
            return Ok(());
        }
        match self.lookup(addr) {
            Some(e) if e.writable != 0 => {
                let rel = (addr - e.lo) as usize;
                self.buf_mut(e.src as u8)[e.off as usize + rel] = v;
                self.bump(e.vbase, rel, 1);
                Ok(())
            }
            _ => {
                self.last_fault = Some((addr, true));
                Err(Fault::Prohibited)
            }
        }
    }
    fn write16(&mut self, addr: u32, v: u16) -> Result<(), Fault> {
        if Self::is_periph(addr) {
            self.periph_write(addr, v as u32, 2);
            return Ok(());
        }
        match self.lookup(addr) {
            Some(e) if e.writable != 0 && addr.wrapping_add(2) <= e.hi => {
                let rel = (addr - e.lo) as usize;
                let o = e.off as usize + rel;
                self.buf_mut(e.src as u8)[o..o + 2].copy_from_slice(&v.to_le_bytes());
                self.bump(e.vbase, rel, 2);
                Ok(())
            }
            Some(e) if e.writable != 0 => {
                let b = v.to_le_bytes();
                self.write8(addr, b[0])?;
                self.write8(addr + 1, b[1])
            }
            _ => {
                self.last_fault = Some((addr, true));
                Err(Fault::Prohibited)
            }
        }
    }
    fn write32(&mut self, addr: u32, v: u32) -> Result<(), Fault> {
        if Self::is_periph(addr) {
            self.periph_write(addr, v, 4);
            return Ok(());
        }
        match self.lookup(addr) {
            Some(e) if e.writable != 0 && addr.wrapping_add(4) <= e.hi => {
                let rel = (addr - e.lo) as usize;
                let o = e.off as usize + rel;
                self.buf_mut(e.src as u8)[o..o + 4].copy_from_slice(&v.to_le_bytes());
                self.bump(e.vbase, rel, 4);
                Ok(())
            }
            Some(e) if e.writable != 0 => {
                let b = v.to_le_bytes();
                for i in 0..4 {
                    self.write8(addr + i, b[i as usize])?;
                }
                Ok(())
            }
            _ => {
                self.last_fault = Some((addr, true));
                Err(Fault::Prohibited)
            }
        }
    }
    fn fetch(&mut self, pc: u32) -> Result<[u8; 4], Fault> {
        let Some(e) = self.lookup(pc) else {
            self.last_fault = Some((pc, false));
            return Err(Fault::Unmapped);
        };
        let o = e.off as usize + (pc - e.lo) as usize;
        let b = self.buf(e.src as u8);
        if let Some(w) = b.get(o..o + 4) {
            return Ok(w
                .try_into()
                .expect("the requested four-byte fetch slice has the required width"));
        }
        // last bytes of a buffer (or of a mapped page): what physical memory has, zero beyond
        let mut r = [0u8; 4];
        for (i, byte) in r.iter_mut().enumerate() {
            if let Some(x) = b.get(o + i) {
                *byte = *x;
            }
        }
        Ok(r)
    }
    #[inline(always)]
    fn page_versions(&self) -> &[u32] {
        &self.page_ver
    }
    #[inline(always)]
    fn note_pc(&mut self, pc: u32) {
        self.periph.cur_pc = pc;
    }
    fn fast_mem(&mut self) -> Option<FastMem> {
        Some(FastMem {
            tlb: self.tlb.as_ptr(),
            page_ver: self.page_ver.as_mut_ptr(),
        })
    }
    #[inline(always)]
    fn block_break(&self) -> bool {
        self.irq_dirty
    }
    fn code_page(&mut self, pc: u32) -> u32 {
        match self.lookup(pc) {
            Some(e) => e.vbase + ((pc - e.lo) >> VPAGE_SHIFT),
            None => self.page_ver.len() as u32 - 1,
        }
    }
    /// Returns 1 when device models actually ran (so interrupt lines may have changed), else 0.
    fn tick(&mut self, cycles: u32) -> u32 {
        self.cycles += cycles as u64;
        self.tick_pending += cycles;
        if self.tick_pending < self.tick_budget {
            return 0;
        }
        self.flush_ticks();
        1
    }
}

impl SocBus {
    fn refresh_tick_budget(&mut self) {
        let mut budget = self.periph.cycles_until_timer().clamp(1, MAX_TICK_DEFER);
        if let Some(deadline) = self.board.next_deadline() {
            let until_deadline = deadline
                .saturating_sub(self.cycles)
                .clamp(1, MAX_TICK_DEFER as u64);
            budget = budget.min(until_deadline as u32);
        }
        self.tick_budget = budget;
    }

    /// Deliver the deferred cycles to the device models now.
    pub fn flush_ticks(&mut self) {
        let c = std::mem::take(&mut self.tick_pending);
        if c == 0 {
            return;
        }
        self.tick_impl(c);
        self.refresh_tick_budget();
    }

    fn tick_impl(&mut self, cycles: u32) -> u32 {
        self.periph.tick(cycles as u64);
        match self.board.advance_to(self.cycles) {
            Ok(()) => {}
            Err(fault) => {
                self.board_deadline_fault = Some(fault);
                return 0;
            }
        }
        self.deliver_board_edges();
        self.deliver_spi2_dma_completion();
        self.dma_i2s_step(cycles as u64);
        self.dma_cam_step(cycles as u64);
        self.dma_lcd_step(cycles as u64);
        if !self.periph.wifi.tx_pending.is_empty() {
            self.wifi_tx_step();
        }
        if self.periph.aes.dma_pending {
            self.aes_dma_step();
        }
        if self.periph.sha.dma_pending {
            self.sha_dma_step();
        }
        if self.periph.wifi.ap.is_some() {
            self.wifi_air_step();
        }
        if let Some(net) = self.periph.wifi.net.as_mut() {
            let now_us = self.cycles / (crate::periph::CPU_HZ / 1_000_000);
            let out = std::mem::take(&mut self.periph.wifi.eth_tx);
            // Frames from the station are handled the moment they are sent, but reading the host
            // sockets means syscalls: doing that every scheduling round costs more than emulating
            // the CPU. NET_POLL_US is well under any timeout the guest's TCP stack cares about.
            const NET_POLL_US: u64 = 500;
            let due = now_us.wrapping_sub(self.periph.wifi.net_polled_us) >= NET_POLL_US;
            if !out.is_empty() || due {
                if due {
                    self.periph.wifi.net_polled_us = now_us;
                }
                let mut replies = Vec::new();
                for e in out {
                    replies.extend(net.handle(&e, now_us));
                }
                replies.extend(net.poll(now_us));
                self.periph.wifi.eth_rx.extend(replies);
            }
        }
        if !self.periph.gpio.changes.is_empty() {
            let ch = std::mem::take(&mut self.periph.gpio.changes);
            self.board.gpio_changes(&ch);
        }
        if !self.periph.rmt.done.is_empty() {
            for (ch, bits) in std::mem::take(&mut self.periph.rmt.done) {
                self.board.rmt_frame(ch, &bits);
            }
            self.irq_dirty = true;
        }
        0
    }
}

impl MeasuredBus for SocBus {
    fn measured_fetch(&self, pc: u32) -> Result<[u8; 4], Fault> {
        let Some((source, offset)) = self.measured_mapping(pc) else {
            return Err(Fault::Unmapped);
        };
        let bytes = self.buf(source);
        let mut result = [0u8; 4];
        for (index, byte) in result.iter_mut().enumerate() {
            if let Some(source_byte) = bytes.get(offset + index) {
                *byte = *source_byte;
            }
        }
        Ok(result)
    }

    fn measured_memory_class(&self, address: u32) -> MemoryClass {
        if Self::is_periph(address) {
            return MemoryClass::Mmio;
        }
        match self.measured_mapping(address).map(|(source, _)| source) {
            Some(SRC_SRAM) => MemoryClass::InternalSram,
            Some(SRC_IROM | SRC_DROM) => MemoryClass::MaskRom,
            Some(SRC_FLASH) => MemoryClass::Flash,
            Some(SRC_PSRAM) => MemoryClass::Psram,
            Some(SRC_RTC_FAST | SRC_RTC_SLOW) => MemoryClass::Rtc,
            _ => MemoryClass::Unknown,
        }
    }
}

#[cfg(test)]
mod gp_spi_board_tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    struct ProbeBoard {
        events: Arc<Mutex<Vec<String>>>,
    }
    impl backend_api::DeadlineModel for ProbeBoard {
        fn next_deadline(&self) -> Option<backend_api::VirtualCycle> {
            None
        }

        fn advance_to(
            &mut self,
            _cycle: backend_api::VirtualCycle,
        ) -> Result<(), backend_api::DeadlineError> {
            Ok(())
        }
    }
    impl crate::board::BoardModel for ProbeBoard {
        fn name(&self) -> &'static str {
            "probe"
        }
        fn gpio_changes(&mut self, changes: &[(u8, bool)]) {
            self.events
                .lock()
                .unwrap()
                .push(format!("gpio:{changes:?}"));
        }
        fn spi_transfer(&mut self, host: u8, tx: &[u8], rx_len: usize) -> Vec<u8> {
            self.events
                .lock()
                .unwrap()
                .push(format!("spi:{host}:{tx:02x?}:{rx_len}"));
            vec![0x5a; rx_len]
        }
    }

    #[test]
    fn board_answers_before_usr_write_returns_and_after_pending_gpio_edges() {
        const SPI2: u32 = 0x6002_4000;
        let events = Arc::new(Mutex::new(Vec::new()));
        let mut bus = SocBus::new(1024, 1024, [0; 6]);
        bus.board = Box::new(ProbeBoard {
            events: events.clone(),
        });
        bus.periph.gpio.changes.push((12, false));

        bus.write32(SPI2 + 0x10, (1 << 31) | (1 << 28)).unwrap();
        bus.write32(SPI2 + 0x18, (7 << 28) | 0x9f).unwrap();
        bus.write32(SPI2 + 0x20, 7).unwrap();
        bus.write32(SPI2, 1 << 24).unwrap();

        assert_eq!(bus.periph.spi2.w[0] & 0xff, 0x5a);
        assert_ne!(bus.periph.spi2.int_raw & (1 << 12), 0);
        assert_eq!(
            &*events.lock().unwrap(),
            &["gpio:[(12, false)]", "spi:2:[9f]:1"]
        );
    }

    #[test]
    fn spi2_data_phase_comes_from_gdma_descriptor() {
        const SPI2: u32 = 0x6002_4000;
        const DESC: u32 = 0x3fc9_0100;
        const DATA: u32 = 0x3fc9_0200;
        let events = Arc::new(Mutex::new(Vec::new()));
        let mut bus = SocBus::new(1024, 1024, [0; 6]);
        bus.board = Box::new(ProbeBoard {
            events: events.clone(),
        });
        bus.write32(DATA, 0x4433_2211).unwrap();
        bus.write32(DESC, 4 | (4 << 12) | (1 << 30) | (1 << 31))
            .unwrap();
        bus.write32(DESC + 4, DATA).unwrap();
        bus.write32(DESC + 8, 0).unwrap();
        bus.periph.gdma.out[0].conf0 = 1 << 2;
        bus.periph.gdma.out[0].peri_sel = 0;
        bus.periph.gdma.out[0].desc = DESC;
        bus.periph.gdma.out[0].running = true;

        bus.write32(SPI2 + 0x10, 1 << 27).unwrap();
        bus.write32(SPI2 + 0x1c, 31).unwrap();
        bus.write32(SPI2, 1 << 24).unwrap();

        assert_eq!(&*events.lock().unwrap(), &["spi:2:[11, 22, 33, 44]:0"]);
        assert_eq!(bus.read32(DESC).unwrap() >> 31, 0);
        assert_eq!(bus.periph.gdma.out[0].int_raw & 0xb, 0xb);
    }

    #[test]
    fn waveshare_spi2_gdma_completion_waits_for_the_receipt_deadline() {
        const SPI2: u32 = 0x6002_4000;
        const FIRST_DESC: u32 = 0x3fc9_0100;
        const DATA: u32 = 0x3fca_0000;
        const SUBMITTED_AT: u64 = 17;
        const TRANSFER_BYTES: usize = 32_768;

        let mut bus = SocBus::new(1024, 1024, [0; 6]);
        bus.board = Box::new(crate::board::WaveshareAmoled18V2::new());
        bus.advance_measured_to(SUBMITTED_AT).unwrap();

        let mut remaining = TRANSFER_BYTES;
        let mut descriptor = FIRST_DESC;
        while remaining != 0 {
            let length = remaining.min(0xfff);
            remaining -= length;
            let eof = u32::from(remaining == 0) << 30;
            let next = if remaining == 0 { 0 } else { descriptor + 12 };
            bus.write32(
                descriptor,
                length as u32 | ((length as u32) << 12) | eof | (1 << 31),
            )
            .unwrap();
            bus.write32(descriptor + 4, DATA).unwrap();
            bus.write32(descriptor + 8, next).unwrap();
            descriptor = next;
        }
        bus.periph.gdma.out[0].conf0 = 1 << 2;
        bus.periph.gdma.out[0].peri_sel = 0;
        bus.periph.gdma.out[0].desc = FIRST_DESC;
        bus.periph.gdma.out[0].running = true;

        bus.write32(SPI2 + 0x0c, 1 << 12).unwrap();
        bus.write32(SPI2 + 0x10, (1 << 27) | (1 << 13)).unwrap();
        bus.write32(SPI2 + 0x1c, (TRANSFER_BYTES as u32 * 8) - 1)
            .unwrap();
        bus.write32(SPI2, 1 << 24).unwrap();

        let completion = SUBMITTED_AT + 401_589;
        assert_eq!(
            bus.last_spi2_dma_timing,
            Some(Spi2DmaTiming {
                submit_cycles: 5_755,
                completion_cycle: completion,
            })
        );
        assert_eq!(bus.periph.spi2.transfers, 0);
        assert_eq!(bus.periph.gdma.out[0].int_raw & 0xb, 0);

        bus.advance_measured_to(completion - 1).unwrap();
        assert_eq!(bus.periph.spi2.transfers, 0);
        assert_eq!(bus.periph.gdma.out[0].int_raw & 0xb, 0);

        bus.advance_measured_to(completion).unwrap();
        assert_eq!(bus.periph.spi2.transfers, 1);
        assert_ne!(bus.periph.spi2.int_raw & (1 << 12), 0);
        assert_eq!(bus.periph.gdma.out[0].int_raw & 0xb, 0xb);
        assert_eq!(bus.read32(FIRST_DESC).unwrap() >> 31, 0);
    }

    #[test]
    fn spi2_dma_descriptor_cycle_is_a_typed_fault() {
        const SPI2: u32 = 0x6002_4000;
        const DESC: u32 = 0x3fc9_0100;
        let mut bus = SocBus::new(1024, 1024, [0; 6]);
        bus.write32(DESC, 1 << 31).unwrap();
        bus.write32(DESC + 4, 0).unwrap();
        bus.write32(DESC + 8, DESC).unwrap();
        bus.periph.gdma.out[0].peri_sel = 0;
        bus.periph.gdma.out[0].desc = DESC;
        bus.periph.gdma.out[0].running = true;

        bus.write32(SPI2 + 0x10, 1 << 27).unwrap();
        bus.write32(SPI2 + 0x1c, 7).unwrap();
        bus.write32(SPI2, 1 << 24).unwrap();

        assert_eq!(
            bus.spi2_dma_fault,
            Some(DmaDescriptorFault::Cycle { descriptor: DESC })
        );
    }

    #[test]
    fn spi2_dma_descriptor_step_budget_is_a_typed_fault() {
        const SPI2: u32 = 0x6002_4000;
        const FIRST_DESC: u32 = 0x3fc9_0100;
        let mut bus = SocBus::new(1024, 1024, [0; 6]);
        for step in 0..=SPI2_DMA_DESCRIPTOR_STEP_BUDGET {
            let descriptor = FIRST_DESC + (step as u32) * 12;
            bus.write32(descriptor, 1 << 31).unwrap();
            bus.write32(descriptor + 4, 0).unwrap();
            bus.write32(descriptor + 8, descriptor + 12).unwrap();
        }
        bus.periph.gdma.out[0].peri_sel = 0;
        bus.periph.gdma.out[0].desc = FIRST_DESC;
        bus.periph.gdma.out[0].running = true;

        bus.write32(SPI2 + 0x10, 1 << 27).unwrap();
        bus.write32(SPI2 + 0x1c, 0x3ffff).unwrap();
        bus.write32(SPI2, 1 << 24).unwrap();

        assert_eq!(
            bus.spi2_dma_fault,
            Some(DmaDescriptorFault::StepBudgetExceeded {
                budget: SPI2_DMA_DESCRIPTOR_STEP_BUDGET,
            })
        );
    }

    #[test]
    fn amoled_touch_edge_reaches_gpio21_interrupt_logic() {
        let mut bus = SocBus::new(1024, 1024, [0; 6]);
        bus.board = Box::new(crate::board::WaveshareAmoled18V2::new());
        bus.periph.gpio.pin[21] = (2 << 7) | (1 << 13);

        bus.board.touch(100, 200, true);
        Bus::tick(&mut bus, 1);

        assert!(!bus.periph.gpio.level(21));
        assert!(bus.periph.gpio.irq());
        assert!(bus.irq_dirty);
    }
}
