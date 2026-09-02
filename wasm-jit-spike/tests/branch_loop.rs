use std::process::Command;

use wasm_jit_spike::{emit, REGISTER_COUNT};
use xtensa_lx7::{decode, step, Cpu, FlatRam, Op};

const SRAM_BASE: u32 = 0x3fc8_0000;

#[derive(Debug, PartialEq, Eq)]
struct RuntimeState {
    registers: [u32; REGISTER_COUNT],
    pc: u32,
    cycles: u64,
    lbeg: u32,
    lend: u32,
    lcount: u32,
}

#[test]
fn one_hundred_random_branch_loop_blocks_match_interpreter_and_measured_cycles() {
    let node = Command::new("node")
        .arg("--version")
        .output()
        .expect("the exit test requires the available Node WebAssembly runtime");
    assert!(node.status.success(), "Node WebAssembly runtime unavailable");

    let mut rng = Rng::new(0x4252_414e_4348_5a4f);
    for case in 0..100 {
        let loop_op = match case % 3 {
            0 => Op::Loop,
            1 => Op::Loopnez,
            _ => Op::Loopgtz,
        };
        let count = if loop_op == Op::Loop {
            1 + rng.range(4)
        } else {
            rng.range(5)
        };
        let branch_on_zero = rng.range(2) == 0;
        let branch_value = rng.range(3).wrapping_sub(1);
        let block = branch_loop_block(loop_op, branch_on_zero);
        let mut registers = std::array::from_fn(|_| rng.next_u32());
        registers[0] = count;
        registers[1] = branch_value;

        let expected = interpret(&block, registers);
        let module = emit(SRAM_BASE, &block, registers).expect("branch and loop block must emit");
        let actual = execute_node(case, &module.bytes);
        assert_eq!(actual, expected, "case {case}");
    }
}

fn branch_loop_block(loop_op: Op, branch_on_zero: bool) -> Vec<u8> {
    let mut block = Vec::new();
    block.extend_from_slice(&encode_loop(loop_op, 0, SRAM_BASE + 15));
    block.extend_from_slice(&encode_addi(2, 2, 1));
    block.extend_from_slice(&encode_branch_zero(
        branch_on_zero,
        1,
        SRAM_BASE + 6,
        SRAM_BASE + 12,
    ));
    block.extend_from_slice(&encode_addi(3, 3, 7));
    block.extend_from_slice(&encode_addi(4, 4, -3));
    block.extend_from_slice(&encode_j(SRAM_BASE + 15, SRAM_BASE + 18));
    block
}

fn interpret(block: &[u8], registers: [u32; REGISTER_COUNT]) -> RuntimeState {
    let mut ram = FlatRam::new(SRAM_BASE, 4096);
    ram.mem[..block.len()].copy_from_slice(block);
    let mut cpu = Cpu {
        pc: SRAM_BASE,
        ..Cpu::default()
    };
    cpu.ar[..REGISTER_COUNT].copy_from_slice(&registers);
    let mut cycles = 0u64;
    for _ in 0..128 {
        if cpu.pc < SRAM_BASE || cpu.pc >= SRAM_BASE + block.len() as u32 {
            return RuntimeState {
                registers: cpu.ar[..REGISTER_COUNT].try_into().expect("register width"),
                pc: cpu.pc,
                cycles,
                lbeg: cpu.lbeg,
                lend: cpu.lend,
                lcount: cpu.lcount,
            };
        }
        let offset = (cpu.pc - SRAM_BASE) as usize;
        let mut bytes = [0u8; 4];
        bytes.copy_from_slice(&ram.mem[offset..offset + 4]);
        let instruction = decode(cpu.pc, bytes);
        cycles += measured_cost(&cpu, instruction.op, instruction.s);
        step(&mut cpu, &mut ram).expect("generated instruction executes");
    }
    panic!("generated block did not terminate");
}

fn measured_cost(cpu: &Cpu, op: Op, s: u8) -> u64 {
    match op {
        Op::Beqz | Op::BeqzN => {
            if cpu.get_ar(s) == 0 { 3 } else { 1 }
        }
        Op::Bnez | Op::BnezN => {
            if cpu.get_ar(s) != 0 { 3 } else { 1 }
        }
        Op::J => 3,
        Op::Loop | Op::Loopnez | Op::Loopgtz => 5,
        _ => 1,
    }
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
  values.push(view.getUint32(80, true));
  values.push(view.getUint32(84, true));
  values.push(view.getUint32(88, true));
  process.stdout.write(values.join(','));
}).catch(error => { console.error(error); process.exit(1); });
"#;
    let path = std::env::temp_dir().join(format!(
        "esp32sim-wasm-jit-branch-loop-{}-{case}.wasm",
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
    assert_eq!(fields.len(), REGISTER_COUNT + 5, "Node state width");
    RuntimeState {
        registers: std::array::from_fn(|index| fields[index].parse().expect("u32 register")),
        pc: fields[REGISTER_COUNT].parse().expect("u32 pc"),
        cycles: fields[REGISTER_COUNT + 1].parse().expect("u64 cycles"),
        lbeg: fields[REGISTER_COUNT + 2].parse().expect("u32 lbeg"),
        lend: fields[REGISTER_COUNT + 3].parse().expect("u32 lend"),
        lcount: fields[REGISTER_COUNT + 4].parse().expect("u32 lcount"),
    }
}

fn encode_loop(op: Op, s: u8, target: u32) -> [u8; 3] {
    let r = match op {
        Op::Loop => 8,
        Op::Loopnez => 9,
        Op::Loopgtz => 10,
        _ => panic!("not a loop opcode"),
    };
    let offset = target.wrapping_sub(SRAM_BASE + 4);
    [0x76, (r << 4) | s, offset as u8]
}

fn encode_branch_zero(taken_on_zero: bool, s: u8, pc: u32, target: u32) -> [u8; 3] {
    let offset = target.wrapping_sub(pc + 4) as i32;
    let word = ((offset as u32 & 0xfff) << 12)
        | ((if taken_on_zero { 0u32 } else { 1 }) << 6)
        | (1 << 4)
        | 6;
    [word as u8, ((word >> 8) as u8 & 0xf0) | s, (word >> 16) as u8]
}

fn encode_j(pc: u32, target: u32) -> [u8; 3] {
    let offset = target.wrapping_sub(pc + 4) as i32;
    let word = ((offset as u32 & 0x3ffff) << 6) | 6;
    [word as u8, (word >> 8) as u8, (word >> 16) as u8]
}

fn encode_addi(t: u8, s: u8, immediate: i8) -> [u8; 3] {
    [(t << 4) | 2, 0xc0 | s, immediate as u8]
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
