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
    assert!(
        node.status.success(),
        "Node WebAssembly runtime unavailable"
    );

    let mut rng = Rng::new(0x4252_414e_4348_5a4f);
    let mut module_bytes = 0usize;
    let mut guest_instructions = 0usize;
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
        let branch_kind = case % 26;
        let block = branch_loop_block(loop_op, branch_kind);
        let mut branch_bytes = [0u8; 4];
        branch_bytes.copy_from_slice(&block[6..10]);
        assert_eq!(
            decode(SRAM_BASE + 6, branch_bytes).op,
            expected_branch_op(branch_kind),
            "branch encoding, case {case}"
        );
        let mut registers = std::array::from_fn(|_| rng.next_u32());
        registers[0] = count;
        set_branch_outcome(&mut registers, branch_kind, (case / 26) % 2 == 0);

        let expected = interpret(&block, registers);
        let module = emit(SRAM_BASE, &block, registers).expect("branch and loop block must emit");
        let actual = execute_node(case, &module.bytes);
        assert_eq!(actual, expected, "case {case}");
        module_bytes += module.bytes.len();
        guest_instructions += module.instruction_count;
    }
    println!(
        "wasm runtime=node cases=100 module_bytes={module_bytes} guest_instructions={guest_instructions} bytes_per_guest_instruction={:.3}",
        module_bytes as f64 / guest_instructions as f64
    );
}

fn set_branch_outcome(registers: &mut [u32; REGISTER_COUNT], kind: usize, should_take: bool) {
    let (s, t) = match kind {
        0 | 24 => (u32::from(!should_take), 0),
        1 | 25 => (u32::from(should_take), 0),
        2 => (if should_take { 0x8000_0000 } else { 0 }, 0),
        3 => (if should_take { 0 } else { 0x8000_0000 }, 0),
        4 => (if should_take { 3 } else { 4 }, 0),
        5 => (if should_take { 4 } else { 3 }, 0),
        6 | 8 => (if should_take { 2 } else { 3 }, 0),
        7 | 9 => (if should_take { 3 } else { 2 }, 0),
        10 => (if should_take { 0 } else { 1 }, 1),
        11 => (1, if should_take { 1 } else { 2 }),
        12 => (if should_take { u32::MAX } else { 1 }, 0),
        13 => (1, if should_take { 2 } else { 0 }),
        14 => (if should_take { u32::MAX } else { 0 }, 1),
        15 => (if should_take { 0 } else { 4 }, 2),
        16 => (if should_take { 0 } else { 4 }, 0),
        17 => (if should_take { 1 } else { 0 }, 1),
        18 => (1, if should_take { 2 } else { 1 }),
        19 => (if should_take { 1 } else { u32::MAX }, 0),
        20 => (if should_take { 2 } else { 0 }, 1),
        21 => (if should_take { 0 } else { u32::MAX }, 1),
        22 => (if should_take { 4 } else { 0 }, 2),
        23 => (if should_take { 4 } else { 0 }, 0),
        _ => panic!("branch kind out of range"),
    };
    registers[1] = s;
    registers[2] = t;
}

fn branch_loop_block(loop_op: Op, branch_kind: usize) -> Vec<u8> {
    let branch_len = if branch_kind >= 24 { 2 } else { 3 };
    let branch_pc = SRAM_BASE + 6;
    let second_addi_pc = branch_pc + branch_len + 3;
    let jump_pc = second_addi_pc + 3;
    let end_pc = jump_pc + 3;
    let mut block = Vec::new();
    block.extend_from_slice(&encode_loop(loop_op, 0, jump_pc));
    block.extend_from_slice(&encode_addi(2, 2, 1));
    block.extend_from_slice(&encode_branch(branch_kind, branch_pc, second_addi_pc));
    block.extend_from_slice(&encode_addi(3, 3, 7));
    block.extend_from_slice(&encode_addi(4, 4, -3));
    block.extend_from_slice(&encode_j(jump_pc, end_pc));
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
        cycles += measured_cost(&cpu, &instruction);
        step(&mut cpu, &mut ram).expect("generated instruction executes");
    }
    panic!("generated block did not terminate");
}

