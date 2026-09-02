use std::process::Command;

use wasm_jit_spike::{emit, emit_with_options, CompileError, EmitOptions, REGISTER_COUNT};
use xtensa_lx7::{decode, step, Cpu, FlatRam, Op};

const SRAM_BASE: u32 = 0x3fc8_0000;

#[test]
fn l32r_emits_when_its_interval_cost_is_resolved() {
    let block = literal_block();
    let expected_literal = u32::from_le_bytes(block[..4].try_into().expect("literal width"));
    let expected = interpret(&block);

    assert!(matches!(
        emit(SRAM_BASE, &block, [0; REGISTER_COUNT]),
        Err(CompileError::IntervalCost { op: Op::L32r, .. })
    ));

    for literal_load_cycles in [1, 2] {
        let module = emit_with_options(
            SRAM_BASE,
            &block,
            [0; REGISTER_COUNT],
            EmitOptions {
                literal_load_cycles: Some(literal_load_cycles),
            },
        )
        .expect("receipt interval endpoint must emit");
        let actual = execute_node(literal_load_cycles, &module.bytes);
        assert_eq!(actual.register, expected_literal);
        assert_eq!(actual.register, expected.0);
        assert_eq!(actual.pc, expected.1);
        assert_eq!(actual.cycles, 2 + literal_load_cycles + 3);
    }
}

#[test]
fn unsupported_instruction_ends_compilation_for_interpreter_fallback() {
    let call0 = [0x05, 0x00, 0x00];
    assert!(matches!(
        emit(SRAM_BASE, &call0, [0; REGISTER_COUNT]),
        Err(CompileError::UnsupportedInstruction {
            pc: SRAM_BASE,
            op: Op::Call0
        })
    ));
}

fn literal_block() -> Vec<u8> {
    let mut block = Vec::new();
    block.extend_from_slice(&encode_movi(2, 0x123));
    block.extend_from_slice(&encode_movi(3, -17));
    block.extend_from_slice(&encode_l32r(5, SRAM_BASE + 6, SRAM_BASE));
    block.extend_from_slice(&encode_j(SRAM_BASE + 9, SRAM_BASE + 12));
    assert_eq!(
        decode(SRAM_BASE + 6, [block[6], block[7], block[8], block[9]]).op,
        Op::L32r
    );
    block
}

fn interpret(block: &[u8]) -> (u32, u32) {
    let mut ram = FlatRam::new(SRAM_BASE, 4096);
    ram.mem[..block.len()].copy_from_slice(block);
    let mut cpu = Cpu {
        pc: SRAM_BASE,
        ..Cpu::default()
    };
    for _ in 0..4 {
        step(&mut cpu, &mut ram).expect("literal block executes");
    }
    (cpu.get_ar(5), cpu.pc)
}

struct ResultState {
    register: u32,
    pc: u32,
    cycles: u64,
}

fn execute_node(case: u64, module: &[u8]) -> ResultState {
    const SCRIPT: &str = r#"
const fs = require('fs');
WebAssembly.instantiate(fs.readFileSync(process.argv[1])).then(({instance}) => {
  instance.exports.run();
  const view = new DataView(instance.exports.memory.buffer);
  process.stdout.write([
    view.getUint32(5 * 4, true),
    view.getUint32(64, true),
    view.getBigUint64(72, true).toString()
  ].join(','));
}).catch(error => { console.error(error); process.exit(1); });
"#;
    let path = std::path::Path::new("/tmp").join(format!(
        "esp32sim-wasm-jit-literal-{}-{case}.wasm",
        std::process::id()
    ));
    std::fs::write(&path, module).expect("write emitted wasm module");
    let output = Command::new("node")
        .args(["-e", SCRIPT])
        .arg(&path)
        .output()
        .expect("execute emitted wasm under Node");
    std::fs::remove_file(path).expect("remove emitted wasm module");
    assert!(
        output.status.success(),
        "Node rejected emitted wasm: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let fields: Vec<&str> = std::str::from_utf8(&output.stdout)
        .expect("Node output is UTF-8")
        .split(',')
        .collect();
    ResultState {
        register: fields[0].parse().expect("u32 register"),
        pc: fields[1].parse().expect("u32 pc"),
        cycles: fields[2].parse().expect("u64 cycles"),
    }
}

fn encode_movi(t: u8, immediate: i32) -> [u8; 3] {
    let immediate = immediate as u32 & 0xfff;
    [
        (t << 4) | 2,
        0xa0 | ((immediate >> 8) as u8 & 0xf),
        immediate as u8,
    ]
}

fn encode_l32r(t: u8, pc: u32, address: u32) -> [u8; 3] {
    let aligned = pc.wrapping_add(3) & !3;
    let word_offset = address.wrapping_sub(aligned) >> 2;
    [(t << 4) | 1, word_offset as u8, (word_offset >> 8) as u8]
}

fn encode_j(pc: u32, target: u32) -> [u8; 3] {
    let offset = target.wrapping_sub(pc + 4) as i32;
    let word = ((offset as u32 & 0x3ffff) << 6) | 6;
    [word as u8, (word >> 8) as u8, (word >> 16) as u8]
}
