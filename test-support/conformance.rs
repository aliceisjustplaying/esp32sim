use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

const SHA256_K: [u32; 64] = [
    0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5,
    0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
    0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
    0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
    0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
    0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
    0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
    0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
];

pub fn read_required_corpus(path: &Path) -> Vec<u8> {
    eprintln!("required decoder corpus={}", path.display());
    std::fs::read(path).expect("required decoder corpus unavailable")
}

pub fn sha256_hex(input: &[u8]) -> String {
    let bit_len = (input.len() as u64) * 8;
    let mut padded = input.to_vec();
    padded.push(0x80);
    while padded.len() % 64 != 56 {
        padded.push(0);
    }
    padded.extend_from_slice(&bit_len.to_be_bytes());

    let mut h = [
        0x6a09e667u32,
        0xbb67ae85,
        0x3c6ef372,
        0xa54ff53a,
        0x510e527f,
        0x9b05688c,
        0x1f83d9ab,
        0x5be0cd19,
    ];
    let (chunks, remainder) = padded.as_chunks::<64>();
    assert!(
        remainder.is_empty(),
        "SHA-256 padding must be block aligned"
    );
    for chunk in chunks {
        let mut w = [0u32; 64];
        for (i, word) in w.iter_mut().take(16).enumerate() {
            let start = i * 4;
            *word = u32::from_be_bytes([
                chunk[start],
                chunk[start + 1],
                chunk[start + 2],
                chunk[start + 3],
            ]);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16]
                .wrapping_add(s0)
                .wrapping_add(w[i - 7])
                .wrapping_add(s1);
        }

        let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut hh] = h;
        for i in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ ((!e) & g);
            let t1 = hh
                .wrapping_add(s1)
                .wrapping_add(ch)
                .wrapping_add(SHA256_K[i])
                .wrapping_add(w[i]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let t2 = s0.wrapping_add(maj);
            hh = g;
            g = f;
            f = e;
            e = d.wrapping_add(t1);
            d = c;
            c = b;
            b = a;
            a = t1.wrapping_add(t2);
        }
        for (slot, value) in h.iter_mut().zip([a, b, c, d, e, f, g, hh]) {
            *slot = slot.wrapping_add(value);
        }
    }
    h.iter().map(|word| format!("{word:08x}")).collect()
}

fn parse_sidecar(path: &Path) -> BTreeMap<String, String> {
    let text = std::fs::read_to_string(path).expect("provenance sidecar must be readable");
    let mut fields = BTreeMap::new();
    for (line_number, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let (key, value) = line
            .split_once('=')
            .expect("provenance fields must use key=value syntax");
        assert!(
            !key.is_empty() && !value.is_empty(),
            "{}:{}: empty provenance field",
            path.display(),
            line_number + 1
        );
        assert!(
            fields.insert(key.to_string(), value.to_string()).is_none(),
            "{}:{}: duplicate provenance key {key}",
            path.display(),
            line_number + 1
        );
    }
    fields
}

pub fn verify_provenance(
    sidecar: &Path,
    architecture: &str,
    corpus: &Path,
    corpus_bytes: &[u8],
    case_count: usize,
    mnemonics: &BTreeSet<String>,
) {
    let fields = parse_sidecar(sidecar);
    for required in [
        "schema",
        "architecture",
        "corpus",
        "sha256",
        "case_count",
        "semantic_mnemonics",
        "source",
        "source_sha256",
        "oracle",
        "toolchain",
        "generator",
        "flags",
        "generated_at",
        "host",
        "seed",
        "virtual_duration",
        "backend_capabilities",
    ] {
        assert!(
            fields.contains_key(required),
            "{}: missing required provenance field {required}",
            sidecar.display()
        );
    }
    assert_eq!(
        fields["schema"],
        "1",
        "{}: unsupported provenance schema",
        sidecar.display()
    );
    assert_eq!(
        fields["architecture"],
        architecture,
        "{}: architecture mismatch",
        sidecar.display()
    );
    assert_eq!(
        fields["corpus"],
        corpus
            .file_name()
            .expect("corpus path must name a file")
            .to_string_lossy(),
        "{}: corpus filename mismatch",
        sidecar.display()
    );
    assert_eq!(
        fields["sha256"],
        sha256_hex(corpus_bytes),
        "{}: corpus digest mismatch",
        sidecar.display()
    );
    assert_eq!(
        fields["case_count"]
            .parse::<usize>()
            .expect("numeric case_count"),
        case_count,
        "{}: case count mismatch",
        sidecar.display()
    );

    let expected_mnemonics: BTreeSet<String> = fields["semantic_mnemonics"]
        .split(',')
        .map(str::to_string)
        .collect();
    assert_eq!(
        &expected_mnemonics,
        mnemonics,
        "{}: semantic mnemonic set mismatch",
        sidecar.display()
    );

    let source = sidecar
        .parent()
        .expect("provenance sidecar must have a parent directory")
        .join(&fields["source"]);
    let source_bytes = std::fs::read(&source).expect("corpus source must be readable");
    assert_eq!(
        fields["source_sha256"],
        sha256_hex(&source_bytes),
        "{}: source digest mismatch",
        sidecar.display()
    );
}

#[cfg(test)]
mod tests {
    use super::{read_required_corpus, sha256_hex};
    use std::path::Path;

    #[test]
    #[should_panic(expected = "required decoder corpus unavailable")]
    fn missing_corpus_fails_closed() {
        let missing = Path::new(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/corpus/.missing-corpus-negative-test"
        ));
        let _ = read_required_corpus(missing);
    }

    #[test]
    fn sha256_matches_published_vector() {
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }
}
