//! ESP32-S3 peripherals (register level). Each device owns its register state;
//! `Peripherals` dispatches by 4 KiB block and logs first-touch unknown accesses.
use std::collections::HashSet;

pub const PERIPH_BASE: u32 = 0x6000_0000;
pub const PERIPH_END: u32 = 0x600D_0000;

// interrupt sources (soc/interrupts.h, with the enum's explicit gaps)
pub const SRC_GPIO: usize = 16;
pub const SRC_UART0: usize = 27;
pub const SRC_UART1: usize = 28;
pub const SRC_SPI2: usize = 21;
pub const SRC_PCNT: usize = 41;
pub const SRC_LCD_CAM: usize = 24;
pub const SRC_I2S0: usize = 25;
pub const SRC_I2S1: usize = 26;
pub const SRC_RMT: usize = 40;
pub const SRC_I2C0: usize = 42;
pub const SRC_I2C1: usize = 43;
pub const SRC_TG0_T0: usize = 50;
pub const SRC_TG0_WDT: usize = 52;
pub const SRC_TG1_T0: usize = 53;
pub const SRC_TG1_WDT: usize = 55;
pub const SRC_SYSTIMER_T0: usize = 57;
pub const SRC_SYSTIMER_T1: usize = 58;
pub const SRC_SYSTIMER_T2: usize = 59;
pub const SRC_DMA_IN_CH0: usize = 66;
pub const SRC_DMA_OUT_CH0: usize = 71;
pub const SRC_FROM_CPU0: usize = 79;
pub const SRC_USB_SERIAL_JTAG: usize = 96;
pub const NUM_SOURCES: usize = 99;

pub const CPU_HZ: u64 = 240_000_000;
pub const APB_HZ: u64 = 80_000_000;
pub const XTAL_HZ: u64 = 40_000_000;
pub const SYSTIMER_HZ: u64 = 16_000_000;
pub const RTC_SLOW_HZ: u64 = 150_000;

/// Generic 4 KiB register block backed by RAM (for devices we only need to "accept").
#[derive(Clone)]
pub struct RegRam { pub regs: Vec<u32> }
impl RegRam {
    pub fn new() -> Self { RegRam { regs: vec![0; 1024] } }
    pub fn read(&self, off: u32) -> u32 { self.regs[((off & 0xfff) >> 2) as usize] }
    pub fn write(&mut self, off: u32, v: u32) { self.regs[((off & 0xfff) >> 2) as usize] = v; }
}

// ------------------------------------------------------------------ USB Serial/JTAG
pub struct UsbSerialJtag {
    pub connected: bool,          // emulate a host: SOF every 1 ms
    pub dbg: bool,
    pub sof_count: u64,
    sof_acc: u64,
    pub tx_fifo: Vec<u8>,         // bytes written since last WR_DONE
    pub tx_out: Vec<u8>,          // flushed bytes for the host
    pub rx: std::collections::VecDeque<u8>,
    pub int_raw: u32, pub int_ena: u32, pub conf0: u32,
    ram: RegRam,
}
impl UsbSerialJtag {
    pub fn new() -> Self { UsbSerialJtag { connected: true, dbg: std::env::var("ESP_EMU_DEBUG_USB").is_ok(), sof_count: 0, sof_acc: 0, tx_fifo: Vec::new(), tx_out: Vec::new(), rx: Default::default(), int_raw: 0, int_ena: 0, conf0: 0, ram: RegRam::new() } }
    /// advance by CPU cycles; raise SOF interrupt every 1 ms of emulated time
    pub fn tick(&mut self, cycles: u64) { if !self.connected { return; } self.sof_acc += cycles; if self.sof_acc >= CPU_HZ / 4000 { self.sof_acc -= CPU_HZ / 4000; self.int_raw |= 1 << 1; if self.dbg { self.sof_count += 1; } } /* 4x per tick: HWCDC's tick hook clears it each tick */ }
    pub fn read(&mut self, off: u32) -> u32 {
        match off {
            0x0 => self.rx.pop_front().map(|b| b as u32).unwrap_or(0),
            0x4 => (1 << 1) | if self.rx.is_empty() { 0 } else { 1 << 2 },
            0x8 => { if self.dbg { eprintln!("[usb] int_raw read -> {:#x}", self.raw()); } self.raw() }
            0xc => { if self.dbg { eprintln!("[usb] int_st read -> {:#x} (ena {:#x})", self.raw() & self.int_ena, self.int_ena); } self.raw() & self.int_ena }
            0x10 => self.int_ena,
            0x18 => self.conf0,
            _ => self.ram.read(off),
        }
    }
    pub fn write(&mut self, off: u32, v: u32) {
        match off {
            0x0 => { self.tx_fifo.push(v as u8); if self.tx_fifo.len() >= 64 { self.flush(); } }
            0x4 => if v & 1 != 0 { self.flush(); },
            0x10 => { if self.dbg && v != self.int_ena { eprintln!("[usb] int_ena {:#x} -> {:#x} (raw {:#x}, fifo {} bytes)", self.int_ena, v, self.raw(), self.tx_fifo.len()); } self.int_ena = v }
            0x14 => { if self.dbg && v & !2 != 0 { eprintln!("[usb] int_clr {:#x} (raw before {:#x})", v, self.raw()); } self.int_raw &= !v; }
            0x18 => self.conf0 = v,
            _ => self.ram.write(off, v),
        }
    }
    fn flush(&mut self) { if self.dbg { eprintln!("[usb] flush {} bytes: {:?}", self.tx_fifo.len(), String::from_utf8_lossy(&self.tx_fifo)); } self.tx_out.extend_from_slice(&self.tx_fifo); self.tx_fifo.clear(); self.int_raw |= 1 << 3; }
    fn raw(&self) -> u32 { self.int_raw }
    pub fn host_input(&mut self, data: &[u8]) { self.rx.extend(data.iter()); if !data.is_empty() { self.int_raw |= 1 << 2; } }
    pub fn irq(&self) -> bool { self.raw() & self.int_ena != 0 }
}

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

// ------------------------------------------------------------------ System timer (16 MHz, 2 units, 3 comparators)
pub struct Systimer {
    /// one-shot alarm armed by COMPx_LOAD; fires as soon as the unit counter >= target (the S3
    /// compensates missed alarms: esp_timer arms targets already in the past to fire "now")
    pub armed: [bool; 3],
    pub conf: u32,
    pub unit: [u64; 2], pub unit_latch: [u64; 2], pub load: [u64; 2],
    pub target: [u64; 3], pub target_conf: [u32; 3], pub period_start: [u64; 3],
    pub int_ena: u32, pub int_raw: u32,
    ticks_acc: u64,
}
impl Systimer {
    pub fn new() -> Self { Systimer { conf: 0, unit: [0; 2], unit_latch: [0; 2], load: [0; 2], target: [0; 3], target_conf: [0; 3], period_start: [0; 3], int_ena: 0, int_raw: 0, ticks_acc: 0, armed: [false; 3] } }
    pub fn tick(&mut self, ticks: u64) {
        for u in 0..2 {
            if self.conf & (1 << (30 - u as u32)) != 0 { self.unit[u] = (self.unit[u] + ticks) & ((1u64 << 52) - 1); }
        }
        for t in 0..3 {
            if self.conf & (1 << (24 + t as u32)) == 0 { continue; }
            let unit = ((self.target_conf[t] >> 31) & 1) as usize;
            let now = self.unit[unit];
            if self.target_conf[t] & (1 << 30) != 0 {
                let period = (self.target_conf[t] & 0x03ff_ffff) as u64;
                if period > 0 && now.wrapping_sub(self.period_start[t]) >= period {
                    // one alarm per elapsed period; re-align so a lagging start can't fire on every call
                    let elapsed = now.wrapping_sub(self.period_start[t]);
                    self.period_start[t] = now - (elapsed % period);
                    self.int_raw |= 1 << t;
                }
            } else if self.armed[t] && now >= self.target[t] {
                self.armed[t] = false;
                self.int_raw |= 1 << t;
            }
        }
        let _ = &mut self.ticks_acc;
    }
    pub fn read(&mut self, off: u32) -> u32 {
        match off {
            0x0 => self.conf,
            0x4 | 0x8 => 1 << 29,
            0xc => (self.load[0] >> 32) as u32, 0x10 => self.load[0] as u32,
            0x14 => (self.load[1] >> 32) as u32, 0x18 => self.load[1] as u32,
            0x1c => (self.target[0] >> 32) as u32, 0x20 => self.target[0] as u32,
            0x24 => (self.target[1] >> 32) as u32, 0x28 => self.target[1] as u32,
            0x2c => (self.target[2] >> 32) as u32, 0x30 => self.target[2] as u32,
            0x34 => self.target_conf[0], 0x38 => self.target_conf[1], 0x3c => self.target_conf[2],
            0x40 => (self.unit_latch[0] >> 32) as u32, 0x44 => self.unit_latch[0] as u32,
            0x48 => (self.unit_latch[1] >> 32) as u32, 0x4c => self.unit_latch[1] as u32,
            0x64 => self.int_ena, 0x68 => self.int_raw, 0x70 => self.int_raw & self.int_ena,
            0x74 => self.target[0] as u32, 0x78 => (self.target[0] >> 32) as u32,
            0x7c => self.target[1] as u32, 0x80 => (self.target[1] >> 32) as u32,
            0x84 => self.target[2] as u32, 0x88 => (self.target[2] >> 32) as u32,
            0xfc => 0x2006171,
            _ => 0,
        }
    }
    pub fn write(&mut self, off: u32, v: u32) {
        match off {
            0x0 => self.conf = v,
            0x4 => if v & (1 << 30) != 0 { self.unit_latch[0] = self.unit[0]; },
            0x8 => if v & (1 << 30) != 0 { self.unit_latch[1] = self.unit[1]; },
            0xc => self.load[0] = (self.load[0] & 0xffff_ffff) | ((v as u64 & 0xfffff) << 32), 0x10 => self.load[0] = (self.load[0] & !0xffff_ffff) | v as u64,
            0x14 => self.load[1] = (self.load[1] & 0xffff_ffff) | ((v as u64 & 0xfffff) << 32), 0x18 => self.load[1] = (self.load[1] & !0xffff_ffff) | v as u64,
            0x1c => self.target[0] = (self.target[0] & 0xffff_ffff) | ((v as u64 & 0xfffff) << 32), 0x20 => self.target[0] = (self.target[0] & !0xffff_ffff) | v as u64,
            0x24 => self.target[1] = (self.target[1] & 0xffff_ffff) | ((v as u64 & 0xfffff) << 32), 0x28 => self.target[1] = (self.target[1] & !0xffff_ffff) | v as u64,
            0x2c => self.target[2] = (self.target[2] & 0xffff_ffff) | ((v as u64 & 0xfffff) << 32), 0x30 => self.target[2] = (self.target[2] & !0xffff_ffff) | v as u64,
            0x34 => self.target_conf[0] = v, 0x38 => self.target_conf[1] = v, 0x3c => self.target_conf[2] = v,
            0x50 | 0x54 | 0x58 => { let t = ((off - 0x50) / 4) as usize; let unit = ((self.target_conf[t] >> 31) & 1) as usize; self.period_start[t] = self.unit[unit]; self.armed[t] = true; }
            0x5c => self.unit[0] = self.load[0], 0x60 => self.unit[1] = self.load[1],
            0x64 => self.int_ena = v & 7, 0x6c => self.int_raw &= !v,
            _ => {}
        }
    }
    pub fn irq(&self, t: usize) -> bool { self.int_raw & self.int_ena & (1 << t) != 0 }
}

