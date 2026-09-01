//! I2C master controller (ESP32-S3 I2C0/I2C1) and the bus devices boards hang on it.
//! The controller executes the command list (RSTART/WRITE/READ/STOP/END) written by the driver
//! at `trans_start`, moving bytes between the FIFOs and the addressed device, and raises the
//! NACK / END_DETECT / TRANS_COMPLETE interrupts the IDF `i2c_master` driver waits for.
use crate::periph::RegRam;
use std::collections::{HashMap, VecDeque};

pub trait I2cDevice {
    /// Address phase: the master addressed this device for a read (`read`) or a write. Return ACK.
    fn start(&mut self, _read: bool) -> bool {
        true
    }
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
        I2c {
            regs: RegRam::new(),
            tx: VecDeque::new(),
            rx: VecDeque::new(),
            int_raw: 0,
            int_ena: 0,
            cmd: [0; 8],
            devices: Vec::new(),
            cur: None,
            expect_addr: false,
            nack: false,
            log: std::env::var("ESP_EMU_DEBUG_I2C").is_ok(),
            transactions: 0,
        }
    }
    pub fn attach(&mut self, addr: u8, dev: Box<dyn I2cDevice>) {
        self.devices.push((addr, dev));
    }
    pub fn irq(&self) -> bool {
        self.int_raw & self.int_ena != 0
    }

    pub fn read(&mut self, off: u32) -> u32 {
        match off {
            0x08 => {
                (self.nack as u32)
                    | ((self.rx.len() as u32 & 0x3f) << 8)
                    | ((self.tx.len() as u32 & 0x3f) << 18)
            } // SR: resp_rec, rxfifo_cnt, txfifo_cnt
            0x14 => ((self.rx.len() as u32 & 0x1f) << 5) | ((self.tx.len() as u32 & 0x1f) << 15), // FIFO_ST: waddr = count, raddr = 0
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
            0x04 => {
                self.regs.write(off, v & !(1 << 5));
                if v & (1 << 5) != 0 {
                    self.run();
                }
            } // CTR.TRANS_START
            0x18 => {
                if v & (1 << 13) != 0 {
                    self.tx.clear();
                }
                if v & (1 << 12) != 0 {
                    self.rx.clear();
                }
                self.regs.write(off, v & !(3 << 12));
            }
            0x1c => {
                if self.tx.len() < 32 {
                    self.tx.push_back(v as u8);
                }
            }
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
                6 => self.expect_addr = true, // RSTART
                1 => {
                    // WRITE n bytes
                    for _ in 0..n {
                        let b = self.tx.pop_front().unwrap_or(0);
                        let ack = if self.expect_addr {
                            self.expect_addr = false;
                            let addr = b >> 1;
                            let rd = b & 1 != 0;
                            self.cur = self.devices.iter().position(|(a, _)| *a == addr);
                            if self.log {
                                eprintln!(
                                    "[i2c] start addr {:#04x} {}{}",
                                    addr,
                                    if rd { "R" } else { "W" },
                                    if self.cur.is_none() {
                                        " (no device)"
                                    } else {
                                        ""
                                    }
                                );
                            }
                            match self.cur {
                                Some(k) => self.devices[k].1.start(rd),
                                None => false,
                            }
                        } else {
                            if self.log {
                                eprintln!("[i2c]   write {:#04x}", b);
                            }
                            match self.cur {
                                Some(k) => self.devices[k].1.write(b),
                                None => false,
                            }
                        };
                        if !ack && ack_check {
                            self.nack = true;
                            self.int_raw |= INT_NACK;
                            self.cmd[i] |= 1 << 31;
                            self.cur = None;
                            return;
                        }
                    }
                }
                3 => {
                    // READ n bytes
                    for _ in 0..n {
                        let b = match self.cur {
                            Some(k) => self.devices[k].1.read(),
                            None => 0xff,
                        };
                        if self.log {
                            eprintln!("[i2c]   read  {:#04x}", b);
                        }
                        if self.rx.len() < 32 {
                            self.rx.push_back(b);
                        }
                    }
                }
                2 => {
                    // STOP
                    if let Some(k) = self.cur {
                        self.devices[k].1.stop();
                    }
                    self.cur = None;
                    self.cmd[i] |= 1 << 31;
                    self.int_raw |= INT_TRANS_COMPLETE;
                    return;
                }
                4 => {
                    self.cmd[i] |= 1 << 31;
                    self.int_raw |= INT_END_DETECT;
                    return;
                } // END: driver continues later
                _ => {
                    self.cmd[i] |= 1 << 31;
                    return;
                }
            }
            self.cmd[i] |= 1 << 31;
        }
    }
}

