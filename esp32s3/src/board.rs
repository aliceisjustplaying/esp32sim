//! Boards around the SoC. The SoC model emits generic events (GPIO edges, RMT symbol streams,
//! I2S samples); a `BoardModel` interprets them as the devices wired to the pins.
//!
//! `Atech14` — the Atech 14-port board:
//!   - ST7735 160x80 TFT on bit-banged SPI: SCLK 2, CS 41, MOSI 1, DC 40
//!   - WS2812 12-LED ring on GPIO 8 (via RMT)
//!   - rotary encoder CLK 5 / DT 4 / SW 9, buttons GPIO 17 / 16 (active low)
//!   - MAX98357A I2S amp: BCLK 12, LRCLK 13, DIN 10
//!
//! `NoBoard` — a bare module: nothing on the pins (any ESP32-S3 firmware, console only).

pub use esp_soc::board::{Board, BoardModel, NoBoard};
use esp_soc::picture;

/// Board by name: `atech14` (default), `none`, `waveshare-cam`, `waveshare-lcd4b`.
pub fn make_board(name: &str) -> Option<Board> {
    match name {
        "atech14" | "atech" => Some(Box::new(Atech14::new())),
        "none" | "bare" => Some(Box::new(NoBoard)),
        "waveshare-cam" | "waveshare" => Some(Box::new(WaveshareCam::new())),
        "waveshare-lcd4b" | "lcd4b" => Some(Box::new(WaveshareLcd4b::new())),
        _ => None,
    }
}

pub const PIN_TFT_SCLK: u8 = 2;
pub const PIN_TFT_CS: u8 = 41;
pub const PIN_TFT_MOSI: u8 = 1;
pub const PIN_TFT_DC: u8 = 40;
pub const PIN_RING: u8 = 8;
pub const PIN_ENC_CLK: u8 = 5;
pub const PIN_ENC_DT: u8 = 4;
pub const PIN_ENC_SW: u8 = 9;
pub const PIN_BTN1: u8 = 17;
pub const PIN_BTN2: u8 = 16;

/// ST7735 controller model: 132x162 GRAM, address window, MADCTL, RGB565 pixels over SPI.
pub struct St7735 {
    pub gram: Vec<u16>,          // 162 rows x 132 cols, index = row*132 + col
    pub madctl: u8,
    pub colmod: u8,
    pub inverted: bool,
    pub sleeping: bool,
    pub on: bool,
    x0: u16, x1: u16, y0: u16, y1: u16,
    xc: u16, yc: u16,
    cmd: u8,
    argn: u8,
    args: [u8; 8],
    // SPI decode state
    sclk: bool, mosi: bool, cs: bool, dc: bool,
    shift: u8, nbits: u8,
    pixel_hi: Option<u8>,
    pub frames: u64,             // RAMWR commands seen
    pub pixels_written: u64,
}

impl Default for St7735 { fn default() -> Self { Self::new() } }

impl St7735 {
    pub const COLS: usize = 132;
    pub const ROWS: usize = 162;
    pub fn new() -> Self {
        St7735 { gram: vec![0; Self::COLS * Self::ROWS], madctl: 0, colmod: 6, inverted: false, sleeping: true, on: false,
                 x0: 0, x1: 131, y0: 0, y1: 161, xc: 0, yc: 0, cmd: 0, argn: 0, args: [0; 8], sclk: false, mosi: false, cs: true, dc: false,
                 shift: 0, nbits: 0, pixel_hi: None, frames: 0, pixels_written: 0 }
    }

    /// Feed one GPIO output change (in order). Returns nothing; decodes SPI on SCLK rising edges while CS is low.
    pub fn gpio(&mut self, pin: u8, level: bool) {
        match pin {
            PIN_TFT_MOSI => self.mosi = level,
            PIN_TFT_DC => self.dc = level,
            PIN_TFT_CS => { self.cs = level; if level { self.nbits = 0; self.shift = 0; } }
            PIN_TFT_SCLK => {
                let rising = level && !self.sclk;
                self.sclk = level;
                if rising && !self.cs {
                    self.shift = (self.shift << 1) | self.mosi as u8;
                    self.nbits += 1;
                    if self.nbits == 8 { let b = self.shift; self.nbits = 0; self.shift = 0; self.byte(b); }
                }
            }
            _ => {}
        }
    }

    /// A byte delivered by a hardware SPI master (DC still comes from its GPIO).
    pub fn spi_byte(&mut self, b: u8) { self.byte(b); }