// ------------------------------------------------------------------ Timer group (T0/T1 + WDT + RTC calibration)
pub struct TimerGroup {
    ram: RegRam,
    pub t: [Timer; 2],
    pub int_raw: u32, pub int_ena: u32,
    apb_acc: u64,
}
#[derive(Default, Clone, Copy)]
pub struct Timer { pub config: u32, pub count: u64, pub latch: u64, pub alarm: u64, pub load: u64, pub prescale_acc: u64 }
impl TimerGroup {
    pub fn new() -> Self { TimerGroup { ram: RegRam::new(), t: [Timer::default(); 2], int_raw: 0, int_ena: 0, apb_acc: 0 } }
    pub fn tick(&mut self, apb_ticks: u64) {
        let _ = &mut self.apb_acc;
        for i in 0..2 {
            let t = &mut self.t[i];
            if t.config & (1 << 31) == 0 { continue; }   // TIMG_T0_EN
            let div = ((t.config >> 13) & 0xffff) as u64;
            let div = if div == 0 { 65536 } else { div };
            t.prescale_acc += apb_ticks;
            let steps = t.prescale_acc / div;
            t.prescale_acc %= div;
            if steps == 0 { continue; }
            let inc = t.config & (1 << 30) != 0;   // TIMG_T0_INCREASE
            let old = t.count;
            t.count = if inc { (t.count + steps) & ((1 << 54) - 1) } else { t.count.wrapping_sub(steps) & ((1 << 54) - 1) };
            if t.config & (1 << 10) != 0 {   // TIMG_T0_ALARM_EN
                let hit = if inc { old < t.alarm && t.count >= t.alarm } else { old > t.alarm && t.count <= t.alarm };
                if hit {
                    self.int_raw |= 1 << i;
                    if t.config & (1 << 29) != 0 { t.count = t.load; } else { t.config &= !(1 << 10); }   // autoreload
                }
            }
        }
    }
    pub fn read(&mut self, off: u32) -> u32 {
        let (i, o) = if off < 0x24 { (0usize, off) } else if off < 0x48 { (1usize, off - 0x24) } else { (2, off) };
        if i < 2 {
            let t = &self.t[i];
            return match o { 0x0 => t.config, 0x4 => t.latch as u32, 0x8 => (t.latch >> 32) as u32, 0x10 => t.alarm as u32, 0x14 => (t.alarm >> 32) as u32, 0x18 => t.load as u32, 0x1c => (t.load >> 32) as u32, _ => 0 };
        }
        match off {
            0x68 => (self.ram.read(off) & !(1 << 15)) | (1 << 15),                       // RTCCALICFG: always RDY
            0x6c => { let n = (self.ram.read(0x68) >> 16) & 0x7fff; ((n as u64 * XTAL_HZ / RTC_SLOW_HZ) as u32) << 7 }   // RTCCALICFG1 value
            0x70 => self.int_ena, 0x74 => self.int_raw, 0x78 => self.int_raw & self.int_ena,
            0xf8 => 0x2006191,
            _ => self.ram.read(off),
        }
    }
    pub fn write(&mut self, off: u32, v: u32) {
        let (i, o) = if off < 0x24 { (0usize, off) } else if off < 0x48 { (1usize, off - 0x24) } else { (2, off) };
        if i < 2 {
            let t = &mut self.t[i];
            match o {
                0x0 => t.config = v,
                0xc => t.latch = t.count,
                0x10 => t.alarm = (t.alarm & !0xffff_ffff) | v as u64, 0x14 => t.alarm = (t.alarm & 0xffff_ffff) | ((v as u64 & 0x3fffff) << 32),
                0x18 => t.load = (t.load & !0xffff_ffff) | v as u64, 0x1c => t.load = (t.load & 0xffff_ffff) | ((v as u64 & 0x3fffff) << 32),
                0x20 => t.count = t.load,
                _ => {}
            }
            return;
        }
        match off {
            0x70 => self.int_ena = v, 0x7c => self.int_raw &= !v,
            _ => self.ram.write(off, v),
        }
    }
}

// ------------------------------------------------------------------ Interrupt matrix
pub struct IntMatrix { pub map: [[u32; NUM_SOURCES]; 2], ram: RegRam }
impl IntMatrix {
    pub fn new() -> Self { IntMatrix { map: [[6; NUM_SOURCES]; 2], ram: RegRam::new() } }
    pub fn read(&self, off: u32, status: &[u32; 4]) -> u32 {
        let (core, o) = if off >= 0x800 { (1usize, off - 0x800) } else { (0usize, off) };
        let idx = (o >> 2) as usize;
        if idx < NUM_SOURCES { return self.map[core][idx]; }
        match o { 0x18c => status[0], 0x190 => status[1], 0x194 => status[2], 0x198 => status[3], 0x7fc => 0x2007210, _ => self.ram.read(off) }
    }
    pub fn write(&mut self, off: u32, v: u32) {
        let (core, o) = if off >= 0x800 { (1usize, off - 0x800) } else { (0usize, off) };
        let idx = (o >> 2) as usize;
        if idx < NUM_SOURCES { self.map[core][idx] = v & 0x1f; } else { self.ram.write(off, v); }
    }
}

// ------------------------------------------------------------------ GPIO
pub struct Gpio {
    pub out: u64, pub enable: u64, pub input: u64, pub status: u64, pub pin: [u32; 49],
    pub func_in_sel: [u32; 256], pub func_out_sel: [u32; 49], ram: RegRam,
    pub input_changes: Vec<(u8, bool)>,
    /// (pin, level) changes of enabled outputs since last drain
    pub changes: Vec<(u8, bool)>,
    pub strap: u32,
}
impl Gpio {
    pub fn new() -> Self { Gpio { out: 0, enable: 0, input: !0u64 & ((1u64 << 49) - 1), status: 0, pin: [0; 49], func_in_sel: [0x3c; 256], func_out_sel: [0x100; 49], ram: RegRam::new(), changes: Vec::new(), strap: 0x0f, input_changes: Vec::new() } }
    fn note_out(&mut self, old: u64) {
        let vis = self.out & self.enable; let oldvis = old & self.enable;
        let diff = vis ^ oldvis;
        if diff == 0 { return; }
        for p in 0..49u8 { if diff & (1u64 << p) != 0 { self.changes.push((p, vis & (1u64 << p) != 0)); } }
    }
    pub fn set_input(&mut self, pin: u8, level: bool) -> bool {
        let old = self.input;
        if level { self.input |= 1u64 << pin; } else { self.input &= !(1u64 << pin); }
        if old == self.input { return false; }
        self.input_changes.push((pin, level));
        // edge detection per GPIO_PINn INT_TYPE (bits 7..9): 1 rising, 2 falling, 3 any, 4 low level, 5 high level
        let typ = (self.pin[pin as usize] >> 7) & 7;
        let rising = level && (typ == 1 || typ == 3);
        let falling = !level && (typ == 2 || typ == 3);
        if rising || falling { self.status |= 1u64 << pin; return true; }
        false
    }
    pub fn level(&self, pin: u8) -> bool {
        if self.enable & (1u64 << pin) != 0 { self.out & (1u64 << pin) != 0 } else { self.input & (1u64 << pin) != 0 }
    }
    pub fn irq(&self) -> bool {
        // level-type interrupts on current input, plus latched edge status, gated by INT_ENA (bits 13..17, bit 13 = core0)
        (0..49u8).any(|p| { let cfg = self.pin[p as usize]; let ena = (cfg >> 13) & 1 != 0; let typ = (cfg >> 7) & 7;
            ena && ((self.status & (1u64 << p) != 0) || (typ == 4 && !self.level(p)) || (typ == 5 && self.level(p))) })
    }
    pub fn read(&mut self, off: u32) -> u32 {
        match off {
            0x4 => self.out as u32, 0x10 => (self.out >> 32) as u32,
            0x20 => self.enable as u32, 0x2c => (self.enable >> 32) as u32,
            0x38 => self.strap,
            0x3c => self.input as u32, 0x40 => (self.input >> 32) as u32,
            0x44 => self.status as u32, 0x50 => (self.status >> 32) as u32,
            0x5c => (self.status as u32), 0x68 => (self.status >> 32) as u32,     // PCPU_INT: interrupt status seen by core 0
            0x74..=0x134 => self.pin[((off - 0x74) / 4) as usize],
            0x154..=0x550 => self.func_in_sel[((off - 0x154) / 4) as usize],
            0x554..=0x614 => self.func_out_sel[((off - 0x554) / 4) as usize],
            0x6fc => 0x2006130,
            _ => self.ram.read(off),
        }
    }
    pub fn write(&mut self, off: u32, v: u32) {
        let old = self.out;
        match off {
            0x4 => self.out = (self.out & !0xffff_ffff) | v as u64,
            0x8 => self.out |= v as u64,
            0xc => self.out &= !(v as u64),
            0x10 => self.out = (self.out & 0xffff_ffff) | ((v as u64 & 0x1ffff) << 32),
            0x14 => self.out |= (v as u64) << 32,
            0x18 => self.out &= !((v as u64) << 32),
            0x20 => { self.enable = (self.enable & !0xffff_ffff) | v as u64; }
            0x24 => self.enable |= v as u64,
            0x28 => self.enable &= !(v as u64),
            0x2c => self.enable = (self.enable & 0xffff_ffff) | ((v as u64 & 0x1ffff) << 32),
            0x30 => self.enable |= (v as u64) << 32,
            0x34 => self.enable &= !((v as u64) << 32),
            0x44 => self.status = (self.status & !0xffff_ffff) | v as u64,
            0x48 => self.status |= v as u64,
            0x4c => self.status &= !(v as u64),
            0x50 => self.status = (self.status & 0xffff_ffff) | ((v as u64) << 32),
            0x54 => self.status |= (v as u64) << 32,
            0x58 => self.status &= !((v as u64) << 32),
            0x74..=0x134 => self.pin[((off - 0x74) / 4) as usize] = v,
            0x154..=0x550 => self.func_in_sel[((off - 0x154) / 4) as usize] = v,
            0x554..=0x614 => self.func_out_sel[((off - 0x554) / 4) as usize] = v,
            _ => self.ram.write(off, v),
        }
        // enable changes also change what's visible on pins
        if matches!(off, 0x4 | 0x8 | 0xc | 0x10 | 0x14 | 0x18 | 0x20 | 0x24 | 0x28 | 0x2c | 0x30 | 0x34) { self.note_out(old); }
    }
}

// ------------------------------------------------------------------ RTC controller
/// Reset causes (RTC_CNTL_RESET_CAUSE_PROCPU), as the ROM prints them.
pub const RST_POWERON: u32 = 1; pub const RST_SW_SYS: u32 = 3; pub const RST_RTCWDT_SYS: u32 = 9; pub const RST_SW_CPU: u32 = 12;
pub const RST_RTCWDT_CPU: u32 = 13; pub const RST_RTCWDT_RTC: u32 = 16;
pub fn reset_cause_name(c: u32) -> &'static str {
    match c { 1 => "POWERON", 3 => "RTC_SW_SYS_RESET", 5 => "DEEPSLEEP", 7 => "TG0WDT_SYS_RESET", 8 => "TG1WDT_SYS_RESET", 9 => "RTCWDT_SYS_RESET", 11 => "TG0WDT_CPU_RESET",
            12 => "RTC_SW_CPU_RESET", 13 => "RTCWDT_CPU_RESET", 15 => "RTCWDT_BROWN_OUT_RESET", 16 => "RTCWDT_RTC_RESET", 17 => "TG1WDT_CPU_RESET", 18 => "SUPER_WDT_RESET", _ => "?" }
}

/// RTC_CNTL: reset control, slow-clock time, and the RTC watchdog (WDTCONFIG0..WDTWPROTECT at 0x98..0xb0).
/// `esp_restart()` on ESP-IDF 5.x arms this watchdog and spins until it resets the chip.
pub struct RtcCntl { pub ram: RegRam, pub slow_ticks: u64, pub time_latch: u64, pub sw_reset: bool, pub reset_cause: u32,
                     wdt_count: u64, wdt_stage: usize, wdt_unlocked: bool }