impl Default for I2c {
    fn default() -> Self {
        Self::new()
    }
}

// ------------------------------------------------------------------ devices

/// Generic 8-bit-register device (audio codecs etc.): first written byte selects the register,
/// following bytes / reads auto-increment.
pub struct Reg8Device {
    pub name: &'static str,
    pub regs: [u8; 256],
    ptr: u8,
    first: bool,
}
impl Reg8Device {
    pub fn new(name: &'static str, defaults: &[(u8, u8)]) -> Self {
        let mut d = Reg8Device {
            name,
            regs: [0; 256],
            ptr: 0,
            first: true,
        };
        for &(r, v) in defaults {
            d.regs[r as usize] = v;
        }
        d
    }
}
impl I2cDevice for Reg8Device {
    fn start(&mut self, read: bool) -> bool {
        if !read {
            self.first = true;
        }
        true
    }
    fn write(&mut self, b: u8) -> bool {
        if self.first {
            self.ptr = b;
            self.first = false;
        } else {
            self.regs[self.ptr as usize] = b;
            self.ptr = self.ptr.wrapping_add(1);
        }
        true
    }
    fn read(&mut self) -> u8 {
        let v = self.regs[self.ptr as usize];
        self.ptr = self.ptr.wrapping_add(1);
        v
    }
}

/// Waveshare's CH32V003 IO expander: regs 0x02 direction, 0x03 output, 0x04 input, 0x05 PWM, 0x06 ADC, 0x07 RTC.
pub struct Ch32v003 {
    pub regs: [u8; 8],
    ptr: u8,
    first: bool,
    pub writes: u64,
}
impl Ch32v003 {
    pub fn new() -> Self {
        let mut r = [0u8; 8];
        r[2] = 0xff;
        r[4] = 0xff;
        Ch32v003 {
            regs: r,
            ptr: 0,
            first: true,
            writes: 0,
        }
    }
}

impl Default for Ch32v003 {
    fn default() -> Self {
        Self::new()
    }
}
impl I2cDevice for Ch32v003 {
    fn start(&mut self, read: bool) -> bool {
        if !read {
            self.first = true;
        }
        true
    }
    fn write(&mut self, b: u8) -> bool {
        if self.first {
            self.ptr = b & 7;
            self.first = false;
        } else {
            self.regs[self.ptr as usize] = b;
            self.writes += 1;
        }
        true
    }
    fn read(&mut self) -> u8 {
        self.regs[self.ptr as usize]
    }
}

/// What the board needs to know about the sensor's configuration (written over SCCB).
#[derive(Default, Debug)]
pub struct SensorState {
    pub width: u32,
    pub height: u32,
    pub format: u8,
    pub streaming: bool,
}

/// OV5640 image sensor over SCCB: 16-bit register addresses, auto-increment.
pub struct Ov5640 {
    pub regs: HashMap<u16, u8>,
    addr: u16,
    phase: u8,
    pub writes: u64,
    state: std::sync::Arc<std::sync::Mutex<SensorState>>,
}
impl Ov5640 {
    pub fn new(state: std::sync::Arc<std::sync::Mutex<SensorState>>) -> Self {
        let mut regs = HashMap::new();
        regs.insert(0x300a, 0x56);
        regs.insert(0x300b, 0x40); // chip ID 0x5640
        regs.insert(0x3008, 0x02); // system control: normal
        regs.insert(0x302a, 0xb0); // silicon revision
        Ov5640 {
            regs,
            addr: 0,
            phase: 0,
            writes: 0,
            state,
        }
    }
    pub fn get(&self, r: u16) -> u8 {
        *self.regs.get(&r).unwrap_or(&0)
    }
    fn sync_state(&self) {
        let mut st = self
            .state
            .lock()
            .expect("OV5640 sensor state mutex poisoned");
        st.width = ((self.get(0x3808) as u32 & 0xf) << 8) | self.get(0x3809) as u32; // DVP output width
        st.height = ((self.get(0x380a) as u32 & 0x7) << 8) | self.get(0x380b) as u32; // DVP output height
        st.format = self.get(0x4300);
        st.streaming = self.get(0x3008) & 0x40 == 0;
    }
}
impl I2cDevice for Ov5640 {
    fn start(&mut self, read: bool) -> bool {
        if !read {
            self.phase = 0;
        }
        true
    }
    fn write(&mut self, b: u8) -> bool {
        match self.phase {
            0 => {
                self.addr = (b as u16) << 8;
                self.phase = 1;
            }
            1 => {
                self.addr |= b as u16;
                self.phase = 2;
            }
            _ => {
                let v = if self.addr == 0x3008 { b & !0x80 } else { b };
                self.regs.insert(self.addr, v);
                if (0x3808..=0x380b).contains(&self.addr)
                    || self.addr == 0x4300
                    || self.addr == 0x3008
                {
                    self.sync_state();
                }
                self.addr = self.addr.wrapping_add(1);
                self.writes += 1;
            }
        }
        true
    }
    fn read(&mut self) -> u8 {
        let v = self.get(self.addr);
        self.addr = self.addr.wrapping_add(1);
        v
    }
}

