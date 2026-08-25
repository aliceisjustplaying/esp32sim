//! The small crypto primitives the WPA2 four-way handshake needs, implemented here so the emulator
//! stays dependency-free: SHA-1, HMAC-SHA1, PBKDF2, the 802.11 PRF and AES-128 key wrap (RFC 3394).
//! Only the access-point side of the handshake uses them; bulk data stays in the clear because the
//! emulated MAC does no CCMP (as far as firmware is concerned the hardware did it).

pub fn sha1(data: &[u8]) -> [u8; 20] {
    let mut h: [u32; 5] = [0x67452301, 0xEFCDAB89, 0x98BADCFE, 0x10325476, 0xC3D2E1F0];
    let mut msg = data.to_vec();
    let bits = (data.len() as u64) * 8;
    msg.push(0x80);
    while msg.len() % 64 != 56 { msg.push(0); }
    msg.extend_from_slice(&bits.to_be_bytes());
    for block in msg.chunks(64) {
        let mut w = [0u32; 80];
        for i in 0..16 { w[i] = u32::from_be_bytes([block[4 * i], block[4 * i + 1], block[4 * i + 2], block[4 * i + 3]]); }
        for i in 16..80 { w[i] = (w[i - 3] ^ w[i - 8] ^ w[i - 14] ^ w[i - 16]).rotate_left(1); }
        let (mut a, mut b, mut c, mut d, mut e) = (h[0], h[1], h[2], h[3], h[4]);
        for i in 0..80 {
            let (f, k) = match i {
                0..=19 => ((b & c) | ((!b) & d), 0x5A827999u32),
                20..=39 => (b ^ c ^ d, 0x6ED9EBA1),
                40..=59 => ((b & c) | (b & d) | (c & d), 0x8F1BBCDC),
                _ => (b ^ c ^ d, 0xCA62C1D6),
            };
            let t = a.rotate_left(5).wrapping_add(f).wrapping_add(e).wrapping_add(k).wrapping_add(w[i]);
            e = d; d = c; c = b.rotate_left(30); b = a; a = t;
        }
        h[0] = h[0].wrapping_add(a); h[1] = h[1].wrapping_add(b); h[2] = h[2].wrapping_add(c);
        h[3] = h[3].wrapping_add(d); h[4] = h[4].wrapping_add(e);
    }
    let mut out = [0u8; 20];
    for i in 0..5 { out[4 * i..4 * i + 4].copy_from_slice(&h[i].to_be_bytes()); }
    out
}

pub fn hmac_sha1(key: &[u8], data: &[u8]) -> [u8; 20] {
    let mut k = [0u8; 64];
    if key.len() > 64 { k[..20].copy_from_slice(&sha1(key)); } else { k[..key.len()].copy_from_slice(key); }
    let mut inner = Vec::with_capacity(64 + data.len());
    let mut outer = Vec::with_capacity(84);
    for i in 0..64 { inner.push(k[i] ^ 0x36); outer.push(k[i] ^ 0x5c); }
    inner.extend_from_slice(data);
    outer.extend_from_slice(&sha1(&inner));
    sha1(&outer)
}

/// PBKDF2-HMAC-SHA1 — the WPA passphrase-to-PMK mapping (4096 iterations, SSID as salt).
pub fn pbkdf2_sha1(password: &[u8], salt: &[u8], iterations: u32, out_len: usize) -> Vec<u8> {
    let mut out = Vec::with_capacity(out_len);
    let mut block = 1u32;
    while out.len() < out_len {
        let mut salted = salt.to_vec();
        salted.extend_from_slice(&block.to_be_bytes());
        let mut u = hmac_sha1(password, &salted);
        let mut acc = u;
        for _ in 1..iterations {
            u = hmac_sha1(password, &u);
            for i in 0..20 { acc[i] ^= u[i]; }
        }
        out.extend_from_slice(&acc);
        block += 1;
    }
    out.truncate(out_len);
    out
}

/// IEEE 802.11 PRF: HMAC-SHA1 expanded to `bits` bits over label || 0x00 || data || counter.
pub fn prf(key: &[u8], label: &str, data: &[u8], bits: usize) -> Vec<u8> {
    let mut out = Vec::new();
    let mut i = 0u8;
    while out.len() * 8 < bits {
        let mut buf = Vec::with_capacity(label.len() + data.len() + 2);
        buf.extend_from_slice(label.as_bytes()); buf.push(0);
        buf.extend_from_slice(data); buf.push(i);
        out.extend_from_slice(&hmac_sha1(key, &buf));
        i += 1;
    }
    out.truncate((bits + 7) / 8);
    out
}

