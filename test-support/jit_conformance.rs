#[derive(Debug)]
pub struct CorpusCase {
    pub name: String,
    pub instructions: Vec<Vec<u8>>,
}

fn json_string_field(line: &str, field: &str) -> String {
    let marker = format!("\"{field}\": \"");
    let value = line.split_once(&marker).map_or("", |parts| parts.1);
    assert!(!value.is_empty(), "missing JSON field {field}");
    let result = value.split_once('"').map_or("", |parts| parts.0);
    assert!(!result.is_empty(), "unterminated JSON field {field}");
    result.to_string()
}

fn hex_bytes(hex: &str) -> Vec<u8> {
    assert!(!hex.is_empty(), "empty instruction encoding");
    assert!(
        hex.len().is_multiple_of(2),
        "odd instruction encoding {hex}"
    );
    (0..hex.len())
        .step_by(2)
        .map(|offset| {
            let byte = u8::from_str_radix(&hex[offset..offset + 2], 16);
            assert!(byte.is_ok(), "invalid instruction encoding {hex}");
            byte.unwrap_or_default()
        })
        .collect()
}

pub fn parse_corpus(text: &str) -> Vec<CorpusCase> {
    let mut cases = Vec::new();
    for line in text.lines().map(str::trim) {
        if !line.starts_with('{') {
            continue;
        }
        let name = json_string_field(line, "name");
        let marker = "\"instructions\": [";
        let array = line.split_once(marker).map_or("", |parts| parts.1);
        assert!(!array.is_empty(), "{name}: missing instruction array");
        let encoded = array.split_once(']').map_or("", |parts| parts.0);
        assert!(
            !encoded.is_empty(),
            "{name}: unterminated instruction array"
        );
        let instructions: Vec<Vec<u8>> = encoded
            .split(',')
            .map(|item| item.trim().trim_matches('"'))
            .map(hex_bytes)
            .collect();
        assert!(!instructions.is_empty(), "{name}: empty instruction array");
        assert!(
            instructions
                .iter()
                .all(|instruction| matches!(instruction.len(), 2 | 3)),
            "{name}: instruction width outside the Xtensa density encoding"
        );
        cases.push(CorpusCase { name, instructions });
    }
    assert!(!cases.is_empty(), "empty JIT conformance corpus");
    cases
}

pub struct XorShift32(u32);

impl XorShift32 {
    pub fn new(seed: u32) -> Self {
        assert_ne!(seed, 0, "xorshift seed must be nonzero");
        Self(seed)
    }

    pub fn next_u32(&mut self) -> u32 {
        let mut value = self.0;
        value ^= value << 13;
        value ^= value >> 17;
        value ^= value << 5;
        self.0 = value;
        value
    }

    pub fn index(&mut self, len: usize) -> usize {
        assert_ne!(len, 0, "cannot select from an empty instruction pool");
        self.next_u32() as usize % len
    }
}
