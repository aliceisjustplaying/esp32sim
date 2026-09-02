use std::process::Command;

use wasm_jit_spike::{emit, price_table_cost, REGISTER_COUNT};
use xtensa_lx7::{decode, step, Cpu, FlatRam};

const SRAM_BASE: u32 = 0x3fc8_0000;

#[derive(Debug)]
struct RuntimeState {
    registers: [u32; REGISTER_COUNT],
    pc: u32,
    cycles: u64,
}

#[test]
fn random_straight_line_blocks_match_interpreter_state_and_cycle_sum() {
    let node = Command::new("node")
        .arg("--version")
        .output()
        .expect("the exit test requires the available Node WebAssembly runtime");
    assert!(
        node.status.success(),
        "Node WebAssembly runtime unavailable"
    );

    let mut rng = Rng::new(0x4c58_3757_4153_4d21);
    let mut module_bytes = 0usize;
    let mut guest_instructions = 0usize;
    for case in 0..100 {
        let instruction_count = 8 + rng.range(25) as usize;
        let block = random_block(&mut rng, instruction_count);
        let initial_registers = std::array::from_fn(|_| rng.next_u32());

        let expected = interpret(&block, initial_registers, instruction_count);
        let expected_cycles = price_sum(&block);
        let module = emit(SRAM_BASE, &block, initial_registers).expect("supported block must emit");
        let actual = execute_node(case, &module.bytes);

        assert_eq!(
            actual.registers, expected.registers,
            "registers, case {case}"
        );
        assert_eq!(actual.pc, expected.pc, "pc, case {case}");
        assert_eq!(actual.cycles, expected_cycles, "cycle ledger, case {case}");
        assert_eq!(
            module.cycle_cost, expected_cycles,
            "emitter price, case {case}"
        );
        assert_eq!(module.instruction_count, instruction_count);
        module_bytes += module.bytes.len();
        guest_instructions += instruction_count;
    }

    let bytes_per_guest = module_bytes as f64 / guest_instructions as f64;
    println!(
        "wasm runtime=node cases=100 module_bytes={module_bytes} guest_instructions={guest_instructions} bytes_per_guest_instruction={bytes_per_guest:.3}"
    );
}

fn random_block(rng: &mut Rng, count: usize) -> Vec<u8> {
    let mut block = Vec::with_capacity(count * 3);
    for _ in 0..count {
        let t = rng.range(REGISTER_COUNT as u32) as u8;
        let s = rng.range(REGISTER_COUNT as u32) as u8;
        let r = rng.range(REGISTER_COUNT as u32) as u8;
        let bytes = match rng.range(7) {
            0 => encode_movi(t, rng.range(4096) as i32 - 2048),
            1 => encode_addi(t, s, rng.next_u32() as u8 as i8),
            2 => encode_rrr(r, s, t, 8),
            3 => encode_rrr(r, s, t, 12),
            4 => encode_rrr(r, s, t, 1),
            5 => encode_rrr(r, s, t, 2),
            _ => encode_rrr(r, s, t, 3),
        };
        block.extend_from_slice(&bytes);
    }
    block
}

fn encode_rrr(r: u8, s: u8, t: u8, op2: u8) -> [u8; 3] {
    [t << 4, (r << 4) | s, op2 << 4]
}

fn encode_movi(t: u8, immediate: i32) -> [u8; 3] {
    let immediate = immediate as u32 & 0xfff;
    [
        (t << 4) | 2,
        0xa0 | ((immediate >> 8) as u8 & 0xf),
        immediate as u8,
    ]
}

fn encode_addi(t: u8, s: u8, immediate: i8) -> [u8; 3] {
    [(t << 4) | 2, 0xc0 | s, immediate as u8]
}

fn interpret(
    block: &[u8],
    initial_registers: [u32; REGISTER_COUNT],
    instruction_count: usize,
) -> RuntimeState {
    let mut ram = FlatRam::new(SRAM_BASE, 4096);
    ram.mem[..block.len()].copy_from_slice(block);
    let mut cpu = Cpu {
        pc: SRAM_BASE,
        ..Cpu::default()
    };
    cpu.ar[..REGISTER_COUNT].copy_from_slice(&initial_registers);
    for _ in 0..instruction_count {
        step(&mut cpu, &mut ram).expect("generated instruction must execute in the interpreter");
    }
    RuntimeState {
        registers: cpu.ar[..REGISTER_COUNT]
            .try_into()
            .expect("register slice has the architectural register width"),
        pc: cpu.pc,
        cycles: 0,
    }
}

fn price_sum(block: &[u8]) -> u64 {
    let mut offset = 0usize;
    let mut sum = 0u64;
    while offset < block.len() {
        let mut bytes = [0u8; 4];
        let available = (block.len() - offset).min(bytes.len());
        bytes[..available].copy_from_slice(&block[offset..offset + available]);
        let instruction = decode(SRAM_BASE + offset as u32, bytes);
        sum += price_table_cost(&instruction).expect("random generator uses priced instructions");
        offset += instruction.len as usize;
    }
    sum
}

fn execute_node(case: usize, module: &[u8]) -> RuntimeState {
    const SCRIPT: &str = r#"
const fs = require('fs');
WebAssembly.instantiate(fs.readFileSync(process.argv[1])).then(({instance}) => {
  instance.exports.run();
  const view = new DataView(instance.exports.memory.buffer);
  const values = [];
  for (let i = 0; i < 16; i++) values.push(view.getUint32(i * 4, true));
  values.push(view.getUint32(64, true));
  values.push(view.getBigUint64(72, true).toString());
  process.stdout.write(values.join(','));
}).catch(error => { console.error(error); process.exit(1); });
"#;
    let path = std::env::temp_dir().join(format!(
        "esp32sim-wasm-jit-spike-{}-{case}.wasm",
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
    assert_eq!(fields.len(), REGISTER_COUNT + 2, "Node state width");
    RuntimeState {
        registers: std::array::from_fn(|index| {
            fields[index].parse().expect("Node register is a u32")
        }),
        pc: fields[REGISTER_COUNT].parse().expect("Node pc is a u32"),
        cycles: fields[REGISTER_COUNT + 1]
            .parse()
            .expect("Node cycle ledger is a u64"),
    }
}

struct Rng(u64);

impl Rng {
    fn new(seed: u64) -> Self {
        Self(seed)
    }

    fn next_u32(&mut self) -> u32 {
        let mut value = self.0;
        value ^= value << 13;
        value ^= value >> 7;
        value ^= value << 17;
        self.0 = value;
        value as u32
    }

    fn range(&mut self, upper: u32) -> u32 {
        self.next_u32() % upper
    }
}
