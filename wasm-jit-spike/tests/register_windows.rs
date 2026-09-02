use std::process::Command;

use wasm_jit_spike::{
    emit_windowed, WindowFallback, WindowState, FALLBACK_OFFSET, PHYSICAL_AR_OFFSET,
    PHYSICAL_REGISTER_COUNT, PS_OFFSET, WINDOWBASE_OFFSET, WINDOWSTART_OFFSET,
};
use xtensa_lx7::state::{ps, vec};
use xtensa_lx7::{step, Cpu, FlatRam, Trap};

const SRAM_BASE: u32 = 0x3fc8_0000;
const PC_OFFSET: usize = 64;
const CYCLE_OFFSET: usize = 72;

#[derive(Clone, Debug, PartialEq, Eq)]
struct Snapshot {
    ar: [u32; PHYSICAL_REGISTER_COUNT],
    pc: u32,
    cycles: u64,
    windowbase: u32,
    windowstart: u32,
    ps: u32,
    fallback: WindowFallback,
}

#[test]
fn one_hundred_windowed_call_return_blocks_match_the_interpreter() {
    require_node();
    let mut rng = Rng::new(0x5749_4e44_4f57_5341);
    for case in 0..100 {
        let increment = 1 + case % 3;
        let indirect = case & 1 != 0;
        let narrow_return = case & 2 != 0;
        let block = window_block(increment, indirect, narrow_return);
        let initial = initial_cpu(&mut rng, &block, increment, indirect);
        let mut expected_cpu = initial.clone();
        let expected = interpret_to_exit(&mut expected_cpu, &block);
        let actual = execute_node(
            case,
            &emit_windowed(SRAM_BASE, &block, WindowState::from(&initial))
                .expect("windowed block emits")
                .bytes,
        );
        assert_eq!(actual, expected, "case {case}");
    }
}

#[test]
fn one_hundred_window_overflows_fallback_then_resume_bit_identically() {
    require_node();
    let mut rng = Rng::new(0x4f56_4552_464c_4f57);
    for case in 0..100 {
        let increment = 1 + case % 3;
        let indirect = case & 1 != 0;
        let block = window_block(increment, indirect, case & 2 != 0);
        let initial = overflowing_cpu(&mut rng, &block, increment, indirect);

        let mut expected_cpu = initial.clone();
        run_overflow_handler(&mut expected_cpu, &block);
        let expected = interpret_to_exit(&mut expected_cpu, &block);

        let first = emit_windowed(SRAM_BASE, &block, WindowState::from(&initial))
            .expect("overflowing block still emits");
        let trapped = execute_node(case * 2, &first.bytes);
        assert!(matches!(
            trapped.fallback,
            WindowFallback::Overflow { increment: n } if n == increment as u8
        ));
        assert_eq!(trapped.pc, SRAM_BASE, "fallback precedes the call");

        let mut resumed_cpu = cpu_from_snapshot(trapped);
        run_overflow_handler(&mut resumed_cpu, &block);
        let resumed = emit_windowed(SRAM_BASE, &block, WindowState::from(&resumed_cpu))
            .expect("block resumes after the handler");
        let actual = execute_node(case * 2 + 1, &resumed.bytes);
        assert_eq!(actual, expected, "case {case}");
    }
}

fn require_node() {
    assert!(Command::new("node")
        .arg("--version")
        .status()
        .expect("the exit test requires Node")
        .success());
}

fn initial_cpu(rng: &mut Rng, block: &[u8], increment: usize, indirect: bool) -> Cpu {
    let mut cpu = Cpu {
        pc: SRAM_BASE,
        ps: ps::WOE | ps::UM,
        windowbase: rng.range(16),
        windowstart: 0,
        ..Cpu::default()
    };
    cpu.windowstart = 1 << cpu.windowbase;
    cpu.ar = std::array::from_fn(|_| rng.next_u32());
    cpu.set_ar(1, 0x3fc8_f000);
    if indirect {
        cpu.set_ar(2, function_pc(block, increment));
    }
    cpu
}

fn overflowing_cpu(rng: &mut Rng, block: &[u8], increment: usize, indirect: bool) -> Cpu {
    let mut cpu = initial_cpu(rng, block, increment, indirect);
    cpu.windowstart |= 1 << ((cpu.windowbase + increment as u32) & 15);
    cpu.vecbase = SRAM_BASE + 0x400;
    cpu
}

fn function_pc(_block: &[u8], _increment: usize) -> u32 {
    SRAM_BASE + 8
}

fn window_block(increment: usize, indirect: bool, narrow_return: bool) -> Vec<u8> {
    let target = SRAM_BASE + 8;
    let mut block = Vec::new();
    let call = if indirect {
        encode_callx(increment, 2)
    } else {
        encode_call(increment, SRAM_BASE, target)
    };
    block.extend_from_slice(&call);
    block.extend_from_slice(&encode_j(SRAM_BASE + 3, SRAM_BASE + 20));
    block.extend_from_slice(&[0x3d, 0xf0]);
    block.extend_from_slice(&encode_entry(1, 32));
    block.extend_from_slice(&encode_movsp(3, 1));
    block.extend_from_slice(&encode_addi(2, 2, 7));
    if narrow_return {
        block.extend_from_slice(&[0x1d, 0xf0]);
    } else {
        block.extend_from_slice(&[0x90, 0x00, 0x00]);
    }
    block
}