// ------------------------------------------------------------------ AES-128 (encrypt only)

const SBOX: [u8; 256] = [
    0x63,0x7c,0x77,0x7b,0xf2,0x6b,0x6f,0xc5,0x30,0x01,0x67,0x2b,0xfe,0xd7,0xab,0x76,
    0xca,0x82,0xc9,0x7d,0xfa,0x59,0x47,0xf0,0xad,0xd4,0xa2,0xaf,0x9c,0xa4,0x72,0xc0,
    0xb7,0xfd,0x93,0x26,0x36,0x3f,0xf7,0xcc,0x34,0xa5,0xe5,0xf1,0x71,0xd8,0x31,0x15,
    0x04,0xc7,0x23,0xc3,0x18,0x96,0x05,0x9a,0x07,0x12,0x80,0xe2,0xeb,0x27,0xb2,0x75,
    0x09,0x83,0x2c,0x1a,0x1b,0x6e,0x5a,0xa0,0x52,0x3b,0xd6,0xb3,0x29,0xe3,0x2f,0x84,
    0x53,0xd1,0x00,0xed,0x20,0xfc,0xb1,0x5b,0x6a,0xcb,0xbe,0x39,0x4a,0x4c,0x58,0xcf,
    0xd0,0xef,0xaa,0xfb,0x43,0x4d,0x33,0x85,0x45,0xf9,0x02,0x7f,0x50,0x3c,0x9f,0xa8,
    0x51,0xa3,0x40,0x8f,0x92,0x9d,0x38,0xf5,0xbc,0xb6,0xda,0x21,0x10,0xff,0xf3,0xd2,
    0xcd,0x0c,0x13,0xec,0x5f,0x97,0x44,0x17,0xc4,0xa7,0x7e,0x3d,0x64,0x5d,0x19,0x73,
    0x60,0x81,0x4f,0xdc,0x22,0x2a,0x90,0x88,0x46,0xee,0xb8,0x14,0xde,0x5e,0x0b,0xdb,
    0xe0,0x32,0x3a,0x0a,0x49,0x06,0x24,0x5c,0xc2,0xd3,0xac,0x62,0x91,0x95,0xe4,0x79,
    0xe7,0xc8,0x37,0x6d,0x8d,0xd5,0x4e,0xa9,0x6c,0x56,0xf4,0xea,0x65,0x7a,0xae,0x08,
    0xba,0x78,0x25,0x2e,0x1c,0xa6,0xb4,0xc6,0xe8,0xdd,0x74,0x1f,0x4b,0xbd,0x8b,0x8a,
    0x70,0x3e,0xb5,0x66,0x48,0x03,0xf6,0x0e,0x61,0x35,0x57,0xb9,0x86,0xc1,0x1d,0x9e,
    0xe1,0xf8,0x98,0x11,0x69,0xd9,0x8e,0x94,0x9b,0x1e,0x87,0xe9,0xce,0x55,0x28,0xdf,
    0x8c,0xa1,0x89,0x0d,0xbf,0xe6,0x42,0x68,0x41,0x99,0x2d,0x0f,0xb0,0x54,0xbb,0x16];

fn xtime(b: u8) -> u8 { (b << 1) ^ if b & 0x80 != 0 { 0x1b } else { 0 } }

fn expand_key(key: &[u8; 16]) -> [[u8; 16]; 11] {
    let mut rk = [[0u8; 16]; 11];
    rk[0].copy_from_slice(key);
    let mut rcon = 1u8;
    for r in 1..11 {
        let prev = rk[r - 1];
        let mut t = [prev[13], prev[14], prev[15], prev[12]];
        for b in t.iter_mut() { *b = SBOX[*b as usize]; }
        t[0] ^= rcon;
        rcon = xtime(rcon);
        for i in 0..4 { rk[r][i] = prev[i] ^ t[i]; }
        for i in 4..16 { rk[r][i] = prev[i] ^ rk[r][i - 4]; }
    }
    rk
}

