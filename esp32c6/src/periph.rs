//! ESP32-C6 peripherals.
//!
//! The data-path IP is the C3's — UART, USB-Serial/JTAG, systimer, timer groups, GPIO, the SPI
//! flash controller, SHA/AES/RSA have the same register layouts — so those models come from
//! `esp-periph`. Written here: the address map, the interrupt matrix with its PLIC/INTPRI
//! front-ends, the L1 cache controller, the always-on LP blocks (reset cause, software reset,
//! RTC timer, store registers, watchdog), PCR, PMU and the RNG.

use emu_core::{ClockDomain, ClockTree};
use esp_periph::{device_set, mmio, Device, DeviceSet, Dispatch, Misc, RegRam, WriteEffect, NO_SOURCE};
use esp_periph::{Aes, Efuse, Gdma, Gpio, GpSpi, Rmt, Rsa, Sha, SpiMem, Systimer, TimerGroup, Uart, UsbSerialJtag};
use esp_periph::{RST_POWERON, RST_SW_CPU, RST_SW_SYS};

pub const CPU_HZ: u64 = 160_000_000;
pub const PERIPH_BASE: u32 = 0x6000_0000;
pub const PERIPH_END: u32 = 0x6010_0000;
/// The CPU-subsystem window: PLIC (machine 0x000, user 0x400) and CLINT (0x800, 0xc00).
pub const CPU_SUB_BASE: u32 = 0x2000_1000;
pub const CPU_SUB_END: u32 = 0x2000_2000;

/// Interrupt sources, numbered by the order of the `INTMTX_CORE0_*_MAP_REG` registers (which is
/// also `soc/interrupts.h`'s order on this chip). Only the sources we can assert are listed.
pub mod src {
    pub const LP_TIMER: usize = 7; pub const ZB_MAC: usize = 12; pub const PMU: usize = 13; pub const EFUSE: usize = 14;
    pub const LP_RTC_TIMER: usize = 15; pub const LP_WDT: usize = 18;
    /// software interrupts, raised by writing `INTPRI_CPU_INTR_FROM_CPU_n`: the FreeRTOS yield
    pub const FROM_CPU0: usize = 22;
    pub const GPIO: usize = 30; pub const MSPI: usize = 40; pub const I2S: usize = 41;
    pub const UART0: usize = 43; pub const UART1: usize = 44; pub const LEDC: usize = 45;
    pub const USB_SERIAL_JTAG: usize = 48; pub const RMT: usize = 49; pub const I2C_EXT0: usize = 50;
    pub const TG0_T0: usize = 51; pub const TG0_T1: usize = 52; pub const TG0_WDT: usize = 53;
    pub const TG1_T0: usize = 54; pub const TG1_T1: usize = 55; pub const TG1_WDT: usize = 56;
    pub const SYSTIMER_T0: usize = 57; pub const SYSTIMER_T1: usize = 58; pub const SYSTIMER_T2: usize = 59;
    pub const DMA_IN_CH0: usize = 66; pub const DMA_OUT_CH0: usize = 69; pub const GPSPI2: usize = 72;
    pub const AES: usize = 73; pub const SHA: usize = 74; pub const RSA: usize = 75; pub const ECC: usize = 76;
    pub const COUNT: usize = 77;
}

/// The interrupt matrix (`INTMTX`, 0x60010000): 77 peripheral sources, each mapped to one of the
/// 31 CPU interrupt lines.
pub struct IntMatrix { pub map: [u32; src::COUNT], ram: RegRam }
impl Default for IntMatrix { fn default() -> Self { Self::new() } }
impl IntMatrix {
    pub fn new() -> Self { IntMatrix { map: [0; src::COUNT], ram: RegRam::new() } }
    pub fn read(&self, off: u32) -> u32 {
        match off { 0x000..=0x130 => self.map[(off / 4) as usize], _ => self.ram.read(off) }
    }
    pub fn write(&mut self, off: u32, v: u32) {
        match off { 0x000..=0x130 => self.map[(off / 4) as usize] = v & 0x1f, _ => self.ram.write(off, v) }
    }
}
impl Device for IntMatrix {
    fn read(&mut self, off: u32) -> u32 { IntMatrix::read(self, off) }
    fn write(&mut self, off: u32, v: u32) -> WriteEffect { IntMatrix::write(self, off, v); WriteEffect::NONE }
}

/// The CPU interrupt controller: enable, type, priority and threshold per line, reachable as
/// the PLIC (0x20001000, what ESP-IDF drives) and as INTPRI (0x600C5000, the same state with the
/// C3's register order plus the four software-interrupt latches). A line is taken when it is
/// enabled, its priority is at or above the threshold, and `mstatus.MIE` is set; the CPU then
/// vectors to `mtvec + 4*line`. Level lines follow the source; edge lines latch until cleared.
pub struct Intc {
    pub enable: u32,
    pub int_type: u32,
    pub pri: [u32; 32],
    pub thresh: u32,
    /// latched edge-triggered lines
    pub edge_pending: u32,
    /// level lines asserted right now, recomputed when a source changes
    pub level: u32,
    /// all mapped lines asserted last time, for edge detection
    prev: u32,
    /// INTPRI_CPU_INTR_FROM_CPU_0..3
    pub sw_int: u32,
    ram: RegRam,
}
impl Default for Intc { fn default() -> Self { Self::new() } }
impl Intc {
    pub fn new() -> Self { Intc { enable: 0, int_type: 0, pri: [0; 32], thresh: 0, edge_pending: 0, level: 0, prev: 0, sw_int: 0, ram: RegRam::new() } }