impl RtcCntl {
    pub fn preset_after_bootloader(&mut self) { self.ram.write(0xc0, 0xFFD7_0028); self.ram.write(0xc4, 0xFF0F_00F0); }
    fn request_reset(&mut self, cause: u32) { if !self.sw_reset { self.sw_reset = true; self.reset_cause = cause; } }
    /// Advance the watchdog by RTC slow-clock ticks.
    pub fn wdt_tick(&mut self, ticks: u64) {
        let conf0 = self.ram.read(0x98);
        if conf0 & (1 << 31) == 0 { return; }
        self.wdt_count += ticks;
        while self.wdt_stage < 4 {
            let timeout = self.ram.read(0x9c + 4 * self.wdt_stage as u32) as u64;
            let action = (conf0 >> (28 - 3 * self.wdt_stage as u32)) & 7;
            if action == 0 { self.wdt_stage += 1; continue; }              // stage disabled: skip
            if self.wdt_count < timeout { break; }
            self.wdt_count = 0; self.wdt_stage += 1;
            match action {
                1 => { self.ram.write(0x100, self.ram.read(0x100) | (1 << 10)); }   // INT_RAW.WDT
                2 => self.request_reset(RST_RTCWDT_CPU),
                3 => self.request_reset(RST_RTCWDT_SYS),
                4 => self.request_reset(RST_RTCWDT_RTC),
                _ => {}
            }
            if self.sw_reset { break; }
        }
        if self.wdt_stage >= 4 { self.wdt_stage = 0; }
    }
    pub fn new() -> Self {
        let mut r = RtcCntl { ram: RegRam::new(), slow_ticks: 0, time_latch: 0, sw_reset: false, reset_cause: RST_POWERON, wdt_count: 0, wdt_stage: 0, wdt_unlocked: false };
        r.ram.write(0x38, 1 | (1 << 6));           // RESET_STATE: reset cause POWERON for both CPUs
        r.ram.write(0x74, 0);                        // CLK_CONF
        r
    }
    pub fn read(&mut self, off: u32) -> u32 {
        match off {
            0x10 => self.time_latch as u32, 0x14 => (self.time_latch >> 32) as u32,
            0xc => self.ram.read(off) | (1 << 30),  // TIME_UPDATE: valid
            0x1fc => 0x2007270,
            0x850 => (self.ram.read(off) & !0x1ff) | (1 << 8) | 0x80,   // SENS_SAR_TSENS_CTRL (SENS block at +0x800): TSENS_READY, raw ~ room temperature
            _ => self.ram.read(off),
        }
    }
    pub fn write(&mut self, off: u32, v: u32) {
        match off {
            0x0 => { if v & (1 << 31) != 0 { self.request_reset(RST_SW_SYS); } else if v & (1 << 5) != 0 { self.request_reset(RST_SW_CPU); } self.ram.write(off, v & !((1 << 31) | (1 << 5))); }   // OPTIONS0.SW_SYS_RST / SW_PROCPU_RST
            0xc => { if v & (1 << 31) != 0 { self.time_latch = self.slow_ticks; } self.ram.write(off, v); }
            0xb0 => { self.wdt_unlocked = v == 0x50D8_3AA1; self.ram.write(off, v); }
            0x98..=0xa8 => { if self.wdt_unlocked { if off == 0x98 && (v ^ self.ram.read(0x98)) & (1 << 31) != 0 { self.wdt_count = 0; self.wdt_stage = 0; } self.ram.write(off, v); } }
            0xac => { if self.wdt_unlocked && v & (1 << 31) != 0 { self.wdt_count = 0; self.wdt_stage = 0; } }   // WDTFEED
            _ => self.ram.write(off, v),
        }
    }
}

// ------------------------------------------------------------------ WiFi MAC (blocks 0x33/0x34)
pub const SRC_WIFI_MAC: usize = 0;
/// The 802.11 MAC the closed `libpp`/`libnet80211` drive. Undocumented by Espressif; the register
/// layout matches the classic ESP32's as reverse-engineered by esp32-open-mac (0x3ff73000 there,
/// 0x60033000 here). Modelled from the blob's own accesses — see docs/wifi-plan.md.
///   TX: 5 slots; slot n has TX_CONFIG at 0xd1c-8n and PLCP0 at 0xd20-8n; PLCP0 = (desc & 0xfffff) | 0x600000,
///       bits 31:30 start the transmission. Completion: TXQ_STATE_COMPLETE (0xcc8) bit n, cleared via 0xcc4;
///       DMA_INT_STATUS (0xc48) bit 7, cleared via 0xc4c.
///   RX: descriptor ring base at 0x088 (dma_list_item: size:12 length:12 _:6 has_data:1 owner:1, packet, next).
pub struct WifiMac { pub ram: RegRam, pub ram2: RegRam, pub log: bool,
                     /// TSF: 1 MHz counter (offset applied to the CPU cycle clock), latched into WDEV 0x18/0x1c
                     pub tsf_offset: i64, pub tsf_latched: u64, pub now_cycles: u64,
                     /// interrupt events (0xc3c; cleared by writing 0xc40): bit 7 = TX complete, bits 14/24 = RX data (libpp wDev_ProcessFiq)
                     pub events: u32, pub pwr_events: u32,
                     /// per-queue completion bitmap (0xca8 bits 10:0, cleared via 0xca4)
                     pub txq_complete: u32, pub txq_error: u32,
                     pub tx_pending: Vec<(u8, u32)>, pub tx_frames: u64,
                     /// RX descriptor ring: base written by the driver (0x088), the descriptor the hardware fills next, the last one filled
                     pub rx_base: u32, pub rx_next: u32, pub rx_last: u32, pub rx_frames: u64, pub rx_dropped: u64,
                     pub ap: Option<crate::wifi::VirtualAp>, pub eth_tx: Vec<Vec<u8>>, pub eth_rx: Vec<Vec<u8>> }
impl WifiMac {
    pub fn new() -> Self { WifiMac { ram: RegRam::new(), ram2: RegRam::new(), log: std::env::var("ESP_EMU_DEBUG_WIFI").is_ok(), tsf_offset: 0, tsf_latched: 0, now_cycles: 0, rx_base: 0, rx_next: 0, rx_last: 0, rx_frames: 0, rx_dropped: 0, ap: None, eth_tx: Vec::new(), eth_rx: Vec::new(), events: 0, pwr_events: 0, txq_complete: 0, txq_error: 0, tx_pending: Vec::new(), tx_frames: 0 } }
    pub fn irq(&self) -> bool { self.events != 0 || self.pwr_events != 0 }
    /// TX queue n has its PLCP0 register at 0xd08 - 8n (hal_mac_txq_enable: (0x0c0067a1 - n) << 3).
    fn txq_of(off: u32) -> Option<u8> { if off <= 0xd08 && (0xd08 - off) % 8 == 0 && (0xd08 - off) / 8 < 16 { Some(((0xd08 - off) / 8) as u8) } else { None } }
    pub fn read(&mut self, block: u32, off: u32) -> u32 {
        let v = match (block, off) {
            (0x33, 0xd14) => self.ram.read(off) | 1,                 // hal_init: writes bit 1, waits for bit 0
            (0x33, 0xc3c) => self.events,
            (0x33, 0x088) => self.rx_base, (0x33, 0x08c) => self.rx_next, (0x33, 0x090) => self.rx_last,
            (0x33, 0xca8) => (self.txq_error & 0x7ff),                 // txq state types 0/1 (errors/collisions)
            (0x33, 0xcb0) => self.txq_complete & 0xf,                    // txq state type 2: completed queues
            (0x35, 0x118) => self.pwr_events,
            (0x35, 0x18) => self.tsf_latched as u32,
            (0x35, 0x1c) => (self.tsf_latched >> 32) as u32,
            (0x35, 0x128) => self.ram2.read(off),
            (0x33, _) => self.ram.read(off),
            (_, _) => self.ram2.read(off),
        };
        if self.log { eprintln!("[wifi] rd {:#x}+{:#05x} -> {:#010x}", block, off, v); }
        v
    }
    pub fn write(&mut self, block: u32, off: u32, v: u32) {
        if self.log { eprintln!("[wifi] wr {:#x}+{:#05x} <- {:#010x}", block, off, v); }
        match (block, off) {
            (0x33, 0xc40) => { self.events &= !v; }
            (0x33, 0x088) => { self.rx_base = v; self.rx_next = v; self.ram.write(off, v); }   // the hardware fetches from the base right away
            (0x33, 0x084) => { if v & 1 != 0 { self.rx_next = self.rx_base; } self.ram.write(off, v & !1); }   // DSCR_RELOAD: restart at base
            (0x35, 0x11c) => { self.pwr_events &= !v; }
            (0x35, 0x0c) => {
                let now = (self.now_cycles / (CPU_HZ / 1_000_000)) as i64;
                if v & 3 != 0 { self.tsf_latched = (now + self.tsf_offset) as u64; }                              // latch
                if v & (1 << 4) != 0 { let set = (self.ram2.read(0x10) as u64) | ((self.ram2.read(0x14) as u64) << 32); self.tsf_offset = set as i64 - now; }   // load
                self.ram2.write(off, v);
            }
            (0x33, 0xca4) => { self.txq_error &= !(v & 0x7ff); }
            (0x33, 0xcac) => { self.txq_complete &= !(v & 0xf); }
            (0x33, o) if Self::txq_of(o).is_some() => {                                   // MAC_TX_PLCP0[queue]
                self.ram.write(off, v);
                if v & (1 << 31) != 0 { let q = Self::txq_of(o).unwrap(); self.tx_pending.push((q, DMA_ADDR_BASE | (v & 0xf_ffff))); }
            }
            (0x33, _) => self.ram.write(off, v),
            (_, _) => self.ram2.write(off, v),
        }
    }
    /// Hardware finished sending the frame in `queue`.
    pub fn tx_done(&mut self, queue: u8) {
        self.txq_complete |= 1 << queue; self.events |= 1 << 7; self.tx_frames += 1;
        let o = 0xd08 - 8 * queue as u32; let v = self.ram.read(o); self.ram.write(o, v & !(3 << 30));
        // result word (hal_mac_get_txq_pmd): bits 15:12 = status code, 0 = success (3 would trap the blob)
        let r = 0x320 - 76 * queue as u32; let w = self.ram2.read(r); self.ram2.write(r, w & !(0xf << 12));
    }
}

// ------------------------------------------------------------------ PCNT (pulse counter)
/// Four units, two channels each: a signal input counts on rising/falling edges (mode 0 ignore,
/// 1 increment, 2 decrement) and a control input modifies that (hctrl/lctrl mode 0 keep, 1 invert,
/// 2 disable). Inputs arrive through the GPIO matrix (signals 33 + 4*unit .. 36 + 4*unit).
/// Counters are 16-bit signed; high/low limits, thresholds and zero crossings raise the unit's interrupt.
pub struct Pcnt { pub conf: [[u32; 3]; 4], pub cnt: [i16; 4], pub status: [u32; 4], pub int_raw: u32, pub int_ena: u32, pub ctrl: u32, ram: RegRam, pub events: u64 }
impl Pcnt {
    pub fn new() -> Self { Pcnt { conf: [[0; 3]; 4], cnt: [0; 4], status: [0; 4], int_raw: 0, int_ena: 0, ctrl: 0, ram: RegRam::new(), events: 0 } }
    pub fn irq(&self) -> bool { self.int_raw & self.int_ena != 0 }
    pub fn read(&self, off: u32) -> u32 {
        match off {
            0x00..=0x2c => { let u = (off / 12) as usize; self.conf[u][((off % 12) / 4) as usize] }
            0x30..=0x3c => self.cnt[((off - 0x30) / 4) as usize] as u16 as u32,
            0x40 => self.int_raw, 0x44 => self.int_raw & self.int_ena, 0x48 => self.int_ena,
            0x50..=0x5c => self.status[((off - 0x50) / 4) as usize],
            0x60 => self.ctrl, 0xfc => 0x1912_0400,
            _ => self.ram.read(off),
        }
    }
    pub fn write(&mut self, off: u32, v: u32) {
        match off {
            0x00..=0x2c => { let u = (off / 12) as usize; self.conf[u][((off % 12) / 4) as usize] = v; }
            0x48 => self.int_ena = v, 0x4c => self.int_raw &= !v,
            0x60 => { self.ctrl = v; for u in 0..4 { if v & (1 << (2 * u)) != 0 { self.cnt[u] = 0; } } }
            _ => self.ram.write(off, v),
        }
    }
    /// A GPIO input changed: `sig(idx)` gives the level of matrix input signal `idx` (None = not routed).
    pub fn gpio_edge(&mut self, pin: u8, level: bool, sig: &dyn Fn(u32) -> Option<(u8, bool)>) {
        for u in 0..4 {
            if self.ctrl & (1 << (2 * u)) != 0 || self.ctrl & (1 << (2 * u + 1)) != 0 { continue; }   // reset held / paused
            let conf0 = self.conf[u][0];
            for ch in 0..2u32 {
                let Some((sp, _)) = sig(33 + 4 * u as u32 + ch) else { continue };
                if sp != pin { continue; }
                let sh = 16 + 8 * ch;
                let mode = if level { (conf0 >> (sh + 2)) & 3 } else { (conf0 >> sh) & 3 };          // pos_mode / neg_mode
                let mut delta: i32 = match mode { 1 => 1, 2 => -1, _ => 0 };
                if delta != 0 {
                    let ctrl_level = sig(35 + 4 * u as u32 + ch).map_or(true, |(_, l)| l);
                    let cm = if ctrl_level { (conf0 >> (sh + 4)) & 3 } else { (conf0 >> (sh + 6)) & 3 };   // hctrl / lctrl
                    match cm { 1 => delta = -delta, 2 => delta = 0, _ => {} }
                }
                if delta != 0 { self.count(u, delta); }
            }
        }
    }
    fn count(&mut self, u: usize, delta: i32) {
        let conf0 = self.conf[u][0]; let conf1 = self.conf[u][1]; let conf2 = self.conf[u][2];
        let old = self.cnt[u] as i32; let mut new = old + delta;
        let mut ev = 0u32;
        if conf0 & (1 << 12) != 0 && new >= (conf2 & 0xffff) as i16 as i32 { ev |= 1 << 5; new = 0; }         // h_lim
        if conf0 & (1 << 13) != 0 && new <= (conf2 >> 16) as i16 as i32 { ev |= 1 << 4; new = 0; }             // l_lim
        if conf0 & (1 << 14) != 0 && new == (conf1 & 0xffff) as i16 as i32 { ev |= 1 << 3; }                   // thres0
        if conf0 & (1 << 15) != 0 && new == (conf1 >> 16) as i16 as i32 { ev |= 1 << 2; }                      // thres1
        if conf0 & (1 << 11) != 0 && new == 0 && old != 0 { ev |= if delta > 0 { 1 } else { 2 }; }             // zero (mode: 1 from negative, 2 from positive)
        self.cnt[u] = new as i16; self.events += 1;
        if ev != 0 { self.status[u] = ev; self.int_raw |= 1 << u; }
    }
}

