//! Tiny image loaders for camera sources: binary PPM (P6) and uncompressed 24/32-bit BMP.
pub struct Picture {
    pub w: u32,
    pub h: u32,
    pub rgb: Vec<u8>,
}

pub fn load(path: &str) -> Result<Picture, String> {
    let d = std::fs::read(path).map_err(|e| format!("{}: {}", path, e))?;
    parse(&d).map_err(|e| format!("{}: {}", path, e))
}

/// Decode an image already in memory (the wasm build gets files from the page).
pub fn parse(d: &[u8]) -> Result<Picture, String> {
    if d.starts_with(b"P6") {
        return ppm(d);
    }
    if d.starts_with(b"BM") {
        return bmp(d);
    }
    Err("unsupported image (use PPM P6 or 24/32-bit BMP; `sips -s format bmp in.png --out out.bmp`)".into())
}

fn ppm(d: &[u8]) -> Result<Picture, String> {
    let mut i = 2;
    let mut nums = Vec::new();
    while nums.len() < 3 {
        while i < d.len() && (d[i] as char).is_whitespace() {
            i += 1;
        }
        if i < d.len() && d[i] == b'#' {
            while i < d.len() && d[i] != b'\n' {
                i += 1;
            }
            continue;
        }
        let s = i;
        while i < d.len() && d[i].is_ascii_digit() {
            i += 1;
        }
        if s == i {
            return Err("ppm: bad header".into());
        }
        nums.push(
            std::str::from_utf8(&d[s..i])
                .unwrap()
                .parse::<u32>()
                .map_err(|e| e.to_string())?,
        );
    }
    i += 1;
    let (w, h) = (nums[0], nums[1]);
    let n = (w * h * 3) as usize;
    if d.len() < i + n {
        return Err("ppm: truncated".into());
    }
    Ok(Picture {
        w,
        h,
        rgb: d[i..i + n].to_vec(),
    })
}

fn bmp(d: &[u8]) -> Result<Picture, String> {
    let u32le = |o: usize| u32::from_le_bytes([d[o], d[o + 1], d[o + 2], d[o + 3]]);
    let off = u32le(10) as usize;
    let w = u32le(18) as i32;
    let h = u32le(22) as i32;
    let bpp = u16::from_le_bytes([d[28], d[29]]) as usize;
    if bpp != 24 && bpp != 32 {
        return Err(format!("bmp: {} bpp not supported", bpp));
    }
    let (wu, hu, flip) = (w.unsigned_abs(), h.unsigned_abs(), h > 0);
    let stride = ((wu as usize * bpp / 8) + 3) & !3;
    let mut rgb = vec![0u8; (wu * hu * 3) as usize];
    for y in 0..hu as usize {
        let src_row = if flip { hu as usize - 1 - y } else { y };
        for x in 0..wu as usize {
            let p = off + src_row * stride + x * bpp / 8;
            if p + 2 >= d.len() {
                return Err("bmp: truncated".into());
            }
            let o = (y * wu as usize + x) * 3;
            rgb[o] = d[p + 2];
            rgb[o + 1] = d[p + 1];
            rgb[o + 2] = d[p];
        }
    }
    Ok(Picture { w: wu, h: hu, rgb })
}

/// Nearest-neighbour resample to `w`x`h` and pack as YUYV (BT.601 full range), the OV5640's YUV422 output.
pub fn to_yuyv(p: &Picture, w: u32, h: u32) -> Vec<u8> {
    let mut out = vec![0u8; (w * h * 2) as usize];
    let yuv = |r: u8, g: u8, b: u8| -> (u8, u8, u8) {
        let (r, g, b) = (r as f32, g as f32, b as f32);
        let y = 0.299 * r + 0.587 * g + 0.114 * b;
        let u = -0.169 * r - 0.331 * g + 0.5 * b + 128.0;
        let v = 0.5 * r - 0.419 * g - 0.081 * b + 128.0;
        (
            y.clamp(0.0, 255.0) as u8,
            u.clamp(0.0, 255.0) as u8,
            v.clamp(0.0, 255.0) as u8,
        )
    };
    for y in 0..h {
        let sy = (y as u64 * p.h as u64 / h as u64) as usize;
        for x2 in 0..w / 2 {
            let mut px = [(0u8, 0u8, 0u8); 2];
            for k in 0..2 {
                let x = x2 * 2 + k as u32;
                let sx = (x as u64 * p.w as u64 / w as u64) as usize;
                let o = (sy * p.w as usize + sx) * 3;
                px[k] = yuv(p.rgb[o], p.rgb[o + 1], p.rgb[o + 2]);
            }
            let o = ((y * w + x2 * 2) * 2) as usize;
            out[o] = px[0].0;
            out[o + 1] = ((px[0].1 as u16 + px[1].1 as u16) / 2) as u8;
            out[o + 2] = px[1].0;
            out[o + 3] = ((px[0].2 as u16 + px[1].2 as u16) / 2) as u8;
        }
    }
    out
}
