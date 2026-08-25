//! I2C master controller (ESP32-S3 I2C0/I2C1) and the bus devices boards hang on it.
//! The controller executes the command list (RSTART/WRITE/READ/STOP/END) written by the driver
//! at `trans_start`, moving bytes between the FIFOs and the addressed device, and raises the
//! NACK / END_DETECT / TRANS_COMPLETE interrupts the IDF `i2c_master` driver waits for.
use std::collections::{HashMap, VecDeque};
use crate::periph::RegRam;

pub trait I2cDevice {
    /// Address phase: the master addressed this device for a read (`read`) or a write. Return ACK.
    fn start(&mut self, _read: bool) -> bool { true }
    /// One data byte from the master. Return ACK.
    fn write(&mut self, b: u8) -> bool;
    /// One data byte to the master.
    fn read(&mut self) -> u8;
    fn stop(&mut self) {}
}

pub const INT_END_DETECT: u32 = 1 << 3;
pub const INT_TRANS_COMPLETE: u32 = 1 << 7;
pub const INT_NACK: u32 = 1 << 10;

pub struct I2c {
    pub regs: RegRam,
    tx: VecDeque<u8>,
    rx: VecDeque<u8>,
    pub int_raw: u32,
    pub int_ena: u32,
    cmd: [u32; 8],
    devices: Vec<(u8, Box<dyn I2cDevice>)>,
    cur: Option<usize>,
    expect_addr: bool,
    nack: bool,
    pub log: bool,
    pub transactions: u64,
}

impl I2c {
    pub fn new() -> Self {
        I2c { regs: RegRam::new(), tx: VecDeque::new(), rx: VecDeque::new(), int_raw: 0, int_ena: 0, cmd: [0; 8], devices: Vec::new(), cur: None, expect_addr: false, nack: false,
              log: std::env::var("ESP_EMU_DEBUG_I2C").is_ok(), transactions: 0 }
    }
    pub fn attach(&mut self, addr: u8, dev: Box<dyn I2cDevice>) { self.devices.push((addr, dev)); }
    pub fn irq(&self) -> bool { self.int_raw & self.int_ena != 0 }

    pub fn read(&mut self, off: u32) -> u32 {
        match off {
            0x08 => (self.nack as u32) | ((self.rx.len() as u32 & 0x3f) << 8) | ((self.tx.len() as u32 & 0x3f) << 18),   // SR: resp_rec, rxfifo_cnt, txfifo_cnt
            0x14 => ((self.rx.len() as u32 & 0x1f) << 5) | ((self.tx.len() as u32 & 0x1f) << 15),                        // FIFO_ST: waddr = count, raddr = 0
            0x1c => self.rx.pop_front().unwrap_or(0) as u32,
            0x20 => self.int_raw,
            0x28 => self.int_ena,
            0x2c => self.int_raw & self.int_ena,
            0x58..=0x74 => self.cmd[((off - 0x58) / 4) as usize],
            _ => self.regs.read(off),
        }
    }

    pub fn write(&mut self, off: u32, v: u32) {
        match off {
            0x04 => { self.regs.write(off, v & !(1 << 5)); if v & (1 << 5) != 0 { self.run(); } }               // CTR.TRANS_START
            0x18 => { if v & (1 << 13) != 0 { self.tx.clear(); } if v & (1 << 12) != 0 { self.rx.clear(); } self.regs.write(off, v & !(3 << 12)); }
            0x1c => { if self.tx.len() < 32 { self.tx.push_back(v as u8); } }
            0x24 => self.int_raw &= !v,
            0x28 => self.int_ena = v,
            0x58..=0x74 => self.cmd[((off - 0x58) / 4) as usize] = v & !(1 << 31),
            _ => self.regs.write(off, v),
        }
    }

    fn run(&mut self) {
        self.nack = false;
        self.transactions += 1;
        for i in 0..8 {
            let c = self.cmd[i];
            let op = (c >> 11) & 7;
            let n = (c & 0xff) as usize;
            let ack_check = c & (1 << 8) != 0;
            match op {
                6 => self.expect_addr = true,                                            // RSTART
                1 => {                                                                   // WRITE n bytes
                    for _ in 0..n {
                        let b = self.tx.pop_front().unwrap_or(0);
                        let ack = if self.expect_addr {
                            self.expect_addr = false;
                            let addr = b >> 1; let rd = b & 1 != 0;
                            self.cur = self.devices.iter().position(|(a, _)| *a == addr);
                            if self.log { eprintln!("[i2c] start addr {:#04x} {}{}", addr, if rd { "R" } else { "W" }, if self.cur.is_none() { " (no device)" } else { "" }); }
                            match self.cur { Some(k) => self.devices[k].1.start(rd), None => false }
                        } else {
                            if self.log { eprintln!("[i2c]   write {:#04x}", b); }
                            match self.cur { Some(k) => self.devices[k].1.write(b), None => false }
                        };
                        if !ack && ack_check {
                            self.nack = true; self.int_raw |= INT_NACK; self.cmd[i] |= 1 << 31;
                            self.cur = None;
                            return;
                        }
                    }
                }
                3 => {                                                                   // READ n bytes
                    for _ in 0..n {
                        let b = match self.cur { Some(k) => self.devices[k].1.read(), None => 0xff };
                        if self.log { eprintln!("[i2c]   read  {:#04x}", b); }
                        if self.rx.len() < 32 { self.rx.push_back(b); }
                    }
                }
                2 => {                                                                   // STOP
                    if let Some(k) = self.cur { self.devices[k].1.stop(); }
                    self.cur = None; self.cmd[i] |= 1 << 31; self.int_raw |= INT_TRANS_COMPLETE;
                    return;
                }
                4 => { self.cmd[i] |= 1 << 31; self.int_raw |= INT_END_DETECT; return; } // END: driver continues later
                _ => { self.cmd[i] |= 1 << 31; return; }
            }
            self.cmd[i] |= 1 << 31;
        }
    }
}

