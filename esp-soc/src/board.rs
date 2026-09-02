//! Boards around the SoC. The SoC model emits generic events (GPIO edges, RMT symbol streams,
//! SPI bytes, LCD frames, camera requests); a `BoardModel` interprets them as the devices wired to
//! the pins and offers what the UI and the scripts need back.
use esp_periph::i2c::I2cDevice;

/// What a board does with the SoC's pin-level activity.
pub trait BoardModel {
    fn name(&self) -> &'static str;
    /// GPIO output level changes, in order.
    fn gpio_changes(&mut self, _changes: &[(u8, bool)]) {}
    /// A completed RMT transmission on channel `ch`, decoded to bits by the peripheral model.
    fn rmt_frame(&mut self, _ch: usize, _bits: &[bool]) {}
    /// Bytes a GP-SPI master (`host` = 2 or 3) shifted out on MOSI.
    fn spi_tx(&mut self, _host: u8, _data: &[u8]) {}
    fn gpio_events(&self) -> u64 {
        0
    }
    /// Devices on the I2C buses: (bus, 7-bit address, device).
    fn i2c_devices(&mut self) -> Vec<(u8, u8, Box<dyn I2cDevice>)> {
        Vec::new()
    }
    /// Give the board's camera a picture to look at (RGB888).
    fn set_camera_picture(&mut self, _p: crate::picture::Picture) {}
    /// Next camera frame as the sensor would put it on the DVP bus (YUYV), with its size. None = no camera / nothing to show.
    fn camera_frame(&mut self) -> Option<(u32, u32, std::sync::Arc<Vec<u8>>)> {
        None
    }
    /// Small RGB preview of what the camera is looking at (for the UI), if a picture is loaded.
    fn camera_preview(&self, _w: u32, _h: u32) -> Option<Vec<u8>> {
        None
    }
    /// A complete frame from the LCD_CAM RGB interface (RGB565 little-endian, `w`x`h`).
    fn lcd_frame(&mut self, _w: u32, _h: u32, _rgb565: &[u8]) {}
    /// The board's display for the UI/PNG: (width, height, RGB565 pixels, change counter).
    fn display(&self) -> Option<(u32, u32, Vec<u16>, u64)> {
        None
    }
    /// Completed display frames (for the UI's statistics line).
    fn display_frames(&self) -> u64 {
        0
    }
    /// Cheap change counter of the display (`display().3` without building the frame).
    fn display_version(&self) -> u64 {
        0
    }
    /// True if a frame should only be pushed once the pixel stream has been quiet for a push
    /// interval (a display drawn pixel by pixel), false if every new frame is complete.
    fn display_quiet_push(&self) -> bool {
        false
    }
    /// Raw display memory for a debug PNG: (pixels, columns, rows).
    fn gram(&self) -> Option<(Vec<u16>, usize, usize)> {
        None
    }
    /// LED ring / strip: colours and a change counter.
    fn leds(&self) -> Option<(&[[u8; 3]], u64)> {
        None
    }
    /// Touch input from the UI (panel coordinates).
    fn touch(&mut self, _x: u16, _y: u16, _down: bool) {}
    /// A pin by the name scripts and the UI use (`btn1`, `sw`, ...).
    fn named_pin(&self, _name: &str) -> Option<u8> {
        None
    }
    /// The rotary encoder's (CLK, DT) pins, if there is one.
    fn encoder(&self) -> Option<(u8, u8)> {
        None
    }
    /// Lines for the end-of-run report.
    fn report(&self) -> String {
        String::new()
    }
}

pub type Board = Box<dyn BoardModel>;

/// A bare module: nothing on the pins, console only.
pub struct NoBoard;
impl BoardModel for NoBoard {
    fn name(&self) -> &'static str {
        "none"
    }
}