    fn byte(&mut self, b: u8) {
        if !self.dc {
            self.cmd = b; self.argn = 0; self.pixel_hi = None;
            match b {
                0x01 => { self.madctl = 0; self.inverted = false; self.on = false; self.sleeping = true; }
                0x11 => self.sleeping = false, 0x10 => self.sleeping = true,
                0x20 => self.inverted = false, 0x21 => self.inverted = true,
                0x28 => self.on = false, 0x29 => self.on = true,
                0x2c => { self.xc = self.x0; self.yc = self.y0; self.frames += 1; }
                _ => {}
            }
            return;
        }
        match self.cmd {
            0x2a | 0x2b => {
                if (self.argn as usize) < 4 { self.args[self.argn as usize] = b; self.argn += 1; }
                if self.argn == 4 {
                    let s = ((self.args[0] as u16) << 8) | self.args[1] as u16;
                    let e = ((self.args[2] as u16) << 8) | self.args[3] as u16;
                    if self.cmd == 0x2a { self.x0 = s; self.x1 = e; self.xc = s; } else { self.y0 = s; self.y1 = e; self.yc = s; }
                }
            }
            0x36 => self.madctl = b,
            0x3a => self.colmod = b,
            0x2c => {
                match self.pixel_hi.take() {
                    None => self.pixel_hi = Some(b),
                    Some(hi) => { let px = ((hi as u16) << 8) | b as u16; self.write_pixel(px); }
                }
            }
            _ => {}
        }
    }

    fn write_pixel(&mut self, px: u16) {
        // address counters in the controller's frame; MADCTL MV swaps which counter is "column"
        let (mut col, mut row) = (self.xc as usize, self.yc as usize);
        if self.madctl & 0x20 != 0 { std::mem::swap(&mut col, &mut row); }
        if self.madctl & 0x40 != 0 { col = Self::COLS - 1 - col.min(Self::COLS - 1); }
        if self.madctl & 0x80 != 0 { row = Self::ROWS - 1 - row.min(Self::ROWS - 1); }
        if col < Self::COLS && row < Self::ROWS { self.gram[row * Self::COLS + col] = px; self.pixels_written += 1; }
        // advance within window (x fastest)
        if self.xc >= self.x1 { self.xc = self.x0; if self.yc >= self.y1 { self.yc = self.y0; } else { self.yc += 1; } } else { self.xc += 1; }
    }

    /// Bounding box of non-zero GRAM (for locating the panel's visible window).
    pub fn bbox(&self) -> Option<(usize, usize, usize, usize)> {
        let (mut c0, mut c1, mut r0, mut r1) = (usize::MAX, 0, usize::MAX, 0);
        for r in 0..Self::ROWS { for c in 0..Self::COLS { if self.gram[r * Self::COLS + c] != 0 { c0 = c0.min(c); c1 = c1.max(c); r0 = r0.min(r); r1 = r1.max(r); } } }
        if c1 >= c0 && r1 >= r0 && c0 != usize::MAX { Some((c0, r0, c1, r1)) } else { None }
    }

    /// Visible 160x80 landscape frame. The 0.96" panel maps GRAM cols 26..106 x rows 1..161.
    /// With the driver's rotation 3 (MADCTL MV|MX|BGR) the app's x axis runs down GRAM rows and
    /// its y axis runs right-to-left across GRAM columns.
    pub fn frame_160x80(&self) -> Vec<u16> {
        let mut out = vec![0u16; 160 * 80];
        let mv = self.madctl & 0x20 != 0;
        for y in 0..80 { for x in 0..160 {
            let (col, row) = if mv { (105 - y, 1 + x) } else { (26 + x.min(79), 1 + y) };
            let (col, row) = (col.min(Self::COLS - 1), row.min(Self::ROWS - 1));
            let mut px = self.gram[row * Self::COLS + col];
            if self.inverted { px = !px; }
            // MADCTL BGR compensates the physical panel's subpixel order; the app's RGB565 intent is what we show
            out[y * 160 + x] = px;
        } }
        out
    }

    /// Most common non-zero pixel values (for checking colour decoding).
    pub fn histogram(&self, top: usize) -> Vec<(u16, usize)> {
        let mut m: std::collections::HashMap<u16, usize> = Default::default();
        for &p in &self.gram { if p != 0 { *m.entry(p).or_insert(0) += 1; } }
        let mut v: Vec<(u16, usize)> = m.into_iter().collect(); v.sort_by_key(|a| std::cmp::Reverse(a.1)); v.truncate(top); v
    }
}