    /// INTPRI register order.
    pub fn intpri_read(&self, off: u32) -> u32 {
        match off {
            0x00 => self.enable, 0x04 => self.int_type, 0x08 => self.level | self.edge_pending,
            0x0c..=0x88 => self.pri[((off - 0x0c) / 4) as usize], 0x8c => self.thresh,
            0x90..=0x9c => (self.sw_int >> ((off - 0x90) / 4)) & 1,
            0xa8 => 0,
            _ => self.ram.read(off),
        }
    }
    pub fn intpri_write(&mut self, off: u32, v: u32) {
        match off {
            0x00 => self.enable = v, 0x04 => self.int_type = v,
            0x0c..=0x88 => self.pri[((off - 0x0c) / 4) as usize] = v & 0xf, 0x8c => self.thresh = v & 0xf,
            0x90..=0x9c => { let b = (off - 0x90) / 4; if v & 1 != 0 { self.sw_int |= 1 << b } else { self.sw_int &= !(1 << b) } }
            0xa8 => self.edge_pending &= !v,
            _ => self.ram.write(off, v),
        }
    }
    /// PLIC register order: enable, type, clear, EIP status, 32 priorities, then the threshold at
    /// 0x90 (the interrupt handler raises it to the taken line's priority + 1 before enabling
    /// nesting) and the claim register. The user-level copy at 0x400 is accepted and ignored.
    pub fn plic_read(&self, off: u32) -> u32 {
        match off {
            0x00 => self.enable, 0x04 => self.int_type, 0x08 => 0, 0x0c => self.level | self.edge_pending,
            0x10..=0x8c => self.pri[((off - 0x10) / 4) as usize], 0x90 => self.thresh,
            0x94 => self.pending().unwrap_or(0),          // CLAIM: the line being taken
            _ => self.ram.read(off),
        }
    }
    pub fn plic_write(&mut self, off: u32, v: u32) {
        match off {
            0x00 => self.enable = v, 0x04 => self.int_type = v, 0x08 => self.edge_pending &= !v,
            0x10..=0x8c => self.pri[((off - 0x10) / 4) as usize] = v & 0xf, 0x90 => self.thresh = v & 0xff,
            _ => self.ram.write(off, v),
        }
    }

    /// Recompute line state from the sources that are currently asserted.
    pub fn update(&mut self, map: &[u32; src::COUNT], status: &[u32]) {
        let mut lines = 0u32;
        for (s, &line) in map.iter().enumerate() {
            if status[s / 32] & (1 << (s % 32)) == 0 { continue; }
            if line != 0 { lines |= 1 << line; }
        }
        // an edge line latches on the rising edge of its source and stays until cleared
        self.edge_pending |= lines & !self.prev & self.int_type;
        self.prev = lines;
        self.level = lines & !self.int_type;
    }

    /// The highest-priority line the CPU should take, if any.
    pub fn pending(&self) -> Option<u32> {
        let p = (self.level | self.edge_pending) & self.enable & !1;
        if p == 0 { return None; }
        // a line at exactly the threshold fires: IDF enables with thresh = 1 and allocates at priority 1
        let (mut best, mut best_pri) = (None, 0);
        for n in 1..32 {
            let pri = self.pri[n];
            if p & (1 << n) != 0 && pri >= self.thresh && pri > best_pri { best_pri = pri; best = Some(n as u32); }
        }
        best
    }
}
impl Device for Intc {
    fn read(&mut self, off: u32) -> u32 { Intc::intpri_read(self, off) }
    fn write(&mut self, off: u32, v: u32) -> WriteEffect { Intc::intpri_write(self, off, v); WriteEffect::NONE }
    fn irq_sources(&self) -> u64 { (self.sw_int & 0xf) as u64 }
}

/// The L1 cache controller. As on the C3 only the "operation finished" bits matter: the ROM and
/// the bootloader kick a sync/lock/preload and poll for done.
pub struct Cache { ram: RegRam }
impl Default for Cache { fn default() -> Self { Self::new() } }
impl Cache {
    pub fn new() -> Self { Cache { ram: RegRam::new() } }
    pub fn read(&self, off: u32) -> u32 {
        let v = self.ram.read(off);
        match off {
            0x02c => if v & (1 << 16) != 0 { v | (1 << 18) } else { v & !(1 << 18) },   // FREEZE_CTRL: DONE follows ENA
            0x088 => v | (1 << 2),                 // LOCK_CTRL: LOCK_DONE
            0x098 => v | (1 << 4),                 // SYNC_CTRL: SYNC_DONE
            0x0d8 => v | (1 << 1),                 // PRELOAD_CTRL: PRELOAD_DONE
            0x134 => v | (1 << 1),                 // AUTOLOAD_CTRL: AUTOLOAD_DONE
            0x3fc => 0x2207_0400,                  // DATE
            _ => v,
        }
    }
    pub fn write(&mut self, off: u32, v: u32) { self.ram.write(off, v); }
}
impl Device for Cache {
    fn read(&mut self, off: u32) -> u32 { Cache::read(self, off) }
    fn write(&mut self, off: u32, v: u32) -> WriteEffect { Cache::write(self, off, v); WriteEffect::NONE }
}