/// State of an ST7701S panel controller as seen through its 9-bit init SPI (D/C bit + 8 data bits).
#[derive(Default, Debug)]
pub struct St7701State {
    pub words: u64,
    pub last_cmd: u8,
    pub sleep_out: bool,
    pub display_on: bool,
    pub cmds: Vec<u8>,
}

/// TCA9554 / PCA9554 8-bit IO expander (regs: 0 input, 1 output, 2 polarity, 3 config). On the
/// Waveshare Touch-LCD-4B the panel's init SPI hangs off EXIO0 (CS), EXIO1 (MOSI), EXIO2 (CLK); the
/// device decodes that bit-banged stream into `St7701State`.
pub struct Tca9554 {
    pub regs: [u8; 4],
    ptr: u8,
    first: bool,
    panel: std::sync::Arc<std::sync::Mutex<St7701State>>,
    shift: u16,
    nbits: u8,
}
impl Tca9554 {
    pub fn new(panel: std::sync::Arc<std::sync::Mutex<St7701State>>) -> Self {
        Tca9554 {
            regs: [0xff, 0xff, 0x00, 0xff],
            ptr: 0,
            first: true,
            panel,
            shift: 0,
            nbits: 0,
        }
    }
    fn output(&mut self, old: u8, new: u8) {
        let cs = new & 1 != 0;
        let mosi = (new >> 1) & 1;
        let clk_rise = new & 4 != 0 && old & 4 == 0;
        if cs {
            self.nbits = 0;
            self.shift = 0;
            return;
        }
        if clk_rise {
            self.shift = (self.shift << 1) | mosi as u16;
            self.nbits += 1;
            if self.nbits == 9 {
                let dc = self.shift & 0x100 != 0;
                let b = self.shift as u8;
                self.nbits = 0;
                self.shift = 0;
                let mut st = self
                    .panel
                    .lock()
                    .expect("ST7701 panel state mutex poisoned");
                st.words += 1;
                if !dc {
                    st.last_cmd = b;
                    st.cmds.push(b);
                    match b {
                        0x11 => st.sleep_out = true,
                        0x10 => st.sleep_out = false,
                        0x29 => st.display_on = true,
                        0x28 => st.display_on = false,
                        _ => {}
                    }
                }
            }
        }
    }
}
impl I2cDevice for Tca9554 {
    fn start(&mut self, read: bool) -> bool {
        if !read {
            self.first = true;
        }
        true
    }
    fn write(&mut self, b: u8) -> bool {
        if self.first {
            self.ptr = b & 3;
            self.first = false;
        } else {
            let old = self.regs[1];
            self.regs[self.ptr as usize] = b;
            if self.ptr == 1 {
                self.output(old, b);
            }
        }
        true
    }
    fn read(&mut self) -> u8 {
        self.regs[self.ptr as usize]
    }
}

/// Touch state shared between the board (UI) and the GT911 model.
#[derive(Default, Debug, Clone, Copy)]
pub struct TouchState {
    pub down: bool,
    pub x: u16,
    pub y: u16,
    pub seen: bool,
    pub release_pending: bool,
}