// ------------------------------------------------------------------ GP-SPI2/3 master (CPU-driven)
/// General-purpose SPI master as the Arduino HAL / IDF `spi_master` use it without DMA: the CPU
/// fills W0..W15, sets the phase enables and lengths, writes CMD.UPDATE then CMD.USR, and polls
/// USR until the transfer is done. Transfers complete instantly; the bytes that went out on MOSI
/// are queued in `tx` for the board (the display), MISO reads back as 0xFF.
pub struct GpSpi { pub regs: RegRam, pub w: [u32; 16], pub int_raw: u32, pub int_ena: u32, pub tx: Vec<u8>, pub transfers: u64, pub log: bool }
impl GpSpi {
    pub fn new() -> Self { GpSpi { regs: RegRam::new(), w: [0; 16], int_raw: 0, int_ena: 0, tx: Vec::new(), transfers: 0, log: std::env::var("ESP_EMU_DEBUG_SPI2").is_ok() } }
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
            let n = (((user2 >> 28) & 0xf) + 1 + 7) / 8; let c = user2 & 0xffff;
            for i in 0..n { self.tx.push((c >> (8 * i)) as u8); }
        }
        if user & (1 << 30) != 0 {                                        // address phase, MSB first from the top of ADDR
            let bits = (user1 >> 27) + 1; let n = (bits + 7) / 8; let a = self.regs.read(0x04);
            for i in 0..n { self.tx.push((a >> (24 - 8 * i)) as u8); }
        }
        if user & (1 << 27) != 0 {                                        // MOSI data phase from W0.. (or W8.. with HIGHPART)
            let bits = (self.regs.read(0x1c) & 0x3ffff) + 1; let n = ((bits + 7) / 8) as usize;
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
}

// ------------------------------------------------------------------ LCD_CAM (camera side)
/// The camera engine of LCD_CAM: once started it pulls one frame per sensor period through the GDMA
/// channel bound to trigger 5 (CAM). Only the register semantics the DVP driver needs are modelled.
pub struct LcdCam { pub ram: RegRam, pub cam_ctrl: u32, pub cam_ctrl1: u32, pub int_raw: u32, pub int_ena: u32, pub running: bool,
                    pub frame_cycles: u64, pub acc: u64, pub frames: u64, pub dropped: u64,
                    // LCD side (RGB / DPI mode): the panel is refreshed from a GDMA out-channel on trigger 5
                    pub lcd_clock: u32, pub lcd_user: u32, pub lcd_ctrl: u32, pub lcd_ctrl1: u32, pub lcd_acc: u64, pub lcd_frames: u64, pub lcd_line: Vec<u8>, pub lcd_fifo: std::collections::VecDeque<u8>, pub lcd_log: bool }
impl LcdCam {
    pub fn new() -> Self { LcdCam { ram: RegRam::new(), cam_ctrl: 0, cam_ctrl1: 0, int_raw: 0, int_ena: 0, running: false, frame_cycles: CPU_HZ / 10, acc: 0, frames: 0, dropped: 0,
                                    lcd_clock: 0, lcd_user: 0, lcd_ctrl: 0, lcd_ctrl1: 0, lcd_acc: 0, lcd_frames: 0, lcd_line: Vec::new(), lcd_fifo: std::collections::VecDeque::new(), lcd_log: std::env::var("ESP_EMU_DEBUG_LCD").is_ok() } }
    pub fn irq(&self) -> bool { self.int_raw & self.int_ena != 0 }
    /// LCD RGB mode running: LCD_START (USER bit 27) with LCD_RGB_MODE_EN (CTRL bit 31).
    pub fn lcd_running(&self) -> bool { self.lcd_user & (1 << 27) != 0 && self.lcd_ctrl & (1 << 31) != 0 }
    /// (active width, active height, bytes per pixel, CPU cycles per frame) from the timing registers.
    pub fn lcd_geometry(&self) -> (u32, u32, u32, u64) {
        // the registers hold (value - 1): lcd_ll_set_horizontal/vertical_timing
        let ha = ((self.lcd_ctrl1 >> 8) & 0xfff) + 1; let ht = ((self.lcd_ctrl1 >> 20) & 0xfff) + 1;
        let va = ((self.lcd_ctrl >> 11) & 0x3ff) + 1; let vt = ((self.lcd_ctrl >> 21) & 0x3ff) + 1;
        let bpp = if self.lcd_user & (1 << 23) != 0 { 2 } else { 1 };
        // lcd_clk = src / (div_num + div_b/div_a); pclk = lcd_clk / (clkcnt_n + 1) unless CLK_EQU_SYSCLK
        let src = match (self.lcd_clock >> 29) & 3 { 1 => 40_000_000f64, 2 => 240_000_000.0, _ => 160_000_000.0 };
        let div_num = ((self.lcd_clock >> 9) & 0xff).max(1) as f64; let div_b = ((self.lcd_clock >> 17) & 0x3f) as f64; let div_a = ((self.lcd_clock >> 23) & 0x3f) as f64;
        let lcd_clk = src / (div_num + if div_a > 0.0 { div_b / div_a } else { 0.0 });
        let n = if self.lcd_clock & (1 << 6) != 0 { 1.0 } else { (self.lcd_clock & 0x3f) as f64 + 1.0 };
        let pclk = (lcd_clk / n).max(1_000_000.0) as u64;
        let frame_px = (ht as u64) * (vt as u64);
        (ha, va, bpp, frame_px * CPU_HZ / pclk)
    }
    pub fn read(&self, off: u32) -> u32 {
        match off { 0x00 => self.lcd_clock, 0x04 => self.cam_ctrl, 0x08 => self.cam_ctrl1, 0x14 => self.lcd_user & !((1 << 20) | (1 << 28)), 0x1c => self.lcd_ctrl, 0x20 => self.lcd_ctrl1,
                    0x64 => self.int_ena, 0x68 => self.int_raw, 0x6c => self.int_raw & self.int_ena, _ => self.ram.read(off) }
    }
    pub fn write(&mut self, off: u32, v: u32) {
        match off {
            0x00 => self.lcd_clock = v,
            0x14 => { let was = self.lcd_running(); self.lcd_user = v;
                      if v & (1 << 28) != 0 { self.lcd_line.clear(); self.lcd_fifo.clear(); self.lcd_acc = 0; }                       // LCD_RESET
                      if !was && self.lcd_running() { self.lcd_line.clear(); self.lcd_acc = 0; }
                      if self.lcd_log { eprintln!("[lcd] USER <- {:#010x} (start {} reset {} update {})", v, v >> 27 & 1, v >> 28 & 1, v >> 20 & 1); } }
            0x18 => { if v & (1 << 27) != 0 { self.lcd_fifo.clear(); if self.lcd_log { eprintln!("[lcd] AFIFO reset"); } } self.ram.write(off, v); }   // LCD_MISC.AFIFO_RESET
            0x1c => self.lcd_ctrl = v, 0x20 => self.lcd_ctrl1 = v,
            0x04 => { self.cam_ctrl = v & !(1 << 4); }                                                                          // CAM_UPDATE (self-clearing)
            0x08 => { self.cam_ctrl1 = v & !(3 << 30); self.running = v & (1 << 29) != 0; if v & (1 << 30) != 0 { self.acc = 0; } }   // CAM_START / CAM_RESET
            0x64 => self.int_ena = v, 0x70 => self.int_raw &= !v,
            _ => self.ram.write(off, v),
        }
    }
    /// True when a new frame is due (advances the frame clock while streaming).
    pub fn frame_due(&mut self, cycles: u64) -> bool { if !self.running { self.acc = 0; return false; } self.acc += cycles; if self.acc >= self.frame_cycles { self.acc -= self.frame_cycles; true } else { false } }
}

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

// ------------------------------------------------------------------ SYSTEM
pub struct SystemRegs { pub ram: RegRam }
impl SystemRegs {
    /// values the 2nd-stage bootloader leaves behind; used by the HLE app-boot shortcut
    pub fn preset_after_bootloader(&mut self) { self.ram.write(0x10, (1 << 2) | 2); self.ram.write(0x60, 1 << 10); }
    pub fn new() -> Self {
        let mut s = SystemRegs { ram: RegRam::new() };
        s.ram.write(0x60, 0x000a_8001);        // SYSCLK_CONF reset value (XTAL 40 MHz selected) as read on silicon
        s.ram.write(0x18, 0xffff_ffff); s.ram.write(0x1c, 0xffff_ffff);
        s
    }
    pub fn read(&self, off: u32) -> u32 { match off { 0xffc => 0x2101220, _ => self.ram.read(off) } }
    pub fn write(&mut self, off: u32, v: u32) { self.ram.write(off, v); }
}

// ------------------------------------------------------------------ EXTMEM (cache controller; MMU table lives in the bus)
pub struct Extmem { pub ram: RegRam }
impl Extmem {
    pub fn new() -> Self { Extmem { ram: RegRam::new() } }
    pub fn read(&self, off: u32) -> u32 {
        let v = self.ram.read(off);
        match off {
            0x28 => v | (1 << 3),                 // DCACHE_SYNC_CTRL: SYNC_DONE
            0x88 => v | (1 << 1),                 // ICACHE_SYNC_CTRL: SYNC_DONE
            0x40 | 0x94 => v | (1 << 1),          // *CACHE_PRELOAD_CTRL: PRELOAD_DONE
            0x4c | 0xa0 => v | (1 << 3),          // *CACHE_AUTOLOAD_CTRL: AUTOLOAD_DONE
            0x34 => v | (1 << 1),                 // DCACHE_OCCUPY_CTRL: OCCUPY_DONE
            0x150 | 0x154 => if v & 1 != 0 { v | (1 << 2) } else { v & !(1 << 2) },   // *CACHE_FREEZE: FREEZE_DONE follows FREEZE_ENA
            0x1c | 0x7c => v | (1 << 2),      // *CACHE_LOCK_CTRL: 0x1cONE
            0x130 => 0x1001,                      // CACHE_STATE: icache/dcache idle
            0x3fc => 0x2101070,
            _ => v,
        }
    }
    pub fn write(&mut self, off: u32, v: u32) { self.ram.write(off, v); }
}

