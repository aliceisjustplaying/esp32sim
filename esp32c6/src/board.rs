//! Boards around the ESP32-C6. One so far: the Waveshare ESP32-C6-LCD-1.47 the model was brought
//! up against — an ST7789 172×320 panel on SPI2, one WS2812 on GPIO 8 (RMT), a BOOT button on
//! GPIO 9 and a TF card slot on the same SPI bus (not modelled).
use esp_soc::{Board, BoardModel, NoBoard};

pub fn make_board(name: &str) -> Option<Board> {
    match name {
        "none" | "bare" => Some(Box::new(NoBoard)),
        "waveshare-c6-lcd147" | "c6-lcd147" | "lcd147" => Some(Box::new(WaveshareC6Lcd147::new())),
        _ => None,
    }
}

pub const PIN_LED: u8 = 8;
pub const PIN_BOOT: u8 = 9;
pub const PIN_LCD_CS: u8 = 14;
pub const PIN_LCD_DC: u8 = 15;
pub const PIN_LCD_RST: u8 = 21;
pub const PIN_LCD_BL: u8 = 22;

/// An ST7789 on a 4-wire SPI bus: the DC line tells commands from parameters and pixels. Only
/// the commands that place pixels are interpreted (window, RAMWR, MADCTL, COLMOD, inversion,
/// on/off, sleep); the panel-specific porch/gamma/voltage commands are accepted and ignored.
/// The RAM is the controller's full 240×320; `visible()` is the 172-column window of this
/// module, RAM columns 34..206.
pub struct St7789 {
    pub gram: Vec<u16>,
    pub madctl: u8, pub colmod: u8, pub inverted: bool, pub sleeping: bool, pub on: bool,
    pub dc: bool,
    cmd: u8, args: Vec<u8>,
    x0: u16, x1: u16, y0: u16, y1: u16, xc: u16, yc: u16, hi: Option<u8>,
    pub pixels_written: u64, pub frames: u64, pub resets: u64,
}
impl Default for St7789 { fn default() -> Self { Self::new() } }
impl St7789 {
    pub const COLS: usize = 240;
    pub const ROWS: usize = 320;
    pub const VISIBLE_COLS: usize = 172;
    pub const COL_OFFSET: usize = 34;
    pub fn new() -> Self {
        // the D/C GPIO idles low (command) until the driver first raises it
        St7789 { gram: vec![0; Self::COLS * Self::ROWS], madctl: 0, colmod: 0x66, inverted: false, sleeping: true, on: false, dc: false,
                 cmd: 0, args: Vec::new(), x0: 0, x1: 239, y0: 0, y1: 319, xc: 0, yc: 0, hi: None, pixels_written: 0, frames: 0, resets: 0 }
    }
    pub fn reset(&mut self) { let (gram, resets) = (std::mem::take(&mut self.gram), self.resets + 1); *self = Self::new(); self.gram = gram; self.resets = resets; }
    pub fn byte(&mut self, b: u8) {
        if !self.dc {
            self.cmd = b; self.args.clear(); self.hi = None;
            match b {
                0x01 => self.reset(),
                0x10 => self.sleeping = true, 0x11 => self.sleeping = false,
                0x20 => self.inverted = false, 0x21 => self.inverted = true,
                0x28 => self.on = false, 0x29 => self.on = true,
                0x2c => { self.xc = self.x0; self.yc = self.y0; self.frames += 1; }
                _ => {}
            }
            return;
        }
        match self.cmd {
            0x2a | 0x2b => {
                self.args.push(b);
                if self.args.len() == 4 {
                    let s = u16::from_be_bytes([self.args[0], self.args[1]]); let e = u16::from_be_bytes([self.args[2], self.args[3]]);
                    if self.cmd == 0x2a { self.x0 = s; self.x1 = e; self.xc = s; } else { self.y0 = s; self.y1 = e; self.yc = s; }
                }
            }
            0x36 => self.madctl = b,
            0x3a => self.colmod = b,
            0x2c => match self.hi.take() {
                None => self.hi = Some(b),
                Some(h) => { self.write_pixel(u16::from_be_bytes([h, b])); }
            },
            _ => {}
        }
    }
    fn write_pixel(&mut self, px: u16) {
        let (mut col, mut row) = (self.xc as usize, self.yc as usize);
        self.xc += 1;
        if self.xc > self.x1 { self.xc = self.x0; self.yc += 1; if self.yc > self.y1 { self.yc = self.y0; } }
        if self.madctl & 0x20 != 0 { std::mem::swap(&mut col, &mut row); }
        if self.madctl & 0x40 != 0 { col = Self::COLS - 1 - col.min(Self::COLS - 1); }
        if self.madctl & 0x80 != 0 { row = Self::ROWS - 1 - row.min(Self::ROWS - 1); }
        if col < Self::COLS && row < Self::ROWS { self.gram[row * Self::COLS + col] = px; self.pixels_written += 1; }
    }
    /// What the glass shows: the module's 172 columns, in the direction the mirrored scan puts
    /// them (firmware for this module sets MADCTL.MX), R/B swapped back when BGR order is on.
    /// INVON is not applied: on this IPS module it compensates the panel's polarity, so RAM
    /// colours are what the eye sees.
    pub fn visible(&self) -> Vec<u16> {
        let mut out = Vec::with_capacity(Self::VISIBLE_COLS * Self::ROWS);
        let bgr = self.madctl & 0x08 != 0;
        for r in 0..Self::ROWS {
            for x in 0..Self::VISIBLE_COLS {
                let c = Self::COL_OFFSET + Self::VISIBLE_COLS - 1 - x;
                let mut px = self.gram[r * Self::COLS + c];
                if bgr { px = (px & 0x07e0) | ((px & 0xf800) >> 11) | ((px & 0x001f) << 11); }
                if !self.on || self.sleeping { px = 0; }
                out.push(px);
            }
        }
        out
    }
    pub fn bbox(&self) -> Option<(usize, usize, usize, usize)> {
        let (mut c0, mut c1, mut r0, mut r1) = (usize::MAX, 0, usize::MAX, 0);
        for r in 0..Self::ROWS { for c in 0..Self::COLS { if self.gram[r * Self::COLS + c] != 0 { c0 = c0.min(c); c1 = c1.max(c); r0 = r0.min(r); r1 = r1.max(r); } } }
        if c0 == usize::MAX { None } else { Some((c0, r0, c1, r1)) }
    }
}