/// The always-on LP system blocks, one device: LP_CLKRST at 0x000 (reset cause, CPU reset),
/// LP_AON at 0x400 (the ten STORE registers, the software-reset bits), LP_WDT at 0x800, LP_TIMER
/// at 0xc00 (the RTC counter the app reads for wall time). All of it survives a CPU reset.
pub struct LpSys {
    pub ram: RegRam,
    pub reset_cause: u32,
    pub sw_reset: bool,
    /// LP_TIMER: RTC slow-clock ticks since power-on and the value latched by an UPDATE
    pub slow_ticks: u64,
    pub time_latch: u64,
}
impl Default for LpSys { fn default() -> Self { Self::new() } }
impl LpSys {
    pub const CLKRST: u32 = 0x000; pub const AON: u32 = 0x400; pub const WDT: u32 = 0x800; pub const TIMER: u32 = 0xc00;
    pub fn new() -> Self { LpSys { ram: RegRam::new(), reset_cause: RST_POWERON, sw_reset: false, slow_ticks: 0, time_latch: 0 } }
    fn request_reset(&mut self, cause: u32) { self.reset_cause = cause; self.sw_reset = true; }
    pub fn read(&self, off: u32) -> u32 {
        match off {
            0x010 => (self.ram.read(off) & !0x1f) | (self.reset_cause & 0x1f),   // LP_CLKRST_RESET_CAUSE
            0xc14 => self.time_latch as u32, 0xc18 => (self.time_latch >> 32) as u32,   // LP_TIMER_MAIN_BUF0
            0xc10 => self.ram.read(off) & !(1 << 28),                                   // MAIN_TIMER_UPDATE self-clears
            0x3fc | 0x7fc | 0xbfc | 0xffc => 0x2207_0400,
            _ => self.ram.read(off),
        }
    }
    pub fn write(&mut self, off: u32, v: u32) {
        match off {
            0x434 => { if v & (1 << 31) != 0 { self.request_reset(RST_SW_SYS); } self.ram.write(off, v & !(1 << 31)); }   // LP_AON_SYS_CFG.HPSYS_SW_RESET
            0x438 => { if v & (1 << 28) != 0 { self.request_reset(RST_SW_CPU); } self.ram.write(off, v & !(1 << 28)); }   // LP_AON_CPUCORE0_CFG.CPU_CORE0_SW_RESET
            0xc10 => { if v & (1 << 28) != 0 { self.time_latch = self.slow_ticks; } self.ram.write(off, v); }
            _ => self.ram.write(off, v),
        }
    }
}
impl Device for LpSys {
    fn read(&mut self, off: u32) -> u32 { LpSys::read(self, off) }
    fn write(&mut self, off: u32, v: u32) -> WriteEffect { LpSys::write(self, off, v); WriteEffect::NONE }
    fn clock(&self) -> Option<ClockDomain> { Some(ClockDomain::RtcSlow) }
    fn tick(&mut self, ticks: u64) { self.slow_ticks += ticks; }
}

/// The C6's SPI flash controller is the shared `SpiMem` plus a few status registers of its own:
/// the ROM's `SPI_init` waits for the AXI FIFOs to report empty (AXI_ERR_ADDR, 0x170) before
/// touching the flash.
pub struct SpiMemC6(pub SpiMem);
impl Device for SpiMemC6 {
    fn read(&mut self, off: u32) -> u32 {
        match off {
            0x170 => 0xfc00_0000,                  // ALL_AXI_TRANS_AFIFO_EMPTY and every AFIFO empty/idle bit
            _ => Device::read(&mut self.0, off),
        }
    }
    fn write(&mut self, off: u32, v: u32) -> WriteEffect { Device::write(&mut self.0, off, v) }
    fn debug(&mut self, on: bool) { Device::debug(&mut self.0, on) }
}

/// The analog I2C master (regi2c): two hosts, each a CTRL word — [7:0] slave block, [15:8]
/// register, [23:16] data, [24] write, [25] busy — through which the ROM, IDF and the PHY blob
/// reach the PLL, bias, ADC and RF trim registers. Written values read back per (block,
/// register), like the S3 model. Two things the blobs poll for must come back set:
/// `ANA_CONF0.BBPLL_CAL_DONE`, and the RF block's (0x63) status bits — its registers read as
/// 0xff until written, which is what the PHY's calibration loops wait for.
pub struct AnaMst { ram: RegRam, pub ana: std::collections::HashMap<u32, u8> }
impl Default for AnaMst { fn default() -> Self { Self::new() } }
impl AnaMst {
    pub fn new() -> Self { AnaMst { ram: RegRam::new(), ana: Default::default() } }
    fn ctrl_read(&self, off: u32) -> u32 {
        let c = self.ram.read(off);
        if c & (1 << 24) != 0 { return c & !(1 << 25); }
        let key = c & 0xffff;
        // the RF block (0x63): register 0 is the sigma-delta modulator status the PHY's
        // `wait_i2c_sdm_stable` polls for 0x5b; the other status registers read all-ones
        let d = *self.ana.get(&key).unwrap_or(match key { 0x0063 => &0x5b, k if k & 0xff == 0x63 => &0xff, _ => &0 }) as u32;
        (c & !(0xff << 16) & !(1 << 25)) | (d << 16)
    }
}
impl Device for AnaMst {
    fn read(&mut self, off: u32) -> u32 {
        match off {
            0x00 | 0x04 => self.ctrl_read(off),
            0x18 => self.ram.read(off) | (1 << 24),
            _ => self.ram.read(off),
        }
    }
    fn write(&mut self, off: u32, v: u32) -> WriteEffect {
        if (off == 0x00 || off == 0x04) && v & (1 << 24) != 0 { self.ana.insert(v & 0xffff, (v >> 16) as u8); }
        self.ram.write(off, v);
        WriteEffect::NONE
    }
}