// ------------------------------------------------------------------ all together
pub struct Peripherals {
    pub usb: UsbSerialJtag,
    pub uart: [Uart; 3],
    pub systimer: Systimer,
    pub timg: [TimerGroup; 2],
    pub intmatrix: IntMatrix,
    pub gpio: Gpio,
    pub rtc: RtcCntl,
    pub efuse: Efuse,
    pub system: SystemRegs,
    pub extmem: Extmem,
    pub spi0: SpiMem,
    pub spi1: SpiMem,
    pub i2c: [crate::i2c::I2c; 2],
    pub lcd_cam: LcdCam,
    pub spi2: GpSpi,
    pub pcnt: Pcnt,
    pub wifi: WifiMac,
    pub sha: Sha,
    pub wdev: Wdev,
    pub i2c_mst: I2cMst,
    pub gdma: Gdma,
    pub i2s0: I2s,
    pub i2s1: I2s,
    pub rmt: Rmt,
    pub generic: std::collections::HashMap<u32, RegRam>,
    pub log_unknown: bool,
    /// per-(address, pc, write) access statistics for register reverse engineering (`--regstat FILE`)
    pub regstat: Option<std::collections::HashMap<(u32, u32, bool), (u64, u32)>>,
    /// experiment hook: ESP_EMU_FAKE_READ=addr:or[:and],... applied to register reads
    pub fake_reads: std::collections::HashMap<u32, (u32, u32)>,
    pub log_all: bool,
    seen: HashSet<(u32, bool)>,
    pub cur_pc: u32,
    cycle_total: u64, st_done: u64, apb_done: u64, rtc_done: u64,
    pub sw_int: u32,          // SYSTEM_CPU_INTR_FROM_CPU_0..3
    pub spi_exec: bool,       // SPI1 command pending execution against the flash array
    last_status: [u32; 4],
    pub intmatrix_dirty: bool,
    pub io_mux: RegRam,
}

impl Peripherals {
    pub fn new(mac: [u8; 6]) -> Self {
        Peripherals {
            usb: UsbSerialJtag::new(), uart: [Uart::new(), Uart::new(), Uart::new()], systimer: Systimer::new(),
            timg: [TimerGroup::new(), TimerGroup::new()], intmatrix: IntMatrix::new(), gpio: Gpio::new(), rtc: RtcCntl::new(),
            efuse: Efuse::new(mac), system: SystemRegs::new(), extmem: Extmem::new(), spi0: SpiMem::new(false), spi1: SpiMem::new(true), i2c: [crate::i2c::I2c::new(), crate::i2c::I2c::new()], lcd_cam: LcdCam::new(), spi2: GpSpi::new(), pcnt: Pcnt::new(), wifi: WifiMac::new(), sha: Sha::new(), wdev: Wdev::new(), i2c_mst: I2cMst::new(), gdma: Gdma::new(), i2s0: I2s::new(), i2s1: I2s::new(), rmt: Rmt::new(), generic: Default::default(),
            log_unknown: false, regstat: None, fake_reads: std::env::var("ESP_EMU_FAKE_READ").ok().map(|v| v.split(',').filter_map(|e| { let mut p = e.split(':'); let a = u32::from_str_radix(p.next()?.trim_start_matches("0x"), 16).ok()?; let o = u32::from_str_radix(p.next().unwrap_or("0").trim_start_matches("0x"), 16).ok()?; let m = u32::from_str_radix(p.next().unwrap_or("ffffffff").trim_start_matches("0x"), 16).ok()?; Some((a, (o, m))) }).collect()).unwrap_or_default(), log_all: std::env::var("ESP_EMU_LOG_ALL").is_ok(), seen: HashSet::new(), cur_pc: 0, cycle_total: 0, st_done: 0, apb_done: 0, rtc_done: 0, sw_int: 0, spi_exec: false, last_status: [0; 4], intmatrix_dirty: true, io_mux: RegRam::new(),
        }
    }

    pub fn block_name_pub(block: u32) -> String { Self::block_name(block).to_string() }
    fn block_name(block: u32) -> &'static str {
        match block {
            0x00 => "UART0", 0x02 => "SPI1", 0x03 => "SPI0", 0x04 => "GPIO", 0x05 => "FE2", 0x06 => "FE", 0x07 => "EFUSE", 0x08 => "RTC", 0x09 => "IO_MUX",
            0x0b => "HINF", 0x0c => "UHCI1", 0x0f => "I2S0", 0x10 => "UART1", 0x11 => "BT", 0x13 => "I2C0", 0x14 => "UHCI0", 0x15 => "SLCHOST", 0x16 => "RMT", 0x17 => "PCNT",
            0x18 => "SLC", 0x19 => "LEDC", 0x1c => "NRX", 0x1d => "BB", 0x1e => "PWM0", 0x1f => "TIMG0", 0x20 => "TIMG1", 0x21 => "RTC_SLOWMEM", 0x23 => "SYSTIMER",
            0x24 => "SPI2", 0x25 => "SPI3", 0x26 => "APB_CTRL", 0x27 => "I2C1", 0x28 => "SDMMC", 0x2a => "PERI_BACKUP", 0x2b => "TWAI", 0x2c => "PWM1", 0x2d => "I2S1", 0x2e => "UART2", 0x33 => "WIFI_MAC", 0x34 => "WIFI_MAC2", 0x35 => "WDEV", 0x0e => "I2C_MST",
            0x38 => "USB_SERIAL_JTAG", 0x39 => "USB_WRAP", 0x3a => "AES", 0x3b => "SHA", 0x3c => "RSA", 0x3d => "DS", 0x3e => "HMAC", 0x3f => "GDMA", 0x40 => "APB_SARADC", 0x41 => "LCD_CAM",
            0xc0 => "SYSTEM", 0xc1 => "SENSITIVE", 0xc2 => "INTERRUPT", 0xc4 => "EXTMEM", 0xc5 => "MMU", 0xce => "ASSIST_DEBUG", 0xcf => "ASSIST_DEBUG2", 0xd0 => "WCL",
            _ => "?",
        }
    }

    fn note(&mut self, addr: u32, write: bool, v: u32) {
        if !self.log_unknown { return; }
        let key = (addr & !3, write);
        if self.seen.insert(key) {
            let block = (addr - PERIPH_BASE) >> 12;
            eprintln!("[periph] {} {}+0x{:03x} ({:#010x}) {} pc={:#010x}", if write { "W" } else { "R" }, Self::block_name(block), addr & 0xfff, addr, if write { format!("= {:#x}", v) } else { String::new() }, self.cur_pc);
        }
    }

    pub fn read32(&mut self, addr: u32) -> u32 {
        let mut v = self.read32_inner(addr);
        if !self.fake_reads.is_empty() { if let Some(&(o, m)) = self.fake_reads.get(&addr) { v = (v & m) | o; } }
        if let Some(st) = &mut self.regstat { let e = st.entry((addr, self.cur_pc, false)).or_insert((0, 0)); e.0 += 1; e.1 = v; }
        v
    }
    fn read32_inner(&mut self, addr: u32) -> u32 {
        let block = (addr - PERIPH_BASE) >> 12;
        let off = addr & 0xfff;
        let status = self.source_status();
        let v = match block {
            0x06 if off == 0x174 && self.wifi.ap.is_some() => self.generic.entry(block).or_insert_with(RegRam::new).read(off) | (1 << 16),   // FE: IQ estimation done (ram_iq_est_enable polls bit 16)
            0x00 => self.uart[0].read(off), 0x10 => self.uart[1].read(off), 0x2e => self.uart[2].read(off),
            0x38 => self.usb.read(off),
            0x23 => self.systimer.read(off),
            0x1f => self.timg[0].read(off), 0x20 => self.timg[1].read(off),
            0xc2 => self.intmatrix.read(off, &status),
            0x04 => self.gpio.read(off),
            0x08 => self.rtc.read(off),
            0x07 => self.efuse.read(off),
            0xc0 => match off { 0x30..=0x3c => (self.sw_int >> ((off - 0x30) / 4)) & 1, _ => self.system.read(off) },
            0xc4 => self.extmem.read(off),
            0x09 => self.io_mux.read(off),
            0x02 => self.spi1.read(off),
            0x03 => self.spi0.read(off),
            0x3b => self.sha.read(off),
            0x35 if matches!(off, 0x0c | 0x10 | 0x14 | 0x18 | 0x1c | 0x118 | 0x11c | 0x128) => { self.wifi.now_cycles = self.cycle_total; self.wifi.read(block, off) }
            0x35 => self.wdev.read(off),
            0x0e => self.i2c_mst.read(off),
            0x3f => self.gdma.read(off),
            0x0f => self.i2s0.read(off), 0x2d => self.i2s1.read(off),
            0x16 => self.rmt.read(off),
            0x13 => self.i2c[0].read(off), 0x27 => self.i2c[1].read(off),
            0x41 => self.lcd_cam.read(off),
            0x24 => self.spi2.read(off),
            0x17 => self.pcnt.read(off),
            0x33 | 0x34 => self.wifi.read(block, off),
            _ => { self.note(addr, false, 0); self.generic.entry(block).or_insert_with(RegRam::new).read(off) }
        };
        if self.log_all { eprintln!("[rd] {}+0x{:03x} ({:#010x}) -> {:#010x} pc={:#010x}", Self::block_name(block), off, addr, v, self.cur_pc); }
        v
    }

    pub fn write32(&mut self, addr: u32, v: u32) {
        if let Some(st) = &mut self.regstat { let e = st.entry((addr, self.cur_pc, true)).or_insert((0, 0)); e.0 += 1; e.1 = v; }
        let block = (addr - PERIPH_BASE) >> 12;
        let off = addr & 0xfff;
        if self.log_all { eprintln!("[wr] {}+0x{:03x} ({:#010x}) <- {:#010x} pc={:#010x}", Self::block_name(block), off, addr, v, self.cur_pc); }
        match block {
            0x00 => self.uart[0].write(off, v), 0x10 => self.uart[1].write(off, v), 0x2e => self.uart[2].write(off, v),
            0x38 => self.usb.write(off, v),
            0x23 => self.systimer.write(off, v),
            0x1f => self.timg[0].write(off, v), 0x20 => self.timg[1].write(off, v),
            0xc2 => { self.intmatrix.write(off, v); self.intmatrix_dirty = true; }
            0x04 => self.gpio.write(off, v),
            0x08 => self.rtc.write(off, v),
            0x07 => self.efuse.write(off, v),
            0xc0 => match off { 0x30..=0x3c => { let b = (off - 0x30) / 4; if v & 1 != 0 { self.sw_int |= 1 << b } else { self.sw_int &= !(1 << b) } } _ => self.system.write(off, v) },
            0xc4 => self.extmem.write(off, v),
            0x09 => self.io_mux.write(off, v),
            0x02 => { if self.spi1.write(off, v) { self.spi_exec = true; } }
            0x03 => { self.spi0.write(off, v); }
            0x3b => self.sha.write(off, v),
            0x35 if matches!(off, 0x0c | 0x10 | 0x14 | 0x18 | 0x1c | 0x118 | 0x11c | 0x128) => { self.wifi.now_cycles = self.cycle_total; self.wifi.write(block, off, v) }
            0x35 => self.wdev.write(off, v),
            0x0e => self.i2c_mst.write(off, v),
            0x3f => self.gdma.write(off, v),
            0x0f => self.i2s0.write(off, v), 0x2d => self.i2s1.write(off, v),
            0x16 => self.rmt.write(off, v),
            0x13 => self.i2c[0].write(off, v), 0x27 => self.i2c[1].write(off, v),
            0x41 => self.lcd_cam.write(off, v),
            0x24 => self.spi2.write(off, v),
            0x17 => self.pcnt.write(off, v),
            0x33 | 0x34 => self.wifi.write(block, off, v),
            _ => { self.note(addr, true, v); self.generic.entry(block).or_insert_with(RegRam::new).write(off, v) }
        }
    }

    /// Load raw reset-state register values (from a JTAG dump) into RAM-backed blocks, no side effects.
    /// Returns the number of words applied.
    pub fn init_regs(&mut self, addr: u32, v: u32) -> bool {
        if !(PERIPH_BASE..PERIPH_END).contains(&addr) { return false; }
        let block = (addr - PERIPH_BASE) >> 12; let off = addr & 0xfff;
        match block {
            0x08 => self.rtc.ram.write(off, v),
            0xc0 => { if off >= 0x30 && off <= 0x3c { return false; } self.system.ram.write(off, v) }
            0xc4 => self.extmem.ram.write(off, v),
            0x09 => self.io_mux.write(off, v),
            0x02 => { if off == 0 || (0x58..=0x94).contains(&off) { return false; } self.spi1.regs.write(off, v) }
            0x03 => { if off == 0 { return false; } self.spi0.regs.write(off, v) }
            0x0e => self.i2c_mst.ram.write(off, v),
            0x26 | 0xc1 => self.generic.entry(block).or_insert_with(RegRam::new).write(off, v),
            _ => return false,
        }
        true
    }

    /// Advance all timers by `cycles` CPU cycles. Each derived clock keeps a "delivered"
    /// counter so no cycle is counted twice.
    pub fn tick(&mut self, cycles: u64) {
        self.cycle_total += cycles;
        let st = self.cycle_total / 15;      // systimer 16 MHz = CPU/15
        if st > self.st_done { self.systimer.tick(st - self.st_done); self.st_done = st; }
        let apb = self.cycle_total / 3;      // APB 80 MHz = CPU/3
        if apb > self.apb_done { let d = apb - self.apb_done; self.apb_done = apb; self.timg[0].tick(d); self.timg[1].tick(d); }
        let rtc = self.cycle_total / 1600;   // RTC slow clock 150 kHz
        if rtc > self.rtc_done { let d = rtc - self.rtc_done; self.rtc.slow_ticks += d; self.rtc_done = rtc; self.rtc.wdt_tick(d); }
        self.usb.tick(cycles);
        self.rmt.tick(cycles);
        if !self.gpio.input_changes.is_empty() {
            let changes = std::mem::take(&mut self.gpio.input_changes);
            let gpio = &self.gpio;
            let sig = |idx: u32| -> Option<(u8, bool)> {
                let sel = *gpio.func_in_sel.get(idx as usize)?;
                if sel & 0x80 == 0 { return None; }                       // not routed through the matrix
                let pin = (sel & 0x3f) as u8; if pin >= 49 { return None; }
                let lvl = (gpio.input >> pin) & 1 != 0; Some((pin, lvl ^ (sel & 0x40 != 0)))
            };
            for (pin, level) in changes { self.pcnt.gpio_edge(pin, level, &sig); }
        }
    }

    /// Raw per-source interrupt status (4 × 32 bits).
    pub fn source_status(&self) -> [u32; 4] {
        let mut s = [0u32; 4];
        let mut set = |src: usize, on: bool| if on { s[src / 32] |= 1 << (src % 32); };
        set(SRC_UART0, self.uart[0].irq()); set(SRC_UART1, self.uart[1].irq());
        set(SRC_USB_SERIAL_JTAG, self.usb.irq());
        set(SRC_SYSTIMER_T0, self.systimer.irq(0)); set(SRC_SYSTIMER_T1, self.systimer.irq(1)); set(SRC_SYSTIMER_T2, self.systimer.irq(2));
        set(SRC_TG0_T0, self.timg[0].int_raw & self.timg[0].int_ena & 1 != 0); set(SRC_TG1_T0, self.timg[1].int_raw & self.timg[1].int_ena & 1 != 0);
        set(SRC_GPIO, self.gpio.irq());
        for i in 0..4 { set(SRC_FROM_CPU0 + i, self.sw_int & (1 << i) != 0); }
        for ch in 0..GDMA_CHANNELS { set(SRC_DMA_OUT_CH0 + ch, self.gdma.out[ch].irq()); set(SRC_DMA_IN_CH0 + ch, self.gdma.inp[ch].irq()); }
        set(SRC_LCD_CAM, self.lcd_cam.irq());
        set(SRC_SPI2, self.spi2.irq());
        set(SRC_WIFI_MAC, self.wifi.irq());
        set(SRC_PCNT, self.pcnt.irq());
        set(SRC_I2S0, self.i2s0.irq()); set(SRC_I2S1, self.i2s1.irq());
        set(SRC_RMT, self.rmt.irq());
        set(SRC_I2C0, self.i2c[0].irq()); set(SRC_I2C1, self.i2c[1].irq());
        s
    }

    /// The I2S controller carrying the board's audio output (whichever has played samples).
    pub fn audio(&self) -> &I2s { if self.i2s1.frames_out > self.i2s0.frames_out { &self.i2s1 } else { &self.i2s0 } }

    /// True if the raw source status changed since the last `cpu_lines_both` (cheap check).
    pub fn lines_dirty(&mut self) -> bool { let st = self.source_status(); if st != self.last_status { self.last_status = st; true } else { false } }
    /// Interrupt lines for both cores in one pass over the sources that are active.
    pub fn cpu_lines_both(&self) -> (u32, u32) {
        let st = self.last_status;
        let (mut l0, mut l1) = (0u32, 0u32);
        for w in 0..4 {
            let mut bits = st[w];
            while bits != 0 {
                let b = bits.trailing_zeros(); bits &= bits - 1;
                let src = w * 32 + b as usize; if src >= NUM_SOURCES { break; }
                let n0 = self.intmatrix.map[0][src]; if n0 < 32 { l0 |= 1 << n0; }
                let n1 = self.intmatrix.map[1][src]; if n1 < 32 { l1 |= 1 << n1; }
            }
        }
        (l0, l1)
    }

    /// CPU interrupt lines for `core` (bit n = Xtensa interrupt n) after the interrupt matrix.
    pub fn cpu_lines(&self, core: usize) -> u32 {
        let st = self.source_status();
        let mut lines = 0u32;
        for src in 0..NUM_SOURCES {
            if st[src / 32] & (1 << (src % 32)) != 0 { let n = self.intmatrix.map[core][src]; if n < 32 { lines |= 1 << n; } }
        }
        lines
    }
    /// SYSTEM_CORE_1_CONTROL_0: (clkgate_en, reseting, runstall)
    pub fn core1_control(&self) -> (bool, bool, bool) { let v = self.system.ram.read(0); (v & 2 != 0, v & 4 != 0, v & 1 != 0) }
}