// ------------------------------------------------------------------ devices

/// Generic 8-bit-register device (audio codecs etc.): first written byte selects the register,
/// following bytes / reads auto-increment.
pub struct Reg8Device { pub name: &'static str, pub regs: [u8; 256], ptr: u8, first: bool }
impl Reg8Device {
    pub fn new(name: &'static str, defaults: &[(u8, u8)]) -> Self { let mut d = Reg8Device { name, regs: [0; 256], ptr: 0, first: true }; for &(r, v) in defaults { d.regs[r as usize] = v; } d }
}
impl I2cDevice for Reg8Device {
    fn start(&mut self, read: bool) -> bool { if !read { self.first = true; } true }
    fn write(&mut self, b: u8) -> bool { if self.first { self.ptr = b; self.first = false; } else { self.regs[self.ptr as usize] = b; self.ptr = self.ptr.wrapping_add(1); } true }
    fn read(&mut self) -> u8 { let v = self.regs[self.ptr as usize]; self.ptr = self.ptr.wrapping_add(1); v }
}

/// Waveshare's CH32V003 IO expander: regs 0x02 direction, 0x03 output, 0x04 input, 0x05 PWM, 0x06 ADC, 0x07 RTC.
pub struct Ch32v003 { pub regs: [u8; 8], ptr: u8, first: bool, pub writes: u64 }
impl Ch32v003 {
    pub fn new() -> Self { let mut r = [0u8; 8]; r[2] = 0xff; r[4] = 0xff; Ch32v003 { regs: r, ptr: 0, first: true, writes: 0 } }
}
impl I2cDevice for Ch32v003 {
    fn start(&mut self, read: bool) -> bool { if !read { self.first = true; } true }
    fn write(&mut self, b: u8) -> bool { if self.first { self.ptr = b & 7; self.first = false; } else { self.regs[self.ptr as usize] = b; self.writes += 1; } true }
    fn read(&mut self) -> u8 { self.regs[self.ptr as usize] }
}

/// What the board needs to know about the sensor's configuration (written over SCCB).
#[derive(Default, Debug)]
pub struct SensorState { pub width: u32, pub height: u32, pub format: u8, pub streaming: bool }

/// OV5640 image sensor over SCCB: 16-bit register addresses, auto-increment.
pub struct Ov5640 { pub regs: HashMap<u16, u8>, addr: u16, phase: u8, pub writes: u64, state: std::sync::Arc<std::sync::Mutex<SensorState>> }
impl Ov5640 {
    pub fn new(state: std::sync::Arc<std::sync::Mutex<SensorState>>) -> Self {
        let mut regs = HashMap::new();
        regs.insert(0x300a, 0x56); regs.insert(0x300b, 0x40);   // chip ID 0x5640
        regs.insert(0x3008, 0x02);                              // system control: normal
        regs.insert(0x302a, 0xb0);                              // silicon revision
        Ov5640 { regs, addr: 0, phase: 0, writes: 0, state }
    }
    pub fn get(&self, r: u16) -> u8 { *self.regs.get(&r).unwrap_or(&0) }
    fn sync_state(&self) {
        let mut st = self.state.lock().unwrap();
        st.width = ((self.get(0x3808) as u32 & 0xf) << 8) | self.get(0x3809) as u32;    // DVP output width
        st.height = ((self.get(0x380a) as u32 & 0x7) << 8) | self.get(0x380b) as u32;   // DVP output height
        st.format = self.get(0x4300);
        st.streaming = self.get(0x3008) & 0x40 == 0;
    }
}
impl I2cDevice for Ov5640 {
    fn start(&mut self, read: bool) -> bool { if !read { self.phase = 0; } true }
    fn write(&mut self, b: u8) -> bool {
        match self.phase {
            0 => { self.addr = (b as u16) << 8; self.phase = 1; }
            1 => { self.addr |= b as u16; self.phase = 2; }
            _ => { let v = if self.addr == 0x3008 { b & !0x80 } else { b }; self.regs.insert(self.addr, v); if (0x3808..=0x380b).contains(&self.addr) || self.addr == 0x4300 || self.addr == 0x3008 { self.sync_state(); } self.addr = self.addr.wrapping_add(1); self.writes += 1; }
        }
        true
    }
    fn read(&mut self) -> u8 { let v = self.get(self.addr); self.addr = self.addr.wrapping_add(1); v }
}