/// WS2812 ring fed by RMT symbols.
pub struct Ring { pub leds: Vec<[u8; 3]>, pub updates: u64 }
impl Ring {
    pub fn new(n: usize) -> Self { Ring { leds: vec![[0; 3]; n], updates: 0 } }
    /// Decode a WS2812 bit stream (GRB order) into LED colours.
    pub fn from_bits(&mut self, bits: &[bool]) {
        let n = bits.len() / 24;
        for i in 0..n.min(self.leds.len()) {
            let mut v = 0u32;
            for b in 0..24 { v = (v << 1) | bits[i * 24 + b] as u32; }
            self.leds[i] = [((v >> 8) & 0xff) as u8, ((v >> 16) & 0xff) as u8, (v & 0xff) as u8];   // GRB -> RGB
        }
        self.updates += 1;
    }
}

pub struct Atech14 {
    pub tft: St7735,
    pub ring: Ring,
    pub gpio_events: u64,
}

impl Default for Atech14 { fn default() -> Self { Self::new() } }

impl Atech14 {
    pub fn new() -> Self { Atech14 { tft: St7735::new(), ring: Ring::new(12), gpio_events: 0 } }
}

impl BoardModel for Atech14 {
    fn name(&self) -> &'static str { "atech14" }
    fn gpio_changes(&mut self, changes: &[(u8, bool)]) {
        for &(pin, level) in changes { self.gpio_events += 1; self.tft.gpio(pin, level); }
    }
    fn rmt_frame(&mut self, _ch: usize, bits: &[bool]) { self.ring.from_bits(bits); }
    fn spi_tx(&mut self, host: u8, data: &[u8]) { if host == 2 { for &b in data { self.tft.spi_byte(b); } } }
    fn gpio_events(&self) -> u64 { self.gpio_events }
    fn display(&self) -> Option<(u32, u32, Vec<u16>, u64)> { Some((160, 80, self.tft.frame_160x80(), self.tft.pixels_written)) }
    fn display_version(&self) -> u64 { self.tft.pixels_written }
    fn display_quiet_push(&self) -> bool { true }
    fn display_frames(&self) -> u64 { self.tft.frames }
    fn gram(&self) -> Option<(Vec<u16>, usize, usize)> { Some((self.tft.gram.clone(), St7735::COLS, St7735::ROWS)) }
    fn leds(&self) -> Option<(&[[u8; 3]], u64)> { Some((&self.ring.leds, self.ring.updates)) }
    fn named_pin(&self, name: &str) -> Option<u8> { match name { "btn1" => Some(PIN_BTN1), "btn2" => Some(PIN_BTN2), "sw" | "knob" => Some(PIN_ENC_SW), _ => None } }
    fn encoder(&self) -> Option<(u8, u8)> { Some((PIN_ENC_CLK, PIN_ENC_DT)) }
    fn report(&self) -> String {
        let t = &self.tft;
        format!("[emu] tft: {} RAMWR, {} pixels, madctl={:#x} inverted={} on={} bbox={:?} top colours {:x?}; gpio events {}\n[emu] ring: {} updates, leds {:?}",
                t.frames, t.pixels_written, t.madctl, t.inverted, t.on, t.bbox(), t.histogram(5), self.gpio_events, self.ring.updates, &self.ring.leds[..4])
    }
}

/// Waveshare ESP32-S3-CAM-OV5640: OV5640 on the LCD_CAM DVP port (SCCB on I2C0 GPIO 7/8),
/// CH32V003 IO expander, ES8311 speaker codec + ES7210 mic ADC on I2C0, audio on I2S0
/// (MCLK 10, BCLK 11, LRCLK 12, DIN 13, DOUT 14), buttons GPIO 0 / 15.
pub struct WaveshareCam { pub gpio_events: u64, pub preview_dirty: bool, sensor: std::sync::Arc<std::sync::Mutex<crate::i2c::SensorState>>, picture: Option<picture::Picture>, frame: Option<(u32, u32, std::sync::Arc<Vec<u8>>)>, pub frames: u64 }
impl Default for WaveshareCam { fn default() -> Self { Self::new() } }