/// ASSIST_DEBUG: the ROM enables PC recording at boot and, after a reset, prints the last
/// recorded PC as `Saved PC` — the instruction after the store that requested the reset.
pub struct AssistDebug { pub saved_pc: u32, ram: RegRam }
impl Default for AssistDebug { fn default() -> Self { Self::new() } }
impl AssistDebug {
    pub fn new() -> Self { AssistDebug { saved_pc: 0, ram: RegRam::new() } }
}
impl Device for AssistDebug {
    fn read(&mut self, off: u32) -> u32 { match off { 0x48 => self.saved_pc, _ => self.ram.read(off) } }   // CORE_0_RCD_PDEBUGPC
    fn write(&mut self, off: u32, v: u32) -> WriteEffect { self.ram.write(off, v); WriteEffect::NONE }
}

/// The C6's RMT: the S3's transmitter IP with two TX and two RX channels on a compacted register
/// map. The TX channels' CONF0 bits and the interrupt bits the model raises (TX_END n, TX_THR
/// 8+n) are the same, so this only maps offsets onto the shared model; the RX channels read back
/// what was written.
pub struct RmtC6 { pub rmt: Rmt, ram: RegRam }
impl RmtC6 {
    pub fn new(cpu_hz: u64) -> Self { RmtC6 { rmt: Rmt::new(cpu_hz), ram: RegRam::new() } }
    fn map(off: u32) -> Option<u32> {
        Some(match off {
            0x00..=0x0c => off,                                   // CHnDATA
            0x10 | 0x14 => 0x20 + (off - 0x10),                   // CH0/1 CONF0 (TX)
            0x28 | 0x2c => 0x50 + (off - 0x28),                   // CH0/1 STATUS
            0x38 => 0x70, 0x3c => 0x74, 0x40 => 0x78, 0x44 => 0x7c,   // INT_RAW / ST / ENA / CLR
            0x48 | 0x4c => 0x80 + (off - 0x48),                   // CH0/1 CARRIER_DUTY
            0x58 | 0x5c => 0xa0 + (off - 0x58),                   // CH0/1 TX_LIM
            0x68 => 0xc0, 0x6c => 0xc4, 0x70 => 0xc8,             // SYS_CONF, TX_SIM, REF_CNT_RST
            0x400..=0x6fc => 0x800 + (off - 0x400),               // symbol memory, 48 words per channel
            _ => return None,
        })
    }
}
impl Device for RmtC6 {
    fn read(&mut self, off: u32) -> u32 { match Self::map(off) { Some(o) => self.rmt.read(o), None => self.ram.read(off) } }
    fn write(&mut self, off: u32, v: u32) -> WriteEffect { match Self::map(off) { Some(o) => self.rmt.write(o, v), None => self.ram.write(off, v) } WriteEffect::NONE }
    fn irq_sources(&self) -> u64 { self.rmt.irq() as u64 }
    fn clock(&self) -> Option<ClockDomain> { Some(ClockDomain::Cpu) }
    fn tick(&mut self, cycles: u64) { self.rmt.tick(cycles) }
}

/// The C6's GDMA: three channels with the S3's per-channel registers but the interrupt registers
/// gathered at the front of the block (IN at 0x00 + 0x10n, OUT at 0x30 + 0x10n) and the channel
/// blocks starting at 0x70. Mapped onto the shared model; descriptors live in SRAM.
pub struct GdmaC6 { pub gdma: Gdma, ram: RegRam }
impl Default for GdmaC6 { fn default() -> Self { Self::new() } }
impl GdmaC6 {
    pub fn new() -> Self { let mut g = Gdma::new(); g.addr_base = 0x4080_0000; GdmaC6 { gdma: g, ram: RegRam::new() } }
    fn map(off: u32) -> Option<u32> {
        // the shared (S3) layout per channel: IN conf0 0x00, conf1 0x04, [int 0x08..0x14], fifo 0x18, pop 0x1c,
        // link 0x20, state 0x24, suc_eof 0x28, err_eof 0x2c, dscr 0x30, bf0 0x34, bf1 0x38, pri 0x44, peri_sel 0x48;
        // OUT the same at +0x60. The C6 orders a channel block conf0, conf1, fifo, push, link, state, eof, eof_bfr, dscr, bf0, bf1, pri, peri_sel.
        const T: [u32; 13] = [0x00, 0x04, 0x18, 0x1c, 0x20, 0x24, 0x28, 0x2c, 0x30, 0x34, 0x38, 0x44, 0x48];
        if off < 0x30 { return Some((off / 0x10) * 0xc0 + 0x08 + off % 0x10); }
        if off < 0x60 { let o = off - 0x30; return Some((o / 0x10) * 0xc0 + 0x68 + o % 0x10); }
        if off == 0x64 { return Some(0x3c8); }
        if (0x70..0x70 + 3 * 0xc0).contains(&off) {
            let rel = off - 0x70; let (n, k) = (rel / 0xc0, rel % 0xc0);
            if k <= 0x30 { return Some(n * 0xc0 + T[(k / 4) as usize]); }
            if (0x60..=0x90).contains(&k) { return Some(n * 0xc0 + 0x60 + T[((k - 0x60) / 4) as usize]); }
        }
        None
    }
}
impl Device for GdmaC6 {
    fn read(&mut self, off: u32) -> u32 { match Self::map(off) { Some(o) => self.gdma.read(o), None => self.ram.read(off) } }
    fn write(&mut self, off: u32, v: u32) -> WriteEffect { match Self::map(off) { Some(o) => self.gdma.write(o, v), None => self.ram.write(off, v) } WriteEffect::NONE }
    fn irq_sources(&self) -> u64 { Device::irq_sources(&self.gdma) }
    fn debug(&mut self, on: bool) { self.gdma.dbg = on; }
}

