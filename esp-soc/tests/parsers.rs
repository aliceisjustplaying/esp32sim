//! Anything a user or a page can hand the loaders must come back as an error, never a panic:
//! random bytes, truncations, headers that point past the end.
use esp_soc::{elf, image, picture};

struct Rng(u64);
impl Rng { fn next(&mut self) -> u64 { self.0 ^= self.0 << 13; self.0 ^= self.0 >> 7; self.0 ^= self.0 << 17; self.0 } fn bytes(&mut self, n: usize) -> Vec<u8> { (0..n).map(|_| self.next() as u8).collect() } }

fn no_panic(name: &str, f: &dyn Fn(&[u8]) -> bool, inputs: &[Vec<u8>]) {
    for (i, input) in inputs.iter().enumerate() {
        let ok = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| f(input))).is_ok();
        assert!(ok, "{} panicked on input #{} ({} bytes: {:02x?}...)", name, i, input.len(), &input[..input.len().min(16)]);
    }
}

fn inputs() -> Vec<Vec<u8>> {
    let mut r = Rng(0x9E37_79B9_7F4A_7C15);
    let mut v: Vec<Vec<u8>> = vec![vec![], vec![0], vec![0xff; 3], vec![0; 64]];
    for _ in 0..300 { let n = (r.next() % 4096) as usize; v.push(r.bytes(n)); }
    // plausible headers with lengths and offsets pointing anywhere
    let mut elf = b"\x7fELF\x01\x01\x01\x00".to_vec(); elf.resize(52, 0);
    for i in 0..40 { let mut e = elf.clone(); let k = r.next() as usize % e.len(); e[k] = r.next() as u8; e[28] = (r.next() % 256) as u8; e[44] = (r.next() % 256) as u8; v.push(e); if i % 2 == 0 { v.push(elf[..r.next() as usize % 52].to_vec()); } }
    let mut img = vec![0xe9u8]; img.resize(24, 0); img[1] = 3;
    for _ in 0..40 { let mut e = img.clone(); let k = r.next() as usize % e.len(); e[k] = r.next() as u8; let n = r.next() as usize % 200; e.extend(r.bytes(n)); v.push(e); }
    let mut ppm = b"P6\n4 4\n255\n".to_vec(); ppm.extend(r.bytes(20));
    v.push(ppm); v.push(b"P6\n99999999 99999999\n255\n".to_vec()); v.push(b"BM".to_vec());
    let mut bmp = b"BM".to_vec(); bmp.resize(54, 0); bmp[18] = 200; bmp[22] = 200; v.push(bmp);
    v
}

#[test] fn elf_parse_never_panics() { no_panic("elf::parse", &|d| elf::parse(d).is_ok(), &inputs()); }
#[test] fn image_parse_never_panics() { no_panic("image::parse", &|d| image::parse(d).is_ok(), &inputs()); }
#[test] fn picture_parse_never_panics() { no_panic("picture::parse", &|d| picture::parse(d).is_ok(), &inputs()); }

/// Real images still parse: a truncated one must fail, not panic, and the whole one must parse.
#[test]
fn committed_images_and_their_truncations() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap().to_path_buf();
    let app = std::fs::read(root.join("web/wasm/fw/public/hello_world.bin")).unwrap();
    assert!(image::parse(&app).is_ok());
    let cuts: Vec<Vec<u8>> = (0..app.len().min(4096)).step_by(37).map(|n| app[..n].to_vec()).collect();
    no_panic("image::parse (truncated)", &|d| image::parse(d).is_ok(), &cuts);
}