/// Inverse S-box, derived once from the forward one.
fn inv_sbox() -> [u8; 256] { let mut t = [0u8; 256]; for i in 0..256 { t[SBOX[i] as usize] = i as u8; } t }
fn mul(a: u8, b: u8) -> u8 {
    let (mut a, mut b, mut r) = (a, b, 0u8);
    while b != 0 { if b & 1 != 0 { r ^= a; } a = xtime(a); b >>= 1; }
    r
}

/// AES with any legal key length, both directions — what the ESP32-S3 AES accelerator provides.
pub fn aes_block(key: &[u8], block: &[u8; 16], decrypt: bool) -> [u8; 16] {
    let nk = key.len() / 4;
    let nr = nk + 6;
    // key expansion
    let mut w = vec![[0u8; 4]; 4 * (nr + 1)];
    for i in 0..nk { w[i].copy_from_slice(&key[4 * i..4 * i + 4]); }
    let mut rcon = 1u8;
    for i in nk..4 * (nr + 1) {
        let mut t = w[i - 1];
        if i % nk == 0 {
            t = [SBOX[t[1] as usize] ^ rcon, SBOX[t[2] as usize], SBOX[t[3] as usize], SBOX[t[0] as usize]];
            rcon = xtime(rcon);
        } else if nk > 6 && i % nk == 4 {
            for b in t.iter_mut() { *b = SBOX[*b as usize]; }
        }
        for j in 0..4 { w[i][j] = w[i - nk][j] ^ t[j]; }
    }
    let rk = |round: usize| -> [u8; 16] {
        let mut k = [0u8; 16];
        for c in 0..4 { k[4 * c..4 * c + 4].copy_from_slice(&w[4 * round + c]); }
        k
    };
    let inv = inv_sbox();
    let mut s = *block;
    if !decrypt {
        let k = rk(0); for i in 0..16 { s[i] ^= k[i]; }
        for round in 1..=nr {
            for b in s.iter_mut() { *b = SBOX[*b as usize]; }
            let t = s; for c in 0..4 { for r in 0..4 { s[4 * c + r] = t[4 * ((c + r) % 4) + r]; } }
            if round != nr {
                for c in 0..4 {
                    let col = [s[4 * c], s[4 * c + 1], s[4 * c + 2], s[4 * c + 3]];
                    let x = col[0] ^ col[1] ^ col[2] ^ col[3];
                    for r in 0..4 { s[4 * c + r] = col[r] ^ x ^ xtime(col[r] ^ col[(r + 1) % 4]); }
                }
            }
            let k = rk(round); for i in 0..16 { s[i] ^= k[i]; }
        }
    } else {
        let k = rk(nr); for i in 0..16 { s[i] ^= k[i]; }
        for round in (0..nr).rev() {
            let t = s; for c in 0..4 { for r in 0..4 { s[4 * ((c + r) % 4) + r] = t[4 * c + r]; } }   // InvShiftRows
            for b in s.iter_mut() { *b = inv[*b as usize]; }
            let k = rk(round); for i in 0..16 { s[i] ^= k[i]; }
            if round != 0 {
                for c in 0..4 {                                                                        // InvMixColumns
                    let col = [s[4 * c], s[4 * c + 1], s[4 * c + 2], s[4 * c + 3]];
                    s[4 * c] = mul(col[0], 14) ^ mul(col[1], 11) ^ mul(col[2], 13) ^ mul(col[3], 9);
                    s[4 * c + 1] = mul(col[0], 9) ^ mul(col[1], 14) ^ mul(col[2], 11) ^ mul(col[3], 13);
                    s[4 * c + 2] = mul(col[0], 13) ^ mul(col[1], 9) ^ mul(col[2], 14) ^ mul(col[3], 11);
                    s[4 * c + 3] = mul(col[0], 11) ^ mul(col[1], 13) ^ mul(col[2], 9) ^ mul(col[3], 14);
                }
            }
        }
    }
    s
}

pub fn aes128_encrypt_block(key: &[u8; 16], block: &[u8; 16]) -> [u8; 16] {
    let rk = expand_key(key);
    let mut s = *block;
    for i in 0..16 { s[i] ^= rk[0][i]; }
    for round in 1..11 {
        for b in s.iter_mut() { *b = SBOX[*b as usize]; }
        // ShiftRows (column-major state: byte i is row i%4, column i/4)
        let t = s;
        for c in 0..4 { for r in 0..4 { s[4 * c + r] = t[4 * ((c + r) % 4) + r]; } }
        if round != 10 {
            for c in 0..4 {
                let col = [s[4 * c], s[4 * c + 1], s[4 * c + 2], s[4 * c + 3]];
                let x = col[0] ^ col[1] ^ col[2] ^ col[3];
                for r in 0..4 { s[4 * c + r] = col[r] ^ x ^ xtime(col[r] ^ col[(r + 1) % 4]); }
            }
        }
        for i in 0..16 { s[i] ^= rk[round][i]; }
    }
    s
}