fn measured_cost(cpu: &Cpu, instruction: &xtensa_lx7::Insn) -> u64 {
    match instruction.op {
        _ if branch_outcome(cpu, instruction).is_some() => {
            if branch_outcome(cpu, instruction).expect("conditional branch") {
                3
            } else {
                1
            }
        }
        Op::J => 3,
        Op::Loop | Op::Loopnez | Op::Loopgtz => 5,
        _ => 1,
    }
}

fn expected_branch_op(kind: usize) -> Op {
    [
        Op::Beqz,
        Op::Bnez,
        Op::Bltz,
        Op::Bgez,
        Op::Beqi,
        Op::Bnei,
        Op::Blti,
        Op::Bgei,
        Op::Bltui,
        Op::Bgeui,
        Op::Bnone,
        Op::Beq,
        Op::Blt,
        Op::Bltu,
        Op::Ball,
        Op::Bbc,
        Op::Bbci,
        Op::Bany,
        Op::Bne,
        Op::Bge,
        Op::Bgeu,
        Op::Bnall,
        Op::Bbs,
        Op::Bbsi,
        Op::BeqzN,
        Op::BnezN,
    ][kind]
}

fn branch_outcome(cpu: &Cpu, instruction: &xtensa_lx7::Insn) -> Option<bool> {
    use Op::*;
    let s = cpu.get_ar(instruction.s);
    let t = cpu.get_ar(instruction.t);
    Some(match instruction.op {
        Beqz | BeqzN => s == 0,
        Bnez | BnezN => s != 0,
        Bltz => (s as i32) < 0,
        Bgez => (s as i32) >= 0,
        Beqi => s == instruction.imm2 as u32,
        Bnei => s != instruction.imm2 as u32,
        Blti => (s as i32) < instruction.imm2,
        Bgei => (s as i32) >= instruction.imm2,
        Bltui => s < instruction.imm2 as u32,
        Bgeui => s >= instruction.imm2 as u32,
        Bnone => s & t == 0,
        Bany => s & t != 0,
        Ball => !s & t == 0,
        Bnall => !s & t != 0,
        Beq => s == t,
        Bne => s != t,
        Blt => (s as i32) < (t as i32),
        Bge => (s as i32) >= (t as i32),
        Bltu => s < t,
        Bgeu => s >= t,
        Bbc => s & (1 << (t & 31)) == 0,
        Bbs => s & (1 << (t & 31)) != 0,
        Bbci => s & (1 << instruction.imm2) == 0,
        Bbsi => s & (1 << instruction.imm2) != 0,
        _ => return None,
    })
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
    let path = std::path::Path::new("/tmp").join(format!(
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

fn encode_branch(kind: usize, pc: u32, target: u32) -> Vec<u8> {
    let s = 1u8;
    let t = 2u8;
    if kind < 4 {
        let offset = target.wrapping_sub(pc + 4) as i32;
        let word = ((offset as u32 & 0xfff) << 12) | ((kind as u32) << 6) | (1 << 4) | 6;
        return vec![
            word as u8,
            ((word >> 8) as u8 & 0xf0) | s,
            (word >> 16) as u8,
        ];
    }
    if kind < 10 {
        let immediate_index = 3u8;
        let format = [0u32, 1, 2, 3, 2, 3][kind - 4];
        let n = if kind < 8 { 2 } else { 3 };
        let offset = target.wrapping_sub(pc + 4) as u8;
        return vec![
            ((format << 6) | (n << 4) | 6) as u8,
            (immediate_index << 4) | s,
            offset,
        ];
    }
    if kind < 24 {
        let r = [0u8, 1, 2, 3, 4, 5, 6, 8, 9, 10, 11, 12, 13, 14][kind - 10];
        let offset = target.wrapping_sub(pc + 4) as u8;
        return vec![(t << 4) | 7, (r << 4) | s, offset];
    }
    let offset = target.wrapping_sub(pc + 4) as u8;
    let narrow_t = 8 | (((kind - 24) as u8) << 2) | ((offset >> 4) & 3);
    vec![(narrow_t << 4) | 12, ((offset & 0xf) << 4) | s]
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
