//! Differential decoder test against a mandatory committed objdump corpus.
//! RISCV_DIS_FILES may add larger local corpora; every named file is required.
#[path = "../../test-support/conformance.rs"]
mod conformance;

use riscv_rv32::decode::decode;
use riscv_rv32::disasm;
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

const MANDATORY_CORPUS: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/corpus/mandatory.dis");
const MANDATORY_PROVENANCE: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/corpus/mandatory.provenance"
);

fn corpus_files() -> Vec<PathBuf> {
    let mut files = vec![PathBuf::from(MANDATORY_CORPUS)];
    if let Some(extra) = std::env::var_os("RISCV_DIS_FILES") {
        files.extend(std::env::split_paths(&extra));
    }
    files
}

fn norm_num(t: &str) -> Option<i64> {
    let t = t.trim();
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
    let (mut total, mut bad) = (0usize, 0usize);
    let mut by_mnemonic: BTreeMap<String, (usize, usize, String)> = BTreeMap::new();
    for (file_index, file) in corpus_files().into_iter().enumerate() {
        let corpus_bytes = conformance::read_required_corpus(&file);
        let text = std::str::from_utf8(&corpus_bytes)
            .unwrap_or_else(|e| panic!("{}: {e}", file.display()));
        let mut file_total = 0usize;
        let mut file_mnemonics = BTreeSet::new();
        for line in text.lines() {
            // "40000000:\t0000006f          \tj\t40000000 <_start>"
            let l = line.trim_start();
            let Some((addr_s, rest)) = l.split_once(":\t") else {
                continue;
            };
            let Ok(addr) = u32::from_str_radix(addr_s.trim(), 16) else {
                continue;
            };
            let Some((hex, text)) = rest.split_once('\t') else {
                continue;
            };
            let hex = hex.trim();
            if hex.is_empty() || !hex.chars().all(|c| c.is_ascii_hexdigit()) {
                continue;
            }
            let Ok(word) = u32::from_str_radix(hex, 16) else {
                continue;
            };
            let len = hex.len() / 2;
            if len != 2 && len != 4 {
                continue;
            }
            // objdump appends " <sym+0x..>" and "# 0x.. <sym>" comments, not part of the operands
            let mut t = text.trim();
            if let Some(p) = t.find(" <") {
                t = &t[..p];
            }
            if let Some(p) = t.find(" #") {
                t = &t[..p];
            }
            if let Some(p) = t.find("  ") {
                t = &t[..p];
            }
            let theirs = t.trim();
            if theirs.is_empty() || theirs.starts_with('.') {
                continue;
            }

            let mut bytes = [0u8; 4];
            for (i, byte) in bytes.iter_mut().take(len).enumerate() {
                *byte = ((word >> (8 * i)) & 0xff) as u8;
            }
            let insn = decode(addr, bytes);
            let mine = disasm::format(&insn);
            total += 1;
            file_total += 1;

            let (tm, to) = theirs
                .split_once(char::is_whitespace)
                .unwrap_or((theirs, ""));
            file_mnemonics.insert(tm.to_string());
            let (mm, mo) = mine.split_once('\t').unwrap_or((mine.as_str(), ""));
            let ok = tm == mm
                && insn.len as usize == len
                && to
                    .split(',')
                    .zip(mo.split(','))
                    .all(|(a, b)| same_operand(b.trim(), a.trim()))
                && to.split(',').count() == mo.split(',').count();
            let e = by_mnemonic
                .entry(tm.to_string())
                .or_insert((0, 0, String::new()));
            e.0 += 1;
            if !ok {
                bad += 1;
                e.1 += 1;
                if e.2.is_empty() {
                    e.2 = format!(
                        "{:08x} {:>8}  objdump={:?}  ours={:?}",
                        addr, hex, theirs, mine
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
                "riscv-rv32imc",
                &file,
                &corpus_bytes,
                file_total,
                &file_mnemonics,
            );
        }
    }
    assert!(total > 0, "no decoder conformance cases executed");
    if bad > 0 {
        let mut worst: Vec<_> = by_mnemonic.iter().filter(|(_, v)| v.1 > 0).collect();
        worst.sort_by_key(|(_, v)| std::cmp::Reverse(v.1));
        let mut msg = format!("{}/{} instructions mismatched\n", bad, total);
        for (m, (n, b, ex)) in worst.iter().take(15) {
            msg.push_str(&format!("  {:<12} {:>6}/{:<6}  {}\n", m, b, n, ex));
        }
        panic!("{}", msg);
    }
    eprintln!("decoder: {} instructions, 0 mismatches", total);
}