/// The IEEE 802.15.4 MAC, as far as an energy scan needs it: `ED_START` samples the channel for
/// the programmed number of symbols and raises `ED_DONE` with an RSSI in ED_SCAN_CFG. Nothing is
/// on the air, so the reading is a synthetic 2.4 GHz picture: a quiet floor with the three
/// non-overlapping WiFi channels sitting on top of it, plus noise. The picture moves the way a
/// real one does — every few seconds a network changes level, a short burst lands on some
/// channel and fades — deterministically, from the same xorshift as the noise. The board or a
/// script can set a level outright (`set_channel_dbm`).
pub struct Ieee802154 {
    ram: RegRam,
    pub events: u32, pub event_en: u32, pub freq: u32, pub duration: u32,
    pub ed_rss: i8, pub ed_left: Option<u64>, pub scans: u64,
    /// dBm per 802.15.4 channel 11..26, before noise
    pub level_dbm: [i8; 16],
    /// where each channel is drifting to, 1 dB per 100 ms
    pub target_dbm: [i8; 16],
    noise: u32, scene_acc: u64, scene_ticks: u64,
}
impl Default for Ieee802154 { fn default() -> Self { Self::new() } }
impl Ieee802154 {
    pub const CMD_ED_START: u32 = 0x44; pub const CMD_STOP: u32 = 0x45; pub const EVENT_ED_DONE: u32 = 1 << 6;
    pub fn new() -> Self {
        // WiFi channels 1, 6 and 11 overlap 802.15.4 channels 11-14, 16-19 and 21-24
        let mut level_dbm = [-93i8; 16];
        for (i, l) in [(0, -68), (1, -52), (2, -49), (3, -66), (5, -63), (6, -47), (7, -45), (8, -60), (10, -74), (11, -56), (12, -58), (13, -71), (15, -90)] { level_dbm[i] = l; }
        Ieee802154 { ram: RegRam::new(), events: 0, event_en: 0, freq: 3, duration: 0, ed_rss: -92, ed_left: None, scans: 0, level_dbm, target_dbm: level_dbm, noise: 0x9e37_79b9, scene_acc: 0, scene_ticks: 0 }
    }
    pub fn set_channel_dbm(&mut self, channel: u8, dbm: i8) { if (11..=26).contains(&channel) { let i = (channel - 11) as usize; self.level_dbm[i] = dbm; self.target_dbm[i] = dbm; } }
    fn rand(&mut self) -> u32 { self.noise ^= self.noise << 13; self.noise ^= self.noise >> 17; self.noise ^= self.noise << 5; self.noise }
    /// One 100 ms step of the scene: levels drift toward their targets; every 2.5 s something
    /// happens — a WiFi network moves (channel 1, 6 or 11: 802.15.4 channels 11-14, 16-19, 21-24)
    /// or a burst lands on one channel and is left to fade back to the floor.
    fn scene_step(&mut self) {
        self.scene_ticks += 1;
        if self.scene_ticks.is_multiple_of(25) {
            let r = self.rand();
            if r.is_multiple_of(4) {
                let ch = (self.rand() % 16) as usize; self.target_dbm[ch] = self.target_dbm[ch].max(-50 - (self.rand() % 12) as i8); self.level_dbm[ch] = self.target_dbm[ch];
            } else {
                let base = [0usize, 5, 10][(r / 4 % 3) as usize];
                let peak = -44 - (self.rand() % 28) as i8;
                for (k, drop) in [(0, 6), (1, 0), (2, 2), (3, 9)] { self.target_dbm[base + k] = peak - drop; }
            }
        }
        for i in 0..16 {
            let wifi = matches!(i, 0..=3 | 5..=8 | 10..=13);
            if !wifi && self.target_dbm[i] > -93 && self.scene_ticks.is_multiple_of(5) { self.target_dbm[i] -= 1; }   // a burst fades
            let (l, t) = (self.level_dbm[i], self.target_dbm[i]);
            self.level_dbm[i] = if l < t { l + 1 } else if l > t { l - 1 } else { l };
        }
    }
    /// the channel the frequency register selects: freq = 3 + 5 * (channel - 11)
    pub fn channel(&self) -> u8 { (11 + ((self.freq.saturating_sub(3)) / 5) as u8).clamp(11, 26) }
    fn sample(&mut self) -> i8 {
        let jitter = (self.rand() % 7) as i8 - 3;
        (self.level_dbm[(self.channel() - 11) as usize] as i16 + jitter as i16).clamp(-127, 0) as i8
    }
}
impl Device for Ieee802154 {
    fn read(&mut self, off: u32) -> u32 {
        match off {
            0x48 => self.freq, 0x50 => self.duration,
            0x54 => (self.ram.read(off) & !0x00ff_0000) | ((self.ed_rss as u8 as u32) << 16),   // ED_SCAN_CFG.ED_RSS
            0x60 => self.event_en, 0x64 => self.events,
            _ => self.ram.read(off),
        }
    }
    fn write(&mut self, off: u32, v: u32) -> WriteEffect {
        match off {
            0x00 => match v & 0xff {
                Self::CMD_ED_START => { self.ed_left = Some((self.duration.max(1) as u64) * 16 * (CPU_HZ / 1_000_000)); }   // symbols of 16 µs
                Self::CMD_STOP => self.ed_left = None,
                _ => {}
            },
            0x48 => self.freq = v & 0x7f, 0x50 => self.duration = v & 0xff_ffff,
            0x60 => self.event_en = v & 0x1fff, 0x64 => self.events &= !v,               // EVENT_STATUS: write 1 to clear
            _ => self.ram.write(off, v),
        }
        WriteEffect::NONE
    }
    fn irq_sources(&self) -> u64 { (self.events & self.event_en != 0) as u64 }
    fn clock(&self) -> Option<ClockDomain> { Some(ClockDomain::Cpu) }
    fn tick(&mut self, cycles: u64) {
        self.scene_acc += cycles;
        while self.scene_acc >= CPU_HZ / 10 { self.scene_acc -= CPU_HZ / 10; self.scene_step(); }
        if let Some(left) = self.ed_left {
            if cycles >= left { self.ed_left = None; self.ed_rss = self.sample(); self.scans += 1; self.events |= Self::EVENT_ED_DONE; } else { self.ed_left = Some(left - cycles); }
        }
    }
    fn has_deadline(&self) -> bool { true }
    fn next_deadline(&self) -> Option<u64> { Some(self.ed_left.unwrap_or(u64::MAX).min(CPU_HZ / 10 - self.scene_acc)) }
}