/// Goodix GT911 capacitive touch controller: 16-bit register addresses; product ID at 0x8140,
/// config at 0x8047.., status + up to 5 points at 0x814E...
pub struct Gt911 {
    addr: u16,
    phase: u8,
    touch: std::sync::Arc<std::sync::Mutex<TouchState>>,
    pub reads: u64,
    w: u16,
    h: u16,
}
impl Gt911 {
    pub fn new(touch: std::sync::Arc<std::sync::Mutex<TouchState>>, w: u16, h: u16) -> Self {
        Gt911 {
            addr: 0,
            phase: 0,
            touch,
            reads: 0,
            w,
            h,
        }
    }
    fn reg(&self, a: u16) -> u8 {
        let mut tl = self.touch.lock().expect("GT911 touch state mutex poisoned");
        if a == 0x814e {
            // like the real controller's buffer: a touch stays readable until the host has seen it once
            if tl.release_pending && tl.seen {
                tl.down = false;
                tl.release_pending = false;
            }
            if tl.down {
                tl.seen = true;
            }
        }
        let t = *tl;
        match a {
            0x8140 => b'9',
            0x8141 => b'1',
            0x8142 => b'1',
            0x8143 => 0,
            0x8144 => 0x60,
            0x8145 => 0x10, // "911", firmware 0x1060
            0x8047 => 0x41, // config version
            0x8048 => self.w as u8,
            0x8049 => (self.w >> 8) as u8,
            0x804a => self.h as u8,
            0x804b => (self.h >> 8) as u8,
            0x804c => 5,                   // touch number
            0x814e => 0x80 | t.down as u8, // buffer ready + count
            0x814f => 0,
            0x8150 => t.x as u8,
            0x8151 => (t.x >> 8) as u8,
            0x8152 => t.y as u8,
            0x8153 => (t.y >> 8) as u8,
            0x8154 => 20,
            0x8155 => 0,
            0x8156 => 0,
            _ => 0,
        }
    }
}
impl I2cDevice for Gt911 {
    fn start(&mut self, read: bool) -> bool {
        if !read {
            self.phase = 0;
        }
        true
    }
    fn write(&mut self, b: u8) -> bool {
        match self.phase {
            0 => {
                self.addr = (b as u16) << 8;
                self.phase = 1;
            }
            1 => {
                self.addr |= b as u16;
                self.phase = 2;
            }
            _ => {
                self.addr = self.addr.wrapping_add(1);
            }
        }
        true
    }
    fn read(&mut self) -> u8 {
        let v = self.reg(self.addr);
        self.addr = self.addr.wrapping_add(1);
        self.reads += 1;
        v
    }
}

/// Hynitron CST820 touch controller used on the Waveshare Touch AMOLED 1.8 V2.
/// Its report layout is compatible with the CST816S driver used by the board firmware.
pub struct Cst820 {
    ptr: u8,
    first: bool,
    touch: std::sync::Arc<std::sync::Mutex<TouchState>>,
    pub reads: u64,
}
impl Cst820 {
    pub fn new(touch: std::sync::Arc<std::sync::Mutex<TouchState>>) -> Self {
        Cst820 {
            ptr: 0,
            first: true,
            touch,
            reads: 0,
        }
    }
    fn reg(&self, a: u8) -> u8 {
        let mut tl = self
            .touch
            .lock()
            .expect("the CST820 touch-state mutex must remain usable");
        if a == 0x02 {
            if tl.release_pending && tl.seen {
                tl.down = false;
                tl.release_pending = false;
            }
            if tl.down {
                tl.seen = true;
            }
        }
        let t = *tl;
        match a {
            0x01 => 0,
            0x02 => t.down as u8,
            0x03 => ((t.x >> 8) as u8) & 0x0f,
            0x04 => t.x as u8,
            0x05 => ((t.y >> 8) as u8) & 0x0f,
            0x06 => t.y as u8,
            0xa7 => 0xb7,
            0xa8 => 0x41,
            0xa9 => 0x02,
            _ => 0,
        }
    }
}
impl I2cDevice for Cst820 {
    fn start(&mut self, read: bool) -> bool {
        if !read {
            self.first = true;
        }
        true
    }
    fn write(&mut self, b: u8) -> bool {
        if self.first {
            self.ptr = b;
            self.first = false;
        } else {
            self.ptr = self.ptr.wrapping_add(1);
        }
        true
    }
    fn read(&mut self) -> u8 {
        let v = self.reg(self.ptr);
        self.ptr = self.ptr.wrapping_add(1);
        self.reads += 1;
        v
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn read_regs(dev: &mut Cst820, first: u8, count: usize) -> Vec<u8> {
        assert!(dev.start(false));
        assert!(dev.write(first));
        assert!(dev.start(true));
        (0..count).map(|_| dev.read()).collect()
    }
    #[test]
    fn cst820_reports_captured_identity() {
        let mut dev = Cst820::new(Default::default());
        assert_eq!(read_regs(&mut dev, 0xa7, 3), [0xb7, 0x41, 0x02]);
    }
    #[test]
    fn cst820_reports_touch_coordinates() {
        let touch = std::sync::Arc::new(std::sync::Mutex::new(TouchState {
            down: true,
            x: 0x167,
            y: 0x1bf,
            ..Default::default()
        }));
        let mut dev = Cst820::new(touch);
        assert_eq!(read_regs(&mut dev, 0x02, 5), [1, 0x01, 0x67, 0x01, 0xbf]);
    }
}
