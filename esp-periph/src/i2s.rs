//! I2S TX: the clock tree that sets the frame rate, and the sample sink the SoC's DMA pump fills.
use crate::device::{Device, WriteEffect};
use crate::regram::RegRam;

pub struct I2s {
    pub rx_conf: u32,
    pub tx_conf: u32,
    pub int_raw: u32,
    pub int_ena: u32,
    ram: RegRam,
    /// TX_CONF1 (0x2c: slot width, BCK divider), TX_CLKM_CONF (0x34: source, integer MCLK divider),
    /// TX_CLKM_DIV_CONF (0x3c: fractional MCLK divider x/y/z/yn1), TX_TDM_CTRL (0x54: slot count)
    pub tx_conf1: u32,
    pub tx_clkm_conf: u32,
    pub tx_clkm_div_conf: u32,
    pub tx_tdm_ctrl: u32,
    /// frame rate on the wire, derived from the clock registers exactly as the silicon divides
    /// its source clock; 44.1 kHz until firmware programs the clock
    pub sample_rate: u32,
    pub bytes_per_frame: u32,
    acc: u64,
    /// decoded left-channel samples (host sink)
    pub pcm: Vec<i16>,
    pub frames_out: u64,
    pub tx_started_log: bool,
    cpu_hz: u64,
}
impl I2s {
    pub fn new(cpu_hz: u64) -> Self {
        I2s {
            cpu_hz,
            rx_conf: 0,
            tx_conf: 0,
            int_raw: 0,
            int_ena: 0,
            ram: RegRam::new(),
            tx_conf1: 0,
            tx_clkm_conf: 0,
            tx_clkm_div_conf: 0,
            tx_tdm_ctrl: 0,
            sample_rate: 44100,
            bytes_per_frame: 4,
            acc: 0,
            pcm: Vec::new(),
            frames_out: 0,
            tx_started_log: false,
        }
    }
    pub fn tx_running(&self) -> bool {
        self.tx_conf & (1 << 2) != 0
    }
    pub fn read(&self, off: u32) -> u32 {
        match off {
            0xc => self.int_raw,
            0x10 => self.int_raw & self.int_ena,
            0x14 => self.int_ena,
            0x20 => self.rx_conf & !(1 << 8) & !3,
            0x24 => self.tx_conf & !(1 << 8) & !3, // update/reset bits self-clear
            0x6c => {
                if self.tx_running() {
                    0
                } else {
                    1
                }
            } // STATE: tx_idle
            0x80 => 0x2003070,
            _ => self.ram.read(off),
        }
    }
    pub fn write(&mut self, off: u32, v: u32) {
        match off {
            0x14 => self.int_ena = v,
            0x18 => self.int_raw &= !v,
            0x20 => self.rx_conf = v,
            0x24 => {
                self.tx_conf = v;
                self.update_rate();
            }
            0x2c => {
                self.tx_conf1 = v;
                self.ram.write(off, v);
                self.update_rate();
            }
            0x34 => {
                self.tx_clkm_conf = v;
                self.ram.write(off, v);
                self.update_rate();
            }
            0x3c => {
                self.tx_clkm_div_conf = v;
                self.ram.write(off, v);
                self.update_rate();
            }
            0x54 => {
                self.tx_tdm_ctrl = v;
                self.ram.write(off, v);
                self.update_rate();
            }
            _ => self.ram.write(off, v),
        }
    }
    pub fn irq(&self) -> bool {
        self.int_raw & self.int_ena != 0
    }
    /// The TX frame rate the clock tree produces:
    ///   MCLK = src / (div_num + b/a)   with b/a recovered from the x/y/z/yn1 fields the way
    ///                                  `i2s_ll_tx_set_mclk` encodes them (b = z, a = (x+1)·z + y;
    ///                                  with yn1 the fraction is 1 − b/a)
    ///   BCK  = MCLK / (bck_div_num + 1)
    ///   fs   = BCK / (slot_bits · slots)
    /// Returns None while the clock is off or unprogrammed, so the default stays in force.
    pub fn derive_rate(&self) -> Option<u32> {
        let c = self.tx_clkm_conf;
        if c & (1 << 26) == 0 {
            return None;
        } // TX_CLK_ACTIVE
        let src: u64 = match (c >> 27) & 3 {
            0 => 40_000_000,
            3 => return None,
            _ => 160_000_000,
        }; // XTAL / PLL_F160M / external
        let n = (c & 0xff) as u64;
        if n == 0 {
            return None;
        }
        let d = self.tx_clkm_div_conf;
        let (z, y, x, yn1) = (
            (d & 0x1ff) as u64,
            ((d >> 9) & 0x1ff) as u64,
            ((d >> 18) & 0x1ff) as u64,
            d & (1 << 27) != 0,
        );
        let (a, b) = if z == 0 {
            (1, 0)
        } else {
            let a = (x + 1) * z + y;
            (a, if yn1 { a - z } else { z })
        };
        let bck = ((self.tx_conf1 >> 7) & 0x3f) as u64 + 1;
        let slot_bits = (self.tx_conf1 & 0x7f) as u64 + 1; // TX_TDM_WS_WIDTH = slot width − 1
        let slots = ((self.tx_tdm_ctrl >> 16) & 0xf) as u64 + 1; // TX_TDM_TOT_CHAN_NUM = slots − 1
        let denom = (n * a + b) * bck * slot_bits * slots;
        if denom == 0 {
            return None;
        }
        let fs = (src * a + denom / 2) / denom;
        if !(1_000..=400_000).contains(&fs) {
            return None;
        }
        Some(fs as u32)
    }
    fn update_rate(&mut self) {
        if let Some(fs) = self.derive_rate() {
            self.sample_rate = fs;
        }
    }
    /// Number of frames due after `cycles` CPU cycles at the configured sample rate.
    pub fn frames_due(&mut self, cycles: u64) -> u32 {
        if !self.tx_running() {
            self.acc = 0;
            return 0;
        }
        self.acc += cycles * self.sample_rate as u64;
        let n = (self.acc / self.cpu_hz) as u32;
        self.acc %= self.cpu_hz;
        n
    }
}

impl Device for I2s {
    fn read(&mut self, off: u32) -> u32 {
        I2s::read(self, off)
    }
    fn write(&mut self, off: u32, v: u32) -> WriteEffect {
        I2s::write(self, off, v);
        WriteEffect::NONE
    }
    fn irq_sources(&self) -> u64 {
        self.irq() as u64
    }
}