/// PCR: peripheral clock and reset control. Configuration reads back; the hardware-fixed clock
/// tree fields read their silicon values, because the clock code derives the current CPU
/// frequency from them (SOC_ROOT → HP_ROOT is a fixed ÷3 on the PLL path, XTAL is 40 MHz, the
/// PLL is 480 MHz) and asserts on a divider it cannot explain.
pub struct Pcr { ram: RegRam }
impl Default for Pcr { fn default() -> Self { Self::new() } }
impl Pcr {
    pub fn new() -> Self {
        let mut p = Pcr { ram: RegRam::new() };
        p.ram.write(0x11c, 0x300);          // AHB_FREQ_CONF: AHB_HS_DIV_NUM = 3
        p.ram.write(0x128, 0x1f);           // PLL_DIV_CLK_EN: every PLL-derived clock on
        p
    }
    pub fn read(&self, off: u32) -> u32 {
        let v = self.ram.read(off);
        match off {
            0x110 => (v & 0x00ff_0000) | 40 << 24 | 2 << 8,   // SYSCLK_CONF: CLK_XTAL_FREQ 40, HS_DIV_NUM 2, LS_DIV_NUM 0 (all HRO)
            0x114 => v | 0x5,                                  // CPU_WAITI_CONF: CPUPERIOD_SEL 1, PLL_FREQ_SEL 1 (HRO)
            0x124 => 480 << 8 | 20,                            // SYSCLK_FREQ_QUERY_0: PLL 480 MHz, FOSC 20 MHz
            0xffc => 0x2207_0400,
            _ => v,
        }
    }
}
impl Device for Pcr {
    fn read(&mut self, off: u32) -> u32 { Pcr::read(self, off) }
    fn write(&mut self, off: u32, v: u32) -> WriteEffect { self.ram.write(off, v); WriteEffect::NONE }
}

/// LPPERI + 0x8 is the hardware RNG (`WDEV_RND_REG` on this chip). Real silicon seeds it from
/// radio noise — xorshift is enough for the bootloader's stack canary and `esp_random`.
pub struct Rng { state: u32, pub now: u32 }
impl Device for Rng {
    fn read(&mut self, _off: u32) -> u32 {
        self.state ^= self.state << 13; self.state ^= self.state >> 17; self.state ^= self.state << 5;
        self.state.wrapping_add(self.now)
    }
    fn write(&mut self, _off: u32, _v: u32) -> WriteEffect { WriteEffect::NONE }
}

