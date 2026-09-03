//! ESP-IDF application image format (what esptool writes to flash).
pub struct ImageSegment {
    pub load_addr: u32,
    /// offset of the segment data from the start of the image
    pub file_off: u32,
    pub len: u32,
}

pub struct AppImage {
    pub entry: u32,
    pub segments: Vec<ImageSegment>,
}

pub fn parse(d: &[u8]) -> Result<AppImage, String> {
    if d.len() < 24 || d[0] != 0xE9 { return Err("not an ESP image (magic 0xE9 missing)".into()); }
    let nseg = d[1] as usize;
    let entry = u32::from_le_bytes([d[4], d[5], d[6], d[7]]);
    let mut off = 24usize;
    let mut segments = Vec::new();
    for _ in 0..nseg {
        if off + 8 > d.len() { return Err("truncated image".into()); }
        let load_addr = u32::from_le_bytes([d[off], d[off + 1], d[off + 2], d[off + 3]]);
        let len = u32::from_le_bytes([d[off + 4], d[off + 5], d[off + 6], d[off + 7]]);
        segments.push(ImageSegment { load_addr, file_off: (off + 8) as u32, len });
        off += 8 + len as usize;
    }
    Ok(AppImage { entry, segments })
}