// ------------------------------------------------------------------ SPI flash controller (SPI_MEM: SPI1 = command engine, SPI0 = cache path)
pub struct SpiMem {
    pub regs: RegRam,
    pub w: [u32; 16],
    pub pending_cmd: u32,
    pub status: u32,
    pub jedec: [u8; 3],
    pub is_spi1: bool,
    pub log: bool,
    /// octal PSRAM (APS6408-like) mode registers MR0..MR8, device on CS1 (SPI1 only)
    pub psram_mr: [u8; 9],
}
impl SpiMem {
    pub fn new(is_spi1: bool) -> Self { SpiMem { regs: RegRam::new(), w: [0; 16], pending_cmd: 0, status: 0x200, jedec: [0x20, 0x40, 0x17], is_spi1, log: false , psram_mr: [0x09, 0x0d, 0x8b, 0x00, 0x20, 0, 0, 0, 0x03] } }
    pub fn read(&self, off: u32) -> u32 {
        match off {
            0x0 => 0,                                   // CMD: always idle after execution
            0x2c => self.status,                        // RD_STATUS
            0x54 => 0,                                  // FSM idle
            0x58..=0x94 => self.w[((off - 0x58) / 4) as usize],
            0xa4 => 0,                                  // SUS_STATUS
            0x3fc => 0x2101040,
            _ => self.regs.read(off),
        }
    }
    /// Returns true if a command must be executed against the flash array.
    pub fn write(&mut self, off: u32, v: u32) -> bool {
        match off {
            0x0 => { self.pending_cmd = v; return v & 0xffff_0003 != 0; }
            0x2c => {}   // RD_STATUS is written by hardware only
            0x58..=0x94 => self.w[((off - 0x58) / 4) as usize] = v,
            _ => self.regs.write(off, v),
        }
        false
    }
    fn w_bytes(&self, n: usize) -> Vec<u8> { let mut o = Vec::with_capacity(n); for i in 0..n { o.push((self.w[(i / 4) & 15] >> ((i % 4) * 8)) as u8); } o }
    fn set_w_bytes(&mut self, data: &[u8]) { for w in self.w.iter_mut() { *w = 0; } for (i, b) in data.iter().enumerate().take(64) { self.w[i / 4] |= (*b as u32) << ((i % 4) * 8); } }

    pub fn execute(&mut self, flash: &mut [u8], psram: &mut [u8]) {
        let cmd = self.pending_cmd; self.pending_cmd = 0;
        let user = self.regs.read(0x18); let user1 = self.regs.read(0x1c); let user2 = self.regs.read(0x20);
        let addr_reg = self.regs.read(0x4);
        let miso_bytes = ((self.regs.read(0x28) & 0x3ff) as usize + 8) / 8;
        let mosi_bytes = ((self.regs.read(0x24) & 0x3ff) as usize + 8) / 8;
        let addr_bits = ((user1 >> 26) & 0x3f) + 1;
        let addr = if addr_bits > 24 { addr_reg } else { addr_reg & 0xff_ffff };
        let fsize = flash.len();
        let mut rd = |a: u32, n: usize| -> Vec<u8> { (0..n).map(|i| { let x = a as usize + i; if x < fsize { flash[x] } else { 0xff } }).collect() };
        let misc = self.regs.read(0x34);
        if cmd & (1 << 18) != 0 && misc & 1 != 0 && misc & 2 == 0 {   // USR command with CS0 disabled, CS1 enabled: the octal PSRAM
            let c16 = user2 & 0xffff;
            let has_miso = user & (1 << 28) != 0; let has_mosi = user & (1 << 27) != 0;
            if self.log { eprintln!("[spi1] psram cmd {:#06x} addr {:#x} miso {} mosi {}", c16, addr, if has_miso { miso_bytes } else { 0 }, if has_mosi { mosi_bytes } else { 0 }); }
            let psize = psram.len();
            match c16 {
                0x4040 => { let i = (addr & 0xf) as usize; let d: Vec<u8> = (0..miso_bytes).map(|k| *self.psram_mr.get(i + k).unwrap_or(&0)).collect(); self.set_w_bytes(&d); }   // mode register read
                0xC0C0 => { let d = self.w_bytes(mosi_bytes); let i = (addr & 0xf) as usize; for (k, b) in d.iter().enumerate() { if i + k == 0 || i + k == 8 { self.psram_mr[i + k] = *b; } } }   // mode register write (MR0/MR8 writable)
                0x8080 => { let d = self.w_bytes(mosi_bytes); for (k, b) in d.iter().enumerate() { let x = addr as usize + k; if x < psize { psram[x] = *b; } } }   // sync write
                0x0000 => { let d: Vec<u8> = (0..miso_bytes).map(|k| { let x = addr as usize + k; if x < psize { psram[x] } else { 0 } }).collect(); self.set_w_bytes(&d); }   // sync read
                _ => { if has_miso { self.set_w_bytes(&vec![0u8; miso_bytes]); } }
            }
            return;
        }
        if cmd & (1 << 18) != 0 {   // USR: command from USER2
            let c = if user & (1 << 31) != 0 { (user2 & 0xff) as u8 } else { 0 };
            let has_addr = user & (1 << 30) != 0;
            let has_miso = user & (1 << 28) != 0;
            let has_mosi = user & (1 << 27) != 0;
            if self.log { eprintln!("[spi1] usr cmd {:#04x} addr {:#x}{} miso {} mosi {}", c, addr, if has_addr { "" } else { " (no addr)" }, if has_miso { miso_bytes } else { 0 }, if has_mosi { mosi_bytes } else { 0 }); }
            match c {
                0x03 | 0x0b | 0x3b | 0x6b | 0xbb | 0xeb => { let d = rd(addr, miso_bytes); self.set_w_bytes(&d); }
                0x9f => { let j = self.jedec; self.set_w_bytes(&j); }
                0x05 => { let s = self.status; self.set_w_bytes(&[s as u8]); }
                0x35 => { let s = self.status; self.set_w_bytes(&[(s >> 8) as u8]); }
                0x06 => self.status |= 0x02,                       // WREN: set WEL
                0x04 => self.status &= !0x02,                      // WRDI
                0x01 | 0x31 | 0x11 => self.status &= !0x02,        // WRSR*: latch consumed (keep QE set)
                0x15 => self.set_w_bytes(&[0x00]),
                0x02 | 0x32 | 0x38 => { let d = self.w_bytes(mosi_bytes); for (i, b) in d.iter().enumerate() { let x = addr as usize + i; if x < fsize { flash[x] &= *b; } } self.status &= !0x02; }
                0x20 => { let a = (addr as usize) & !0xfff; for x in a..(a + 0x1000).min(fsize) { flash[x] = 0xff; } self.status &= !0x02; }
                0x52 => { let a = (addr as usize) & !0x7fff; for x in a..(a + 0x8000).min(fsize) { flash[x] = 0xff; } self.status &= !0x02; }
                0xd8 => { let a = (addr as usize) & !0xffff; for x in a..(a + 0x10000).min(fsize) { flash[x] = 0xff; } self.status &= !0x02; }
                0xc7 | 0x60 => { for b in flash.iter_mut() { *b = 0xff; } self.status &= !0x02; }
                _ => { if has_miso { self.set_w_bytes(&vec![0u8; miso_bytes]); } }
            }
            return;
        }
        if cmd & (1 << 31) != 0 { let d = rd(addr, miso_bytes); self.set_w_bytes(&d); }                       // FLASH_READ
        if cmd & (1 << 28) != 0 { let j = self.jedec; self.set_w_bytes(&j); }                                   // RDID
        if cmd & (1 << 30) != 0 { self.status |= 0x02; }                                                         // WREN
        if cmd & (1 << 29) != 0 { self.status &= !0x02; }                                                        // WRDI
        if cmd & (1 << 26) != 0 { self.status &= !0x02; }                                                        // WRSR
        // RDSR (bit 27): RD_STATUS already reflects the live status register
        if cmd & (1 << 25) != 0 {                                                                                // PP
            let n = if addr_reg >> 24 != 0 { (addr_reg >> 24) as usize } else { mosi_bytes };
            let d = self.w_bytes(n);
            for (i, b) in d.iter().enumerate() { let x = (addr & 0xff_ffff) as usize + i; if x < fsize { flash[x] &= *b; } }
            self.status &= !0x02;
        }
        if cmd & (1 << 24) != 0 { let a = (addr as usize) & !0xfff; for x in a..(a + 0x1000).min(fsize) { flash[x] = 0xff; } self.status &= !0x02; }      // SE
        if cmd & (1 << 23) != 0 { let a = (addr as usize) & !0xffff; for x in a..(a + 0x10000).min(fsize) { flash[x] = 0xff; } }    // BE
        if cmd & (1 << 22) != 0 { for b in flash.iter_mut() { *b = 0xff; } }                                                          // CE
    }
}

