use std::process::Command;

use backend_api::MmioTier;
use wasm_jit_spike::{emit_host_memory, HostMemoryClass, POSTED_WRITES_OFFSET, REGISTER_COUNT};

const SRAM_PC: u32 = 0x4038_0000;

#[test]
fn mmio_tiers_and_posted_write_buffer_are_compiled_into_wasm() {
    let mut registers = [0; REGISTER_COUNT];
    registers[2] = 0x600c_0000;
    registers[3] = 0x55aa_aa55;
    let mut writes = Vec::new();
    for _ in 0..9 {
        writes.extend_from_slice(&encode_s32i(3, 2, 0));
    }
    let module = emit_host_memory(
        SRAM_PC,
        &writes,
        registers,
        &[HostMemoryClass::Mmio(MmioTier::Fast); 9],
        0,
    )
    .expect("exact MMIO tier emits");
    let result = execute_node(&module.bytes, 0, 0);
    assert_eq!(result.cycles, 8 + 4);
    assert_eq!(result.posted_writes, 9);
    assert_eq!(result.mmio_calls, 9);
}

#[test]
fn flash_and_psram_accesses_call_the_host_cache_model() {
    let mut registers = [0; REGISTER_COUNT];
    registers[2] = 0x3c00_1000;
    registers[4] = 0x3c80_2000;
    let mut loads = Vec::new();
    loads.extend_from_slice(&encode_l32i(3, 2, 0));
    loads.extend_from_slice(&encode_l32i(5, 4, 0));
    let module = emit_host_memory(
        SRAM_PC,
        &loads,
        registers,
        &[HostMemoryClass::Flash, HostMemoryClass::Psram],
        0,
    )
    .expect("external loads emit");
    let result = execute_node(&module.bytes, 203, 81);
    assert_eq!(result.cycles, 1 + 203 + 1 + 81);
    assert_eq!(result.cache_calls, 2);
}

struct NodeResult {
    cycles: u64,
    posted_writes: u32,
    mmio_calls: u32,
    cache_calls: u32,
}

fn execute_node(module: &[u8], flash_cycles: u32, psram_cycles: u32) -> NodeResult {
    const SCRIPT: &str = r#"
const fs = require('fs');
let mmioCalls = 0, cacheCalls = 0;
const imports = { env: {
  mmio: (tier, store) => { mmioCalls++; return store ? 0n : 0x1234n; },
  cache_access: (source) => {
    cacheCalls++;
    const cycles = source === 5 ? Number(process.argv[3]) : Number(process.argv[4]);
    return (BigInt(cycles) << 32n) | 0x5678n;
  }
}};
WebAssembly.instantiate(fs.readFileSync(process.argv[1]), imports).then(({instance}) => {
  instance.exports.run();
  const v = new DataView(instance.exports.memory.buffer);
  process.stdout.write([
    v.getBigUint64(72, true),
    v.getUint32(Number(process.argv[2]), true),
    mmioCalls,
    cacheCalls
  ].join(','));
}).catch(error => { console.error(error); process.exit(1); });
"#;
    let path = std::path::Path::new("/tmp").join(format!(
        "esp32sim-wasm-jit-memory-{}-{}.wasm",
        std::process::id(),
        flash_cycles + psram_cycles
    ));
    std::fs::write(&path, module).expect("write wasm");
    let output = Command::new("node")
        .args(["-e", SCRIPT])
        .arg(&path)
        .arg(POSTED_WRITES_OFFSET.to_string())
        .arg(flash_cycles.to_string())
        .arg(psram_cycles.to_string())
        .output()
        .expect("run wasm");
    std::fs::remove_file(path).expect("remove wasm");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let fields: Vec<_> = std::str::from_utf8(&output.stdout)
        .expect("utf8")
        .split(',')
        .collect();
    NodeResult {
        cycles: fields[0].parse().expect("cycles"),
        posted_writes: fields[1].parse().expect("posted writes"),
        mmio_calls: fields[2].parse().expect("mmio calls"),
        cache_calls: fields[3].parse().expect("cache calls"),
    }
}

fn encode_l32i(target: u8, source: u8, word_offset: u8) -> [u8; 3] {
    [(target << 4) | 2, (2 << 4) | source, word_offset]
}

fn encode_s32i(value: u8, base: u8, word_offset: u8) -> [u8; 3] {
    [(value << 4) | 2, (6 << 4) | base, word_offset]
}