/// Seed the efuse block the way this C6 reads back (a Waveshare ESP32-C6-LCD-1.47, MAC
/// dc:1e:d5:6e:8c:dc, `hw/c6-efuse.txt`): wafer v0.1, package 1, block revision v0.3, embedded
/// flash present. BLK1 word 3 (bits 96..127) holds WAFER_VERSION_MINOR (bit 114, 4 bits),
/// WAFER_VERSION_MAJOR (118), PKG_VERSION (120), BLK_VERSION_MINOR (123), BLK_VERSION_MAJOR (126);
/// word 4 holds FLASH_CAP (128) and FLASH_VENDOR (133). The bootloader prints the wafer and block
/// revisions on every boot and refuses an app whose `min_chip_rev` is above the wafer version.
pub fn efuse_c6(mac: [u8; 6], rev_major: u32, rev_minor: u32, pkg: u32, blk_major: u32, blk_minor: u32) -> Efuse {
    let mut e = Efuse::new(mac);
    e.write(0x48, (mac[0] as u32) << 8 | mac[1] as u32 | 0xfffe << 16);       // BLK1 word 1: MAC high, MAC_EXT ff:fe
    e.write(0x50, (rev_minor & 0xf) << 18 | (rev_major & 3) << 22 | (pkg & 7) << 24 | (blk_minor & 7) << 27 | (blk_major & 3) << 30);
    e.write(0x54, 1 | 1 << 5);                                                 // FLASH_CAP = 1 (4 MB), FLASH_VENDOR = 1
    e.write(0x6c, 0);                                                          // (the S3 layout's BLK_VERSION_MAJOR lives elsewhere here)
    e
}

pub struct Peripherals {
    pub uart: [Uart; 2],
    pub usb: UsbSerialJtag,
    pub systimer: Systimer,
    pub timg: [TimerGroup; 2],
    pub gpio: Gpio,
    pub efuse: Efuse,
    pub spi0: SpiMemC6,
    pub spi1: SpiMemC6,
    pub sha: Sha,
    pub aes: Aes,
    pub rsa: Rsa,
    pub rmt: RmtC6,
    pub gdma: GdmaC6,
    pub spi2: GpSpi,
    pub radio: Ieee802154,
    pub intmtx: IntMatrix,
    pub intc: Intc,
    pub cache: Cache,
    pub lpsys: LpSys,
    pub pcr: Pcr,
    pub ana_mst: AnaMst,
    pub assist_debug: AssistDebug,
    pub rng: Rng,
    /// the CPU-subsystem window behind the PLIC: user-level PLIC and the CLINT, unmodelled
    pub cpu_sub: RegRam,
    /// register RAM behind unmodelled blocks, first-touch logging, pc attribution
    pub misc: Misc,
    pub spi_exec: bool,
    clock: ClockTree<4>,
    last_status: [u32; 4],
}

// Every peripheral, where it sits (4 KB block number from 0x60000000), and its interrupt sources.
device_set! { Peripherals; clock: (clock) CPU_HZ, [(ClockDomain::Systimer, 10), (ClockDomain::Apb, 2), (ClockDomain::RtcSlow, 1067), (ClockDomain::Cpu, 1)];
    0x00 "UART0" (uart[0]) => [src::UART0];
    0x01 "UART1" (uart[1]) => [src::UART1];
    0x02 "SPI0" (spi0) => [];
    0x03 "SPI1" (spi1) => [];
    0x06 "RMT" (rmt) => [src::RMT];
    0x08 "TIMG0" (timg[0]) => [src::TG0_T0];
    0x09 "TIMG1" (timg[1]) => [src::TG1_T0];
    0x0a "SYSTIMER" (systimer) => [src::SYSTIMER_T0, src::SYSTIMER_T1, src::SYSTIMER_T2];
    0x0f "USB_SERIAL_JTAG" (usb) => [src::USB_SERIAL_JTAG];
    0x10 "INTMTX" (intmtx) => [];
    // three channels; the model numbers its sources out 0..4 then in 0..4
    0x80 "GDMA" (gdma) => [src::DMA_OUT_CH0, src::DMA_OUT_CH0 + 1, src::DMA_OUT_CH0 + 2, NO_SOURCE, NO_SOURCE, src::DMA_IN_CH0, src::DMA_IN_CH0 + 1, src::DMA_IN_CH0 + 2, NO_SOURCE, NO_SOURCE];
    0x81 "SPI2" (spi2) => [src::GPSPI2];
    0x88 "AES" (aes) => [src::AES];
    0x89 "SHA" (sha) => [];
    0x8a "RSA" (rsa) => [src::RSA];
    0x91 "GPIO" (gpio) => [src::GPIO];
    0x96 "PCR" (pcr) => [];
    0xa3 "IEEE802154" (radio) => [src::ZB_MAC];
    0xaf "I2C_ANA_MST" (ana_mst) delta -0x800 @ 0x800..=0xfff => [];
    // the LP address space: PMU at 0xb0000 is generic; the four LP blocks below are one device
    0xb0 "LP_CLKRST" (lpsys) delta -0x400 @ 0x400..=0x7ff => [];
    0xb0 "EFUSE" (efuse) delta -0x800 @ 0x800..=0xbff => [];
    0xb0 "LP_TIMER" alias (lpsys) @ 0xc00..=0xfff => [];
    0xb1 "LP_AON" alias (lpsys) delta 0x400 @ 0x000..=0x3ff => [];
    0xb1 "LP_WDT" alias (lpsys) delta -0x400 @ 0xc00..=0xfff => [];
    0xb2 "LPPERI_RNG" (rng) @ 0x808..=0x80b => [];
    0xc2 "ASSIST_DEBUG" (assist_debug) => [];
    0xc5 "INTPRI" (intc) => [src::FROM_CPU0, src::FROM_CPU0 + 1, src::FROM_CPU0 + 2, src::FROM_CPU0 + 3];
    0xc8 "CACHE" (cache) => [];
}