// ------------------------------------------------------------------ SHA accelerator (register/block mode; SHA-1/224/256)
pub struct Sha { pub mode: u32, pub h: [u32; 16], pub m: [u32; 32], pub busy: bool, ram: RegRam }
impl Sha {
    pub fn new() -> Self { Sha { mode: 2, h: [0; 16], m: [0; 32], busy: false, ram: RegRam::new() } }
    fn init(&mut self) {
        self.h = [0; 16];
        match self.mode {
            0 => self.h[..5].copy_from_slice(&[0x67452301, 0xEFCDAB89, 0x98BADCFE, 0x10325476, 0xC3D2E1F0]),
            1 => self.h[..8].copy_from_slice(&[0xc1059ed8, 0x367cd507, 0x3070dd17, 0xf70e5939, 0xffc00b31, 0x68581511, 0x64f98fa7, 0xbefa4fa4]),
            _ => self.h[..8].copy_from_slice(&[0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19]),
        }
    }
    fn compress(&mut self) {
        // message words are written by software as the bytes of the block; interpret big-endian per SHA
        let w0: Vec<u32> = self.m[..16].iter().map(|x| x.swap_bytes()).collect();
        match self.mode {
            0 => sha1_block(&mut self.h, &w0),
            _ => sha256_block(&mut self.h, &w0),
        }
    }
    pub fn read(&self, off: u32) -> u32 {
        match off {
            0x0 => self.mode,
            0x18 => self.busy as u32,
            0x2c => 0x20190402,
            0x40..=0x7c => { let i = ((off - 0x40) / 4) as usize; self.h[i].swap_bytes() }   // H regs read back as big-endian bytes in memory order
            0x80..=0xfc => self.m[((off - 0x80) / 4) as usize],
            _ => self.ram.read(off),
        }
    }
    pub fn write(&mut self, off: u32, v: u32) {
        match off {
            0x0 => self.mode = v & 7,
            0x10 => { self.init(); self.compress(); }
            0x14 => self.compress(),
            0x40..=0x7c => { let i = ((off - 0x40) / 4) as usize; self.h[i] = v.swap_bytes(); }
            0x80..=0xfc => self.m[((off - 0x80) / 4) as usize] = v,
            _ => self.ram.write(off, v),
        }
    }
}

fn sha256_block(h: &mut [u32; 16], w0: &[u32]) {
    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
        0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
        0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
        0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3, 0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2];
    let mut w = [0u32; 64];
    w[..16].copy_from_slice(&w0[..16]);
    for i in 16..64 {
        let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
        let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
        w[i] = w[i - 16].wrapping_add(s0).wrapping_add(w[i - 7]).wrapping_add(s1);
    }
    let (mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut hh) = (h[0], h[1], h[2], h[3], h[4], h[5], h[6], h[7]);
    for i in 0..64 {
        let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
        let ch = (e & f) ^ (!e & g);
        let t1 = hh.wrapping_add(s1).wrapping_add(ch).wrapping_add(K[i]).wrapping_add(w[i]);
        let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
        let maj = (a & b) ^ (a & c) ^ (b & c);
        let t2 = s0.wrapping_add(maj);
        hh = g; g = f; f = e; e = d.wrapping_add(t1); d = c; c = b; b = a; a = t1.wrapping_add(t2);
    }
    h[0] = h[0].wrapping_add(a); h[1] = h[1].wrapping_add(b); h[2] = h[2].wrapping_add(c); h[3] = h[3].wrapping_add(d);
    h[4] = h[4].wrapping_add(e); h[5] = h[5].wrapping_add(f); h[6] = h[6].wrapping_add(g); h[7] = h[7].wrapping_add(hh);
}

fn sha1_block(h: &mut [u32; 16], w0: &[u32]) {
    let mut w = [0u32; 80];
    w[..16].copy_from_slice(&w0[..16]);
    for i in 16..80 { w[i] = (w[i - 3] ^ w[i - 8] ^ w[i - 14] ^ w[i - 16]).rotate_left(1); }
    let (mut a, mut b, mut c, mut d, mut e) = (h[0], h[1], h[2], h[3], h[4]);
    for i in 0..80 {
        let (f, k) = match i { 0..=19 => ((b & c) | (!b & d), 0x5A827999), 20..=39 => (b ^ c ^ d, 0x6ED9EBA1), 40..=59 => ((b & c) | (b & d) | (c & d), 0x8F1BBCDC), _ => (b ^ c ^ d, 0xCA62C1D6) };
        let t = a.rotate_left(5).wrapping_add(f).wrapping_add(e).wrapping_add(k).wrapping_add(w[i]);
        e = d; d = c; c = b.rotate_left(30); b = a; a = t;
    }
    h[0] = h[0].wrapping_add(a); h[1] = h[1].wrapping_add(b); h[2] = h[2].wrapping_add(c); h[3] = h[3].wrapping_add(d); h[4] = h[4].wrapping_add(e);
}

// ------------------------------------------------------------------ WDEV (radio) block: only the hardware RNG register matters to us
pub struct Wdev { state: u64, ram: RegRam }
impl Wdev {
    pub fn new() -> Self { Wdev { state: 0x9E37_79B9_7F4A_7C15, ram: RegRam::new() } }
    pub fn read(&mut self, off: u32) -> u32 {
        match off {
            0x7c => { let mut x = self.state; x ^= x << 13; x ^= x >> 7; x ^= x << 17; self.state = x; (x >> 16) as u32 }   // WDEV_RND_REG (xorshift64)
            _ => self.ram.read(off),
        }
    }
    pub fn write(&mut self, off: u32, v: u32) { self.ram.write(off, v); }
}

// ------------------------------------------------------------------ I2C_MST: analog "regi2c" master (PLL / SAR ADC trim registers)
pub struct I2cMst { pub ram: RegRam, pub ana: std::collections::HashMap<u32, u8> }
impl I2cMst {
    pub fn new() -> Self { I2cMst { ram: RegRam::new(), ana: Default::default() } }
    pub fn read(&mut self, off: u32) -> u32 {
        match off {
            0x0 => {   // I2C0_CTRL: [7:0] slave, [15:8] reg, [23:16] data, [24] write, [25] busy
                let c = self.ram.read(0);
                if c & (1 << 24) == 0 { let key = c & 0xffff; let d = *self.ana.get(&key).unwrap_or(&0) as u32; (c & !(0xff << 16) & !(1 << 25)) | (d << 16) } else { c & !(1 << 25) }
            }
            // analog-block handshakes (BBPLL cal, pkdet, txdc/rxdc comparators...): the blob writes a start bit and
            // polls a done bit in 26:24; comparator sign bits 31:30 read as 0 — enough for its search loops to run
            0x40..=0x5c => (self.ram.read(off) & 0x3fff_ffff) | (7 << 24),
            _ => self.ram.read(off),
        }
    }
    pub fn write(&mut self, off: u32, v: u32) {
        if off == 0 && v & (1 << 24) != 0 { self.ana.insert(v & 0xffff, (v >> 16) as u8); }
        self.ram.write(off, v);
    }
}

// ------------------------------------------------------------------ GDMA (out/TX channels only for now) + I2S0 TX
pub const GDMA_CHANNELS: usize = 5;
pub const GDMA_CH_STRIDE: u32 = 0xC0;
pub const DMA_ADDR_BASE: u32 = 0x3FC0_0000;

#[derive(Clone, Copy, Default)]
pub struct GdmaOutCh {
    pub conf0: u32, pub conf1: u32, pub int_raw: u32, pub int_ena: u32, pub link: u32, pub peri_sel: u32, pub pri: u32,
    pub desc: u32,            // current descriptor address (0 = none)
    pub buf_pos: u32,         // bytes consumed from the current descriptor
    pub running: bool,
    pub eof_desc: u32,
}
impl GdmaOutCh {
    pub fn irq(&self) -> bool { self.int_raw & self.int_ena != 0 }
}

/// GDMA receive (IN) channel: peripheral -> memory through a descriptor chain.
#[derive(Clone, Copy, Default)]
pub struct GdmaInCh {
    pub conf0: u32, pub conf1: u32, pub int_raw: u32, pub int_ena: u32, pub link: u32, pub peri_sel: u32, pub pri: u32,
    pub desc: u32, pub eof_desc: u32, pub running: bool,
}
impl GdmaInCh { pub fn irq(&self) -> bool { self.int_raw & self.int_ena != 0 } }