impl WaveshareCam { pub fn new() -> Self { WaveshareCam { gpio_events: 0, preview_dirty: false, sensor: Default::default(), picture: None, frame: None, frames: 0 } } }
impl BoardModel for WaveshareCam {
    fn name(&self) -> &'static str { "waveshare-cam" }
    fn gpio_changes(&mut self, changes: &[(u8, bool)]) { self.gpio_events += changes.len() as u64; }
    fn gpio_events(&self) -> u64 { self.gpio_events }
    fn set_camera_picture(&mut self, p: picture::Picture) { self.picture = Some(p); self.frame = None; self.preview_dirty = true; }
    fn camera_preview(&self, w: u32, h: u32) -> Option<Vec<u8>> {
        let p = self.picture.as_ref()?;
        let mut out = Vec::with_capacity((w * h * 3) as usize);
        for y in 0..h { let sy = (y as u64 * p.h as u64 / h as u64) as usize; for x in 0..w { let sx = (x as u64 * p.w as u64 / w as u64) as usize; let o = (sy * p.w as usize + sx) * 3; out.extend_from_slice(&p.rgb[o..o + 3]); } }
        Some(out)
    }
    fn camera_frame(&mut self) -> Option<(u32, u32, std::sync::Arc<Vec<u8>>)> {
        let (w, h) = { let s = self.sensor.lock().unwrap(); (s.width, s.height) };
        if w == 0 || h == 0 { return None; }
        let stale = match &self.frame { Some((fw, fh, _)) => *fw != w || *fh != h, None => true };
        if stale {
            let p = self.picture.as_ref()?;
            self.frame = Some((w, h, std::sync::Arc::new(picture::to_yuyv(p, w, h))));
        }
        self.frames += 1;
        self.frame.clone()
    }
    fn i2c_devices(&mut self) -> Vec<(u8, u8, Box<dyn crate::i2c::I2cDevice>)> {
        use crate::i2c::*;
        vec![
            (0, 0x24, Box::new(Ch32v003::new())),
            (0, 0x3c, Box::new(Ov5640::new(self.sensor.clone()))),
            (0, 0x18, Box::new(Reg8Device::new("es8311", &[(0xfd, 0x83), (0xfe, 0x11)]))),
            (0, 0x40, Box::new(Reg8Device::new("es7210", &[(0x3d, 0x72), (0x3e, 0x10)]))),
        ]
    }
}

/// Waveshare ESP32-S3-Touch-LCD-4B: ST7701S 480x480 on the LCD_CAM RGB bus (16-bit, DE 17, VSYNC 3,
/// HSYNC 46, PCLK 9), its init SPI bit-banged through a TCA9554 (I2C0 @0x20, SDA 47 / SCL 48),
/// GT911 touch @0x14, ES8311/ES7210 codecs on I2S0 (MCLK 5, BCLK 16), backlight LEDC on GPIO 4.
pub struct WaveshareLcd4b {
    pub gpio_events: u64, pub w: u32, pub h: u32, pub frame: Vec<u16>, pub frames: u64,
    pub panel: std::sync::Arc<std::sync::Mutex<crate::i2c::St7701State>>,
    pub touch_state: std::sync::Arc<std::sync::Mutex<crate::i2c::TouchState>>,
}
impl Default for WaveshareLcd4b { fn default() -> Self { Self::new() } }

impl WaveshareLcd4b {
    pub fn new() -> Self { WaveshareLcd4b { gpio_events: 0, w: 480, h: 480, frame: vec![0; 480 * 480], frames: 0, panel: Default::default(), touch_state: Default::default() } }
}
impl BoardModel for WaveshareLcd4b {
    fn name(&self) -> &'static str { "waveshare-lcd4b" }
    fn gpio_changes(&mut self, changes: &[(u8, bool)]) { self.gpio_events += changes.len() as u64; }
    fn gpio_events(&self) -> u64 { self.gpio_events }
    fn i2c_devices(&mut self) -> Vec<(u8, u8, Box<dyn crate::i2c::I2cDevice>)> {
        use crate::i2c::*;
        vec![
            (0, 0x20, Box::new(Tca9554::new(self.panel.clone()))),
            (0, 0x14, Box::new(Gt911::new(self.touch_state.clone(), 480, 480))),
            (0, 0x18, Box::new(Reg8Device::new("es8311", &[(0xfd, 0x83), (0xfe, 0x11)]))),
            (0, 0x40, Box::new(Reg8Device::new("es7210", &[(0x3d, 0x72), (0x3e, 0x10)]))),
        ]
    }
    fn lcd_frame(&mut self, w: u32, h: u32, rgb565: &[u8]) {
        if (w, h) != (self.w, self.h) { self.w = w; self.h = h; self.frame = vec![0; (w * h) as usize]; }
        for (i, px) in rgb565.as_chunks::<2>().0.iter().enumerate().take(self.frame.len()) { self.frame[i] = u16::from_le_bytes([px[0], px[1]]); }
        self.frames += 1;
    }
    fn display(&self) -> Option<(u32, u32, Vec<u16>, u64)> { Some((self.w, self.h, self.frame.clone(), self.frames)) }
    fn display_version(&self) -> u64 { self.frames }
    fn display_frames(&self) -> u64 { self.frames }
    fn touch(&mut self, x: u16, y: u16, down: bool) {
        let mut t = self.touch_state.lock().unwrap(); t.x = x; t.y = y;
        if down { t.down = true; t.seen = false; t.release_pending = false; } else if t.seen { t.down = false; } else { t.release_pending = true; }
    }
}