/// The Waveshare ESP32-C6-LCD-1.47.
pub struct WaveshareC6Lcd147 {
    pub panel: St7789,
    pub led: [[u8; 3]; 1], pub led_updates: u64,
    pub backlight: bool, pub gpio_events: u64,
}
impl Default for WaveshareC6Lcd147 { fn default() -> Self { Self::new() } }
impl WaveshareC6Lcd147 {
    pub fn new() -> Self { WaveshareC6Lcd147 { panel: St7789::new(), led: [[0; 3]], led_updates: 0, backlight: false, gpio_events: 0 } }
}
impl BoardModel for WaveshareC6Lcd147 {
    fn name(&self) -> &'static str { "waveshare-c6-lcd147" }
    fn gpio_changes(&mut self, changes: &[(u8, bool)]) {
        for &(pin, level) in changes {
            self.gpio_events += 1;
            match pin {
                PIN_LCD_DC => self.panel.dc = level,
                PIN_LCD_RST => if !level { self.panel.reset(); },
                PIN_LCD_BL => self.backlight = level,
                _ => {}
            }
        }
    }
    /// One WS2812: 24 bits, G then R then B, MSB first.
    fn rmt_frame(&mut self, _pin: u8, bits: &[bool]) {
        if bits.len() < 24 { return; }
        let byte = |i: usize| bits[i..i + 8].iter().fold(0u8, |v, &b| (v << 1) | b as u8);
        self.led[0] = [byte(8), byte(0), byte(16)];
        self.led_updates += 1;
    }
    fn spi_tx(&mut self, host: u8, data: &[u8]) { if host == 2 { for &b in data { self.panel.byte(b); } } }
    fn gpio_events(&self) -> u64 { self.gpio_events }
    fn display(&self) -> Option<(u32, u32, Vec<u16>, u64)> { Some((St7789::VISIBLE_COLS as u32, St7789::ROWS as u32, self.panel.visible(), self.panel.pixels_written)) }
    fn display_version(&self) -> u64 { self.panel.pixels_written }
    // LVGL redraws continuously through DMA: push at the regular interval, not on quiet
    fn display_frames(&self) -> u64 { self.panel.frames }
    fn gram(&self) -> Option<(Vec<u16>, usize, usize)> { Some((self.panel.gram.clone(), St7789::COLS, St7789::ROWS)) }
    fn leds(&self) -> Option<(&[[u8; 3]], u64)> { Some((&self.led, self.led_updates)) }
    fn named_pin(&self, name: &str) -> Option<u8> { match name { "boot" | "btn" | "btn1" => Some(PIN_BOOT), _ => None } }
    fn report(&self) -> String {
        let p = &self.panel;
        format!("[emu] lcd147: {} RAMWR, {} pixels, madctl={:#x} colmod={:#x} inverted={} on={} backlight={} bbox={:?}; led {:?} ({} updates); gpio events {}",
                p.frames, p.pixels_written, p.madctl, p.colmod, p.inverted, p.on, self.backlight, p.bbox(), self.led[0], self.led_updates, self.gpio_events)
    }
}