pub struct Gdma { pub out: [GdmaOutCh; GDMA_CHANNELS], pub inp: [GdmaInCh; GDMA_CHANNELS], ram: RegRam, pub misc: u32 }
impl Gdma {
    pub fn new() -> Self { Gdma { out: [GdmaOutCh::default(); GDMA_CHANNELS], inp: [GdmaInCh::default(); GDMA_CHANNELS], ram: RegRam::new(), misc: 0 } }
    pub fn read(&self, off: u32) -> u32 {
        if off < GDMA_CH_STRIDE * GDMA_CHANNELS as u32 {
            let ch = (off / GDMA_CH_STRIDE) as usize; let o = off % GDMA_CH_STRIDE; let c = &self.out[ch]; let r = &self.inp[ch];
            return match o {
                0x00 => r.conf0, 0x04 => r.conf1, 0x08 => r.int_raw, 0x0c => r.int_raw & r.int_ena, 0x10 => r.int_ena,
                0x18 => 1 << 1 | 0x1f,                       // INFIFO_STATUS: empty
                0x20 => r.link & 0xF_FFFF,
                0x24 => if r.running { (r.desc & 0x3ffff) | (1 << 20) } else { 0 },
                0x28 => r.eof_desc, 0x2c => r.eof_desc, 0x30 => r.desc, 0x44 => r.pri, 0x48 => r.peri_sel,
                0x60 => c.conf0, 0x64 => c.conf1, 0x68 => c.int_raw, 0x6c => c.int_raw & c.int_ena, 0x70 => c.int_ena,
                0x78 => 0x1f | (1 << 1),   // OUTFIFO_STATUS: fifo empty
                0x80 => c.link & 0xF_FFFF,
                0x84 => if c.running { (c.desc & 0x3ffff) | (1 << 20) } else { 0 },   // OUT_STATE: dscr addr + state
                0x88 => c.eof_desc, 0x8c => c.eof_desc, 0x90 => c.desc, 0xa4 => c.pri, 0xa8 => c.peri_sel,
                _ => self.ram.read(off),
            };
        }
        match off { 0x3c8 => self.misc, 0x40c => 0x2008250, _ => self.ram.read(off) }
    }
    pub fn write(&mut self, off: u32, v: u32) {
        if off < GDMA_CH_STRIDE * GDMA_CHANNELS as u32 {
            let ch = (off / GDMA_CH_STRIDE) as usize; let o = off % GDMA_CH_STRIDE;
            if o < 0x60 {
                let r = &mut self.inp[ch];
                match o {
                    0x00 => { r.conf0 = v & !1; if v & 1 != 0 { r.running = false; r.desc = 0; } }   // IN_RST self-clears
                    0x04 => r.conf1 = v, 0x10 => r.int_ena = v, 0x14 => r.int_raw &= !v,
                    0x20 => {
                        r.link = v & 0xF_FFFF;
                        if v & (1 << 22) != 0 || v & (1 << 23) != 0 { r.desc = DMA_ADDR_BASE | (v & 0xF_FFFF); r.running = true; }   // START / RESTART
                        if v & (1 << 21) != 0 { r.running = false; }                                                                 // STOP
                    }
                    0x44 => r.pri = v, 0x48 => r.peri_sel = v & 0x3f,
                    _ => self.ram.write(off, v),
                }
                return;
            }
            let c = &mut self.out[ch];
            match o {
                0x60 => { c.conf0 = v & !1; if v & 1 != 0 { c.running = false; c.desc = 0; c.buf_pos = 0; } }   // OUT_RST self-clears
                0x64 => c.conf1 = v, 0x70 => c.int_ena = v, 0x74 => c.int_raw &= !v,
                0x80 => {
                    c.link = v & 0xF_FFFF;
                    if v & (1 << 21) != 0 || v & (1 << 22) != 0 { c.desc = DMA_ADDR_BASE | (v & 0xF_FFFF); c.buf_pos = 0; c.running = true; if c.peri_sel == 5 && std::env::var("ESP_EMU_DEBUG_LCD").is_ok() { eprintln!("[lcd] gdma out link {} at {:#010x}", if v & (1 << 22) != 0 { "RESTART" } else { "START" }, c.desc); } }   // START / RESTART
                    if v & (1 << 20) != 0 { c.running = false; }                                                                                 // STOP
                }
                0xa4 => c.pri = v, 0xa8 => c.peri_sel = v & 0x3f,
                _ => self.ram.write(off, v),
            }
            return;
        }
        match off { 0x3c8 => self.misc = v, _ => self.ram.write(off, v) }
    }
    /// Find the out channel bound to peripheral `peri` (GDMA_TRIG_PERIPH_*).
    pub fn out_channel_for(&self, peri: u32) -> Option<usize> { (0..GDMA_CHANNELS).find(|&i| self.out[i].running && self.out[i].peri_sel == peri) }
    pub fn in_channel_for(&self, peri: u32) -> Option<usize> { (0..GDMA_CHANNELS).find(|&i| self.inp[i].running && self.inp[i].peri_sel == peri) }
}

/// One DMA descriptor (dma_descriptor_t): dw0 = size[11:0] length[23:12] suc_eof[30] owner[31]; dw1 = buffer; dw2 = next
pub struct DmaDesc { pub addr: u32, pub size: u32, pub length: u32, pub eof: bool, pub owner_dma: bool, pub buf: u32, pub next: u32 }
pub fn read_desc(mem: &dyn Fn(u32) -> u32, addr: u32) -> DmaDesc {
    let dw0 = mem(addr);
    DmaDesc { addr, size: dw0 & 0xfff, length: (dw0 >> 12) & 0xfff, eof: dw0 & (1 << 30) != 0, owner_dma: dw0 & (1 << 31) != 0, buf: mem(addr + 4), next: mem(addr + 8) }
}

pub struct I2s {
    pub rx_conf: u32, pub tx_conf: u32, pub int_raw: u32, pub int_ena: u32,
    ram: RegRam,
    pub sample_rate: u32,
    pub bytes_per_frame: u32,
    acc: u64,
    /// decoded left-channel samples (host sink)
    pub pcm: Vec<i16>,
    pub frames_out: u64,
    pub tx_started_log: bool,
}
impl I2s {
    pub fn new() -> Self { I2s { rx_conf: 0, tx_conf: 0, int_raw: 0, int_ena: 0, ram: RegRam::new(), sample_rate: 44100, bytes_per_frame: 4, acc: 0, pcm: Vec::new(), frames_out: 0, tx_started_log: false } }
    pub fn tx_running(&self) -> bool { self.tx_conf & (1 << 2) != 0 }
    pub fn read(&self, off: u32) -> u32 {
        match off {
            0xc => self.int_raw, 0x10 => self.int_raw & self.int_ena, 0x14 => self.int_ena,
            0x20 => self.rx_conf & !(1 << 8) & !3, 0x24 => self.tx_conf & !(1 << 8) & !3,   // update/reset bits self-clear
            0x6c => if self.tx_running() { 0 } else { 1 },                                   // STATE: tx_idle
            0x80 => 0x2003070,
            _ => self.ram.read(off),
        }
    }
    pub fn write(&mut self, off: u32, v: u32) {
        match off {
            0x14 => self.int_ena = v, 0x18 => self.int_raw &= !v,
            0x20 => self.rx_conf = v, 0x24 => self.tx_conf = v,
            _ => self.ram.write(off, v),
        }
    }
    pub fn irq(&self) -> bool { self.int_raw & self.int_ena != 0 }
    /// Number of frames due after `cycles` CPU cycles at the configured sample rate.
    pub fn frames_due(&mut self, cycles: u64) -> u32 {
        if !self.tx_running() { self.acc = 0; return 0; }
        self.acc += cycles * self.sample_rate as u64;
        let n = (self.acc / CPU_HZ) as u32;
        self.acc %= CPU_HZ;
        n
    }
}

// ------------------------------------------------------------------ RMT (TX channels 0-3) — enough for WS2812 via the legacy driver
pub const RMT_MEM_WORDS: usize = 48;
#[derive(Clone, Default)]
pub struct RmtTxCh {
    pub conf0: u32, pub tx_lim: u32, pub carrier: u32,
    pub running: bool, pub rd: usize, pub since_thr: u32, pub wr: usize,
    pub acc_cycles: i64,
    pub bits: Vec<bool>,
}
pub struct Rmt {
    pub ch: [RmtTxCh; 4],
    pub mem: [u32; RMT_MEM_WORDS * 8],
    pub int_raw: u32, pub int_ena: u32, pub sys_conf: u32,
    ram: RegRam,
    /// completed transmissions: (channel, bits)
    pub done: Vec<(usize, Vec<bool>)>,
    pub tx_count: u64,
}
impl Rmt {
    pub fn new() -> Self { Rmt { ch: Default::default(), mem: [0; RMT_MEM_WORDS * 8], int_raw: 0, int_ena: 0, sys_conf: 0, ram: RegRam::new(), done: Vec::new(), tx_count: 0 } }
    pub fn irq(&self) -> bool { self.int_raw & self.int_ena != 0 }
    pub fn read(&self, off: u32) -> u32 {
        match off {
            0x20..=0x2c => { let c = &self.ch[((off - 0x20) / 4) as usize]; c.conf0 & !(1 << 0) & !(1 << 1) & !(1 << 2) & !(1 << 23) & !(1 << 24) }
            0x50..=0x5c => { let n = ((off - 0x50) / 4) as usize; let c = &self.ch[n]; ((c.wr as u32 + (n as u32) * 48) << 11) | if c.running { 2 << 22 } else { 0 } }
            0x70 => self.int_raw, 0x74 => self.int_raw & self.int_ena, 0x78 => self.int_ena,
            0x80..=0x8c => self.ch[((off - 0x80) / 4) as usize].carrier,
            0xa0..=0xac => self.ch[((off - 0xa0) / 4) as usize].tx_lim,
            0xc0 => self.sys_conf, 0xcc => 0x2101271,
            0x800..=0xbfc => self.mem[((off - 0x800) / 4) as usize],
            _ => self.ram.read(off),
        }
    }
    pub fn write(&mut self, off: u32, v: u32) {
        match off {
            0x0..=0xc => { let n = ((off) / 4) as usize; let c = &mut self.ch[n]; if c.wr < RMT_MEM_WORDS { self.mem[n * RMT_MEM_WORDS + c.wr] = v; c.wr += 1; } }
            0x20..=0x2c => {
                let n = ((off - 0x20) / 4) as usize;
                let c = &mut self.ch[n];
                c.conf0 = v;
                if v & (1 << 2) != 0 { c.wr = 0; }                          // APB_MEM_RST
                if v & (1 << 1) != 0 { c.rd = 0; }                          // MEM_RD_RST
                if v & (1 << 0) != 0 { c.running = true; c.rd = 0; c.since_thr = 0; c.acc_cycles = 0; c.bits.clear(); }   // TX_START
                if v & (1 << 7) != 0 { c.running = false; }                 // TX_STOP
            }
            0x78 => self.int_ena = v, 0x7c => self.int_raw &= !v,
            0x80..=0x8c => self.ch[((off - 0x80) / 4) as usize].carrier = v,
            0xa0..=0xac => self.ch[((off - 0xa0) / 4) as usize].tx_lim = v,
            0xc0 => self.sys_conf = v,
            0x800..=0xbfc => self.mem[((off - 0x800) / 4) as usize] = v,
            _ => self.ram.write(off, v),
        }
    }
    /// Advance transmitters by CPU cycles; symbols are consumed at their programmed duration.
    pub fn tick(&mut self, cycles: u64) {
        for n in 0..4 {
            let c = &mut self.ch[n];
            if !c.running { continue; }
            c.acc_cycles += cycles as i64;
            let div = ((c.conf0 >> 8) & 0xff).max(1) as i64;
            let cycles_per_tick = 3 * div;   // RMT clock = APB 80 MHz / div; CPU 240 MHz
            let mem_words = (((c.conf0 >> 16) & 0xf).max(1) as usize) * RMT_MEM_WORDS;
            let base = n * RMT_MEM_WORDS;
            let mut guard = 0;
            while c.acc_cycles > 0 && guard < 4096 {
                guard += 1;
                let sym = self.mem[base + (c.rd % mem_words)];
                let (d0, l0, d1, l1) = ((sym & 0x7fff) as i64, sym & 0x8000 != 0, ((sym >> 16) & 0x7fff) as i64, sym & 0x8000_0000 != 0);
                if d0 == 0 { // end marker
                    c.running = false;
                    self.int_raw |= 1 << n;
                    self.tx_count += 1;
                    self.done.push((n, std::mem::take(&mut c.bits)));
                    break;
                }
                // decode WS2812 bit: compare high vs low durations
                let high = if l0 { d0 } else { 0 } + if l1 { d1 } else { 0 };
                let low = if !l0 { d0 } else { 0 } + if !l1 { d1 } else { 0 };
                c.bits.push(high > low);
                c.acc_cycles -= (d0 + d1) * cycles_per_tick;
                c.rd += 1;
                c.since_thr += 1;
                if c.tx_lim & 0x1ff != 0 && c.since_thr >= c.tx_lim & 0x1ff { c.since_thr = 0; self.int_raw |= 1 << (8 + n); }   // TX_THR_EVENT
                if d1 == 0 && !l1 && c.rd % mem_words == 0 && c.conf0 & (1 << 4) == 0 { /* no wrap: stop at end of memory */ }
            }
        }
    }
}
