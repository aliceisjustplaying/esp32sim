//! Minimal ELF32 (little-endian) loader: PT_LOAD segments + symbol table.
use std::collections::BTreeMap;

pub struct Segment {
    pub vaddr: u32,
    pub paddr: u32,
    pub data: Vec<u8>,
    pub memsz: u32,
    pub flags: u32,
}

pub struct Section {
    pub name: String,
    pub addr: u32,
    pub data: Vec<u8>,
    pub is_bss: bool,
}

pub struct Elf {
    pub entry: u32,
    pub segments: Vec<Segment>,
    /// allocatable sections (PROGBITS with data, NOBITS as bss) — the ROM ELF keeps
    /// its RAM initialisers here without matching program headers
    pub sections: Vec<Section>,
    /// address -> symbol name (functions and objects)
    pub symbols: BTreeMap<u32, String>,
    /// name -> address (all symbols incl. NOTYPE linker symbols)
    pub by_name: std::collections::HashMap<String, u32>,
}

fn u16le(d: &[u8], o: usize) -> u16 {
    u16::from_le_bytes([d[o], d[o + 1]])
}
fn u32le(d: &[u8], o: usize) -> u32 {
    u32::from_le_bytes([d[o], d[o + 1], d[o + 2], d[o + 3]])
}

pub fn parse(d: &[u8]) -> Result<Elf, String> {
    if d.len() < 52 || &d[0..4] != b"\x7fELF" {
        return Err("not an ELF file".into());
    }
    if d[4] != 1 || d[5] != 1 {
        return Err("need ELF32 little-endian".into());
    }
    let entry = u32le(d, 24);
    let phoff = u32le(d, 28) as usize;
    let shoff = u32le(d, 32) as usize;
    let phentsize = u16le(d, 42) as usize;
    let phnum = u16le(d, 44) as usize;
    let shentsize = u16le(d, 46) as usize;
    let shnum = u16le(d, 48) as usize;
    let mut segments = Vec::new();
    for i in 0..phnum {
        let p = phoff + i * phentsize;
        if p + 32 > d.len() {
            break;
        }
        let ptype = u32le(d, p);
        if ptype != 1 {
            continue;
        }
        let offset = u32le(d, p + 4) as usize;
        let vaddr = u32le(d, p + 8);
        let paddr = u32le(d, p + 12);
        let filesz = u32le(d, p + 16) as usize;
        let memsz = u32le(d, p + 20);
        let flags = u32le(d, p + 24);
        if memsz == 0 {
            continue;
        }
        let data = if filesz > 0 && offset + filesz <= d.len() {
            d[offset..offset + filesz].to_vec()
        } else {
            Vec::new()
        };
        segments.push(Segment {
            vaddr,
            paddr,
            data,
            memsz,
            flags,
        });
    }
    // symbols
    let mut symbols = BTreeMap::new();
    let mut by_name = std::collections::HashMap::new();
    let mut sections = Vec::new();
    let mut alloc_sections = Vec::new();
    let shstrndx = u16le(d, 50) as usize;
    let shstr_off = if shstrndx < shnum {
        u32le(d, shoff + shstrndx * shentsize + 16) as usize
    } else {
        0
    };
    for i in 0..shnum {
        let s = shoff + i * shentsize;
        if s + 40 > d.len() {
            break;
        }
        let (name_off, stype, flags, addr, off, size) = (
            u32le(d, s) as usize,
            u32le(d, s + 4),
            u32le(d, s + 8),
            u32le(d, s + 12),
            u32le(d, s + 16) as usize,
            u32le(d, s + 20) as usize,
        );
        sections.push((
            stype,
            off,
            size,
            u32le(d, s + 24) as usize,
            u32le(d, s + 36) as usize,
        ));
        if size > 0 && addr != 0 && (stype == 1 || (stype == 8 && flags & 2 != 0)) {
            // ROM ELFs mark RAM initialisers W-only (no SHF_ALLOC)
            let nstart = shstr_off + name_off;
            let nend = d[nstart..]
                .iter()
                .position(|&c| c == 0)
                .map(|p| nstart + p)
                .unwrap_or(nstart);
            let name = String::from_utf8_lossy(&d[nstart..nend]).into_owned();
            let data = if stype == 1 && off + size <= d.len() {
                d[off..off + size].to_vec()
            } else {
                Vec::new()
            };
            alloc_sections.push(Section {
                name,
                addr,
                data,
                is_bss: stype == 8,
            });
        }
    }
    for &(stype, off, size, link, entsize) in &sections {
        if stype != 2 || entsize == 0 {
            continue;
        } // SHT_SYMTAB
        let Some(&(_, stroff, strsize, _, _)) = sections.get(link) else {
            continue;
        };
        for j in 0..size / entsize {
            let e = off + j * entsize;
            if e + 16 > d.len() {
                break;
            }
            let name_off = u32le(d, e) as usize;
            let value = u32le(d, e + 4);
            let info = d[e + 12];
            let typ = info & 0xf;
            if name_off >= strsize {
                continue;
            }
            let start = stroff + name_off;
            let end = d[start..]
                .iter()
                .position(|&c| c == 0)
                .map(|p| start + p)
                .unwrap_or(start);
            let name = String::from_utf8_lossy(&d[start..end]).into_owned();
            if name.is_empty() {
                continue;
            }
            by_name.entry(name.clone()).or_insert(value);
            if typ == 1 || typ == 2 {
                symbols.entry(value).or_insert(name);
            } // OBJECT / FUNC
        }
    }
    Ok(Elf {
        entry,
        segments,
        sections: alloc_sections,
        symbols,
        by_name,
    })
}
