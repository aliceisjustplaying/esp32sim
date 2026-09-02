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
    if d.len() < 24 || d[0] != 0xE9 {
        return Err("not an ESP image (magic 0xE9 missing)".into());
    }
    let nseg = d[1] as usize;
    let entry = u32::from_le_bytes(
        d[4..8]
            .try_into()
            .expect("the checked image entry has the required length"),
    );
    let mut off = 24usize;
    let mut segments = Vec::new();
    for _ in 0..nseg {
        if off + 8 > d.len() {
            return Err("truncated image".into());
        }
        let load_addr = u32::from_le_bytes(
            d[off..off + 4]
                .try_into()
                .expect("the checked load address has the required length"),
        );
        let len = u32::from_le_bytes(
            d[off + 4..off + 8]
                .try_into()
                .expect("the checked segment length has the required length"),
        );
        segments.push(ImageSegment {
            load_addr,
            file_off: (off + 8) as u32,
            len,
        });
        off += 8 + len as usize;
    }
    Ok(AppImage { entry, segments })
}