impl DeviceSet for Peripherals {
    const BASE: u32 = PERIPH_BASE;
    fn block_name(block: u32) -> &'static str { Peripherals::block_name(block) }
    fn misc(&self) -> &Misc { &self.misc }
    fn misc_mut(&mut self) -> &mut Misc { &mut self.misc }
    fn pre_access(&mut self, block: u32, _off: u32, _write: bool) {
        if block == 0xb2 { self.rng.now = self.clock.cycles() as u32; }
    }
}

impl Peripherals {
    pub fn new(mac: [u8; 6]) -> Self {
        Peripherals {
            uart: [Uart::new(), Uart::new()], usb: UsbSerialJtag::new(CPU_HZ), systimer: Systimer::new(),
            timg: [TimerGroup::new(), TimerGroup::new()], gpio: Gpio::new(),
            efuse: efuse_c6(mac, 0, 1, 1, 0, 3),
            spi0: SpiMemC6({ let mut s = SpiMem::new(false); s.has_psram = false; s }),
            spi1: SpiMemC6({ let mut s = SpiMem::new(true); s.has_psram = false; s }),   // no PSRAM on the C6
            sha: Sha::new(), aes: Aes::new(), rsa: Rsa::new(),
            rmt: RmtC6::new(CPU_HZ), gdma: GdmaC6::new(), spi2: GpSpi::new(), radio: Ieee802154::new(),
            intmtx: IntMatrix::new(), intc: Intc::new(), cache: Cache::new(), lpsys: LpSys::new(), pcr: Pcr::new(), ana_mst: AnaMst::new(), assist_debug: AssistDebug::new(),
            rng: Rng { state: 0x2545_f491, now: 0 }, cpu_sub: RegRam::new(),
            misc: Misc::new(), spi_exec: false, clock: Self::new_clock(),
            last_status: [0; 4],
        }
    }

    pub fn block_name(block: u32) -> &'static str {
        match block {
            0x00 => "UART0", 0x01 => "UART1", 0x02 => "SPI0", 0x03 => "SPI1", 0x04 => "I2C0", 0x05 => "UHCI0",
            0x06 => "RMT", 0x07 => "LEDC", 0x08 => "TIMG0", 0x09 => "TIMG1", 0x0a => "SYSTIMER", 0x0b => "TWAI0",
            0x0c => "I2S", 0x0d => "TWAI1", 0x0e => "APB_SARADC", 0x0f => "USB_SERIAL_JTAG", 0x10 => "INTMTX",
            0x11 => "ATOMIC", 0x12 => "PCNT", 0x13 => "SOC_ETM", 0x14 => "MCPWM", 0x15 => "PARL_IO", 0x16 => "HINF",
            0x17 => "SLC", 0x18 => "SLCHOST", 0x19 => "PVT_MONITOR", 0x80 => "GDMA", 0x81 => "SPI2", 0x88 => "AES",
            0x89 => "SHA", 0x8a => "RSA", 0x8b => "ECC_MULT", 0x8c => "DS", 0x8d => "HMAC", 0x90 => "IO_MUX",
            0x91 => "GPIO", 0x92 => "MEM_MONITOR", 0x93 => "PAU", 0x95 => "HP_SYSTEM", 0x96 => "PCR", 0x98 => "TEE",
            0x99 => "HP_APM", 0x9f => "MISC", 0xa3 => "IEEE802154", 0xa9 => "MODEM_SYSCON", 0xaf => "I2C_ANA_MST", 0xb0 => "PMU/LP_CLKRST/EFUSE/LP_TIMER",
            0xb1 => "LP_AON/LP_UART/LP_I2C/LP_WDT", 0xb2 => "LP_IO/LP_I2C_ANA/LPPERI/LP_ANA_PERI",
            0xb3 => "LP_TEE/LP_APM/OTP_DEBUG", 0xc0 => "TRACE", 0xc2 => "ASSIST_DEBUG", 0xc5 => "INTPRI", 0xc8 => "CACHE",
            _ => "?",
        }
    }

    pub fn read32(&mut self, addr: u32) -> u32 { mmio::read32(self, addr) }

    pub fn write32(&mut self, addr: u32, v: u32) {
        if mmio::write32(self, addr, v).contains(WriteEffect::SPI_EXEC) { self.spi_exec = true; }
    }

    /// The CPU-subsystem window (0x20001000): the machine-level PLIC is the interrupt controller;
    /// the user-level PLIC and the CLINT read back what was written.
    pub fn cpu_sub_read(&mut self, off: u32) -> u32 {
        if off < 0x400 { self.intc.plic_read(off) } else { self.cpu_sub.read(off) }
    }
    pub fn cpu_sub_write(&mut self, off: u32, v: u32) {
        if off < 0x400 { self.intc.plic_write(off, v) } else { self.cpu_sub.write(off, v) }
    }

    /// Advance every clocked device by `cycles` CPU cycles (16 MHz systimer, 80 MHz APB-domain
    /// timers, the RTC slow clock), with delivered-tick accounting so a slow clock never drifts.
    pub fn tick(&mut self, cycles: u64) { Dispatch::tick(self, cycles); }

    /// Which interrupt sources are asserted right now.
    pub fn source_status(&self) -> [u32; 4] { Dispatch::source_status(self) }

    /// Refresh the interrupt matrix; returns true if any source changed.
    pub fn refresh_lines(&mut self) -> bool {
        let st = self.source_status();
        let changed = st != self.last_status;
        self.last_status = st;
        self.intc.update(&self.intmtx.map, &st);
        changed
    }
}
