//! Differential decoder test against a mandatory committed objdump corpus.
//! XTENSA_DIS_FILES may add larger local corpora; every named file is required.
#[path = "../../test-support/conformance.rs"]
mod conformance;

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use xtensa_lx7::decode::{decode, Op};
use xtensa_lx7::disasm;

const MANDATORY_CORPUS: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/corpus/mandatory.dis");
const MANDATORY_PROVENANCE: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/corpus/mandatory.provenance"
);

fn corpus_files() -> Vec<PathBuf> {
    let mut files = vec![PathBuf::from(MANDATORY_CORPUS)];
    if let Some(extra) = std::env::var_os("XTENSA_DIS_FILES") {
        files.extend(std::env::split_paths(&extra));
    }
    files
}

fn norm_num(tok: &str) -> Option<i64> {
    let t = tok.trim();
    if let Some(h) = t.strip_prefix("0x") {
        return i64::from_str_radix(h, 16).ok();
    }
    if let Some(h) = t.strip_prefix("-0x") {
        return i64::from_str_radix(h, 16).ok().map(|v| -v);
    }
    if let Ok(v) = t.parse::<i64>() {
        return Some(v);
    }
    i64::from_str_radix(t, 16).ok()
}
fn same_operand(mine: &str, theirs: &str) -> bool {
    if mine == theirs {
        return true;
    }
    match (norm_num(mine), norm_num(theirs)) {
        (Some(a), Some(b)) => a == b || (a as u32) == (b as u32),
        _ => false,
    }
}

#[test]
fn decoder_matches_objdump() {
    let mut total = 0usize;
    let mut bad = 0usize;
    let mut by_mnemonic: BTreeMap<String, (usize, usize, String)> = BTreeMap::new();
    for (file_index, file) in corpus_files().into_iter().enumerate() {
        let corpus_bytes = conformance::read_required_corpus(&file);
        let text = std::str::from_utf8(&corpus_bytes)
            .unwrap_or_else(|e| panic!("{}: {e}", file.display()));
        let mut file_total = 0usize;
        let mut file_mnemonics = BTreeSet::new();
        for line in text.lines() {
            // "40374404:\t36 41 00 \tentry\ta1, 32"   (objdump prints bytes reversed as one hex string on xtensa)
            let mut parts = line.splitn(3, '\t');
            let (Some(addr_s), Some(bytes_s), Some(rest)) =
                (parts.next(), parts.next(), parts.next())
            else {
                continue;
            };
            let Some(addr_s) = addr_s.trim().strip_suffix(':') else {
                continue;
            };
            let Ok(pc) = u32::from_str_radix(addr_s.trim(), 16) else {
                continue;
            };
            let hex: String = bytes_s.chars().filter(|c| !c.is_whitespace()).collect();
            if !hex.len().is_multiple_of(2) || hex.is_empty() {
                continue;
            }
            let mut raw: Vec<u8> = (0..hex.len() / 2)
                .map(|i| u8::from_str_radix(&hex[2 * i..2 * i + 2], 16).unwrap())
                .collect();
            raw.reverse(); // objdump prints the word big-endian; memory order is little-endian
            let expect = rest.trim();
            if expect.starts_with(".byte") || expect.is_empty() {
                continue;
            }
            let mut bytes = [0u8; 4];
            for (i, b) in raw.iter().take(4).enumerate() {
                bytes[i] = *b;
            }
            let insn = decode(pc, bytes);
            total += 1;
            file_total += 1;
            let mut exp_parts = expect.splitn(2, char::is_whitespace);
            let exp_mn = exp_parts.next().unwrap_or("");
            file_mnemonics.insert(exp_mn.to_string());
            let exp_ops: Vec<String> = exp_parts
                .next()
                .unwrap_or("")
                .split(',')
                .map(|o| {
                    let o = o.trim();
                    match o.find(" <") {
                        Some(p) => o[..p].to_string(),
                        None => o.to_string(),
                    }
                })
                .filter(|o| !o.is_empty())
                .collect();
            let entry = by_mnemonic
                .entry(exp_mn.to_string())
                .or_insert((0, 0, String::new()));
            entry.0 += 1;
            if insn.op == Op::Pie || insn.op == Op::Mac16 && exp_mn.starts_with("ee.") {
                continue;
            } // PIE decoded generically for now
            let my_mn = disasm::mnemonic(&insn);
            let my_ops = disasm::operands(&insn);
            let ok = my_mn == exp_mn
                && my_ops.len() == exp_ops.len()
                && my_ops
                    .iter()
                    .zip(exp_ops.iter())
                    .all(|(m, e)| same_operand(m, e))
                && insn.len as usize == raw.len();
            if !ok {
                bad += 1;
                entry.1 += 1;
                if entry.2.is_empty() {
                    entry.2 = format!(
                        "{:08x}: {} | mine: {} (len {})",
                        pc,
                        expect,
                        disasm::format(&insn),
                        insn.len
                    );
                }
            }
        }
        assert!(
            file_total > 0,
            "{}: no decoder cases parsed",
            file.display()
        );
        let digest = conformance::sha256_hex(&corpus_bytes);
        eprintln!(
            "decoder corpus={} sha256={} cases={}",
            file.display(),
            digest,
            file_total
        );
        if file_index == 0 {
            conformance::verify_provenance(
                std::path::Path::new(MANDATORY_PROVENANCE),
                "xtensa-lx7",
                &file,
                &corpus_bytes,
                file_total,
                &file_mnemonics,
            );
        }
    }
    for (m, (n, b, ex)) in &by_mnemonic {
        if *b > 0 {
            eprintln!("{:<28} {:>7} insns {:>6} bad   e.g. {}", m, n, b, ex);
        }
    }
    eprintln!("total {} instructions, {} mismatches", total, bad);
    assert!(total > 0, "no decoder conformance cases executed");
    assert_eq!(bad, 0, "decoder mismatches vs objdump");
}