/// AES key wrap (RFC 3394) — how the GTK travels inside message 3 of the handshake.
pub fn aes_key_wrap(kek: &[u8; 16], plain: &[u8]) -> Vec<u8> {
    let n = plain.len() / 8;
    let mut a = [0xa6u8; 8];
    let mut r: Vec<[u8; 8]> = (0..n).map(|i| { let mut b = [0u8; 8]; b.copy_from_slice(&plain[i * 8..i * 8 + 8]); b }).collect();
    for j in 0..6u64 {
        for i in 0..n {
            let mut block = [0u8; 16];
            block[..8].copy_from_slice(&a); block[8..].copy_from_slice(&r[i]);
            let b = aes128_encrypt_block(kek, &block);
            a.copy_from_slice(&b[..8]);
            let t = j * n as u64 + i as u64 + 1;
            for (k, tb) in t.to_be_bytes().iter().enumerate() { a[k] ^= tb; }
            r[i].copy_from_slice(&b[8..]);
        }
    }
    let mut out = Vec::with_capacity(8 + n * 8);
    out.extend_from_slice(&a);
    for b in r { out.extend_from_slice(&b); }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test] fn sha1_known_answers() {
        assert_eq!(sha1(b"abc")[..], hex("a9993e364706816aba3e25717850c26c9cd0d89d")[..]);
        assert_eq!(sha1(b"")[..], hex("da39a3ee5e6b4b0d3255bfef95601890afd80709")[..]);
    }
    #[test] fn hmac_rfc2202() {
        assert_eq!(hmac_sha1(&[0x0b; 20], b"Hi There")[..], hex("b617318655057264e28bc0b6fb378c8ef146be00")[..]);
    }
    #[test] fn pbkdf2_wpa_vector() {
        // IEEE 802.11i-2004 Annex H.4: passphrase "password", SSID "IEEE"
        assert_eq!(pbkdf2_sha1(b"password", b"IEEE", 4096, 32),
                   hex("f42c6fc52df0ebef9ebb4b90b38a5f902e83fe1b135a70e23aed762e9710a12e"));
    }
    #[test] fn aes_all_key_lengths_roundtrip() {
        // FIPS-197 appendix C vectors for 128/192/256, plus decrypt round-trips
        let pt = hex("00112233445566778899aabbccddeeff");
        let mut b = [0u8; 16]; b.copy_from_slice(&pt);
        for (klen, ct) in [(16, "69c4e0d86a7b0430d8cdb78070b4c55a"), (24, "dda97ca4864cdfe06eaf70a0ec0d7191"), (32, "8ea2b7ca516745bfeafc49904b496089")] {
            let key: Vec<u8> = (0..klen as u8).collect();
            let enc = aes_block(&key, &b, false);
            assert_eq!(enc[..], hex(ct)[..], "encrypt with {}-byte key", klen);
            assert_eq!(aes_block(&key, &enc, true)[..], pt[..], "decrypt with {}-byte key", klen);
        }
    }

    #[test] fn aes_and_keywrap_rfc_vectors() {
        // FIPS-197 AES-128 and RFC 3394 section 4.1
        let key = hex("000102030405060708090a0b0c0d0e0f"); let pt = hex("00112233445566778899aabbccddeeff");
        let (mut k, mut b) = ([0u8; 16], [0u8; 16]); k.copy_from_slice(&key); b.copy_from_slice(&pt);
        assert_eq!(aes128_encrypt_block(&k, &b)[..], hex("69c4e0d86a7b0430d8cdb78070b4c55a")[..]);
        assert_eq!(aes_key_wrap(&k, &pt), hex("1fa68b0a8112b447aef34bd8fb5a7b829d3e862371d2cfe5"));
    }
    fn hex(s: &str) -> Vec<u8> { (0..s.len()).step_by(2).map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap()).collect() }
}