fn interpret_to_exit(cpu: &mut Cpu, block: &[u8]) -> Snapshot {
    let mut ram = ram_with_program(block, cpu.vecbase);
    let mut cycles = 0;
    for _ in 0..32 {
        if cpu.pc < SRAM_BASE || cpu.pc >= SRAM_BASE + block.len() as u32 {
            return snapshot(cpu, cycles, WindowFallback::None);
        }
        step(cpu, &mut ram).expect("non-overflowing window block executes");
        cycles += 1;
    }
    panic!("window block did not exit");
}

fn run_overflow_handler(cpu: &mut Cpu, block: &[u8]) {
    let mut ram = ram_with_program(block, cpu.vecbase);
    assert!(matches!(
        step(cpu, &mut ram),
        Err(Trap::Exception(0x201..=0x203))
    ));
    let handler = cpu.vecbase
        + match cpu.exccause - 0x200 {
            1 => vec::WINDOW_OF4,
            2 => vec::WINDOW_OF8,
            _ => vec::WINDOW_OF12,
        };
    assert_eq!(cpu.pc, handler);
    step(cpu, &mut ram).expect("synthetic rfwo handler returns to the call");
    assert_eq!(cpu.pc, SRAM_BASE);
}

fn ram_with_program(block: &[u8], vecbase: u32) -> FlatRam {
    let mut ram = FlatRam::new(SRAM_BASE, 4096);
    ram.mem[..block.len()].copy_from_slice(block);
    for offset in [vec::WINDOW_OF4, vec::WINDOW_OF8, vec::WINDOW_OF12] {
        let start = vecbase.wrapping_add(offset).wrapping_sub(SRAM_BASE) as usize;
        ram.mem[start..start + 3].copy_from_slice(&[0x40, 0x30, 0x00]);
    }
    ram
}

fn cpu_from_snapshot(value: Snapshot) -> Cpu {
    Cpu {
        ar: value.ar,
        pc: value.pc,
        windowbase: value.windowbase,
        windowstart: value.windowstart,
        ps: value.ps,
        ..Cpu::default()
    }
}

fn snapshot(cpu: &Cpu, cycles: u64, fallback: WindowFallback) -> Snapshot {
    Snapshot {
        ar: cpu.ar,
        pc: cpu.pc,
        cycles,
        windowbase: cpu.windowbase,
        windowstart: cpu.windowstart,
        ps: cpu.ps,
        fallback,
    }
}

fn execute_node(case: usize, module: &[u8]) -> Snapshot {
    const SCRIPT: &str = r#"
const fs = require('fs');
WebAssembly.instantiate(fs.readFileSync(process.argv[1])).then(({instance}) => {
  instance.exports.run();
  const v = new DataView(instance.exports.memory.buffer);
  const values = [];
  for (let i = 0; i < 64; i++) values.push(v.getUint32(Number(process.argv[2]) + i * 4, true));
  values.push(v.getUint32(64, true), v.getBigUint64(72, true).toString());
  for (let i = 3; i < 7; i++) values.push(v.getUint32(Number(process.argv[i]), true));
  process.stdout.write(values.join(','));
}).catch(error => { console.error(error); process.exit(1); });
"#;
    let path = std::path::Path::new("/tmp").join(format!(
        "esp32sim-wasm-jit-windows-{}-{case}.wasm",
        std::process::id()
    ));
    std::fs::write(&path, module).expect("write wasm");
    let output = Command::new("node")
        .args(["-e", SCRIPT])
        .arg(&path)
        .arg(PHYSICAL_AR_OFFSET.to_string())
        .arg(WINDOWBASE_OFFSET.to_string())
        .arg(WINDOWSTART_OFFSET.to_string())
        .arg(PS_OFFSET.to_string())
        .arg(FALLBACK_OFFSET.to_string())
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
    Snapshot {
        ar: std::array::from_fn(|index| fields[index].parse().expect("ar")),
        pc: fields[64].parse().expect("pc"),
        cycles: fields[65].parse().expect("cycles"),
        windowbase: fields[66].parse().expect("windowbase"),
        windowstart: fields[67].parse().expect("windowstart"),
        ps: fields[68].parse().expect("ps"),
        fallback: WindowFallback::from_code(fields[69].parse().expect("fallback")),
    }
}

fn encode_call(increment: usize, pc: u32, target: u32) -> [u8; 3] {
    let words = target.wrapping_sub((pc & !3).wrapping_add(4)) >> 2;
    let raw = (words << 6) | ((increment as u32) << 4) | 5;
    [raw as u8, (raw >> 8) as u8, (raw >> 16) as u8]
}

fn encode_callx(increment: usize, source: u8) -> [u8; 3] {
    [((12 + increment as u8) << 4), source, 0]
}

fn encode_entry(source: u8, frame_bytes: u32) -> [u8; 3] {
    let raw = 0x36 | (u32::from(source) << 8) | ((frame_bytes >> 3) << 12);
    [raw as u8, (raw >> 8) as u8, (raw >> 16) as u8]
}

fn encode_movsp(target: u8, source: u8) -> [u8; 3] {
    [target << 4, 0x10 | source, 0]
}

fn encode_addi(target: u8, source: u8, immediate: i8) -> [u8; 3] {
    [(target << 4) | 2, 0xc0 | source, immediate as u8]
}

fn encode_j(pc: u32, target: u32) -> [u8; 3] {
    let offset = target.wrapping_sub(pc + 4) as i32;
    let raw = ((offset as u32 & 0x3ffff) << 6) | 6;
    [raw as u8, (raw >> 8) as u8, (raw >> 16) as u8]
}

struct Rng(u64);

impl Rng {
    fn new(state: u64) -> Self {
        Self(state)
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
