use std::fmt;

use xtensa_lx7::{decode, Insn, Op};

pub const REGISTER_COUNT: usize = 16;
const REGISTER_BYTES: usize = REGISTER_COUNT * 4;
const PC_OFFSET: usize = REGISTER_BYTES;
const CYCLE_OFFSET: usize = 72;
const STATE_BYTES: usize = 80;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CompileError {
    EmptyBlock,
    TruncatedInstruction { offset: usize, decoded_len: usize },
    UnsupportedInstruction { pc: u32, op: Op },
}

impl fmt::Display for CompileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyBlock => write!(f, "empty block"),
            Self::TruncatedInstruction {
                offset,
                decoded_len,
            } => write!(f, "instruction at byte {offset} needs {decoded_len} bytes"),
            Self::UnsupportedInstruction { pc, op } => {
                write!(
                    f,
                    "unsupported straight-line instruction {op:?} at {pc:#010x}"
                )
            }
        }
    }
}

impl std::error::Error for CompileError {}

#[derive(Debug, Clone)]
pub struct EmittedModule {
    pub bytes: Vec<u8>,
    pub instruction_count: usize,
    pub cycle_cost: u64,
}

pub fn price_table_cost(instruction: &Insn) -> Result<u64, CompileError> {
    match instruction.op {
        Op::Movi | Op::Addi | Op::Add | Op::Sub | Op::And | Op::Or | Op::Xor => Ok(1),
        op => Err(CompileError::UnsupportedInstruction { pc: 0, op }),
    }
}

pub fn emit(
    base_pc: u32,
    block: &[u8],
    initial_registers: [u32; REGISTER_COUNT],
) -> Result<EmittedModule, CompileError> {
    if block.is_empty() {
        return Err(CompileError::EmptyBlock);
    }

    let mut offset = 0usize;
    let mut body = Vec::new();
    let mut instruction_count = 0usize;
    let mut cycle_cost = 0u64;
    while offset < block.len() {
        let mut bytes = [0u8; 4];
        let available = (block.len() - offset).min(bytes.len());
        bytes[..available].copy_from_slice(&block[offset..offset + available]);
        let pc = base_pc.wrapping_add(offset as u32);
        let instruction = decode(pc, bytes);
        let decoded_len = instruction.len as usize;
        if decoded_len > block.len() - offset {
            return Err(CompileError::TruncatedInstruction {
                offset,
                decoded_len,
            });
        }

        cycle_cost += price_at(pc, &instruction)?;
        emit_instruction(&mut body, pc, &instruction)?;
        instruction_count += 1;
        offset += decoded_len;
    }

    emit_cycle_charge(&mut body, cycle_cost);
    body.push(0x0b);

    let final_pc = base_pc.wrapping_add(block.len() as u32);
    let state = initial_state(initial_registers, final_pc);
    Ok(EmittedModule {
        bytes: wasm_module(body, &state),
        instruction_count,
        cycle_cost,
    })
}

fn price_at(pc: u32, instruction: &Insn) -> Result<u64, CompileError> {
    price_table_cost(instruction).map_err(|error| match error {
        CompileError::UnsupportedInstruction { op, .. } => {
            CompileError::UnsupportedInstruction { pc, op }
        }
        other => other,
    })
}

fn emit_instruction(body: &mut Vec<u8>, pc: u32, instruction: &Insn) -> Result<(), CompileError> {
    match instruction.op {
        Op::Movi => {
            emit_i32_const(body, register_offset(instruction.t));
            emit_i32_const(body, instruction.imm);
            emit_i32_store(body);
        }
        Op::Addi => {
            emit_i32_const(body, register_offset(instruction.t));
            emit_register_load(body, instruction.s);
            emit_i32_const(body, instruction.imm);
            body.push(0x6a);
            emit_i32_store(body);
        }
        Op::Add | Op::Sub | Op::And | Op::Or | Op::Xor => {
            emit_i32_const(body, register_offset(instruction.r));
            emit_register_load(body, instruction.s);
            emit_register_load(body, instruction.t);
            body.push(match instruction.op {
                Op::Add => 0x6a,
                Op::Sub => 0x6b,
                Op::And => 0x71,
                Op::Or => 0x72,
                Op::Xor => 0x73,
                _ => unreachable!(),
            });
            emit_i32_store(body);
        }
        op => return Err(CompileError::UnsupportedInstruction { pc, op }),
    }
    Ok(())
}

fn emit_register_load(body: &mut Vec<u8>, register: u8) {
    emit_i32_const(body, register_offset(register));
    body.extend_from_slice(&[0x28, 0x02, 0x00]);
}

fn register_offset(register: u8) -> i32 {
    i32::from(register) * 4
}

fn emit_i32_store(body: &mut Vec<u8>) {
    body.extend_from_slice(&[0x36, 0x02, 0x00]);
}

fn emit_cycle_charge(body: &mut Vec<u8>, cycle_cost: u64) {
    emit_i32_const(body, CYCLE_OFFSET as i32);
    emit_i32_const(body, CYCLE_OFFSET as i32);
    body.extend_from_slice(&[0x29, 0x03, 0x00]);
    body.push(0x42);
    push_sleb(body, cycle_cost as i64);
    body.push(0x7c);
    body.extend_from_slice(&[0x37, 0x03, 0x00]);
}

fn emit_i32_const(body: &mut Vec<u8>, value: i32) {
    body.push(0x41);
    push_sleb(body, i64::from(value));
}

fn initial_state(registers: [u32; REGISTER_COUNT], final_pc: u32) -> [u8; STATE_BYTES] {
    let mut state = [0u8; STATE_BYTES];
    for (index, value) in registers.into_iter().enumerate() {
        let start = index * 4;
        state[start..start + 4].copy_from_slice(&value.to_le_bytes());
    }
    state[PC_OFFSET..PC_OFFSET + 4].copy_from_slice(&final_pc.to_le_bytes());
    state
}

fn wasm_module(body: Vec<u8>, state: &[u8; STATE_BYTES]) -> Vec<u8> {
    let mut module = b"\0asm\x01\0\0\0".to_vec();
    append_section(&mut module, 1, &[1, 0x60, 0, 0]);
    append_section(&mut module, 3, &[1, 0]);
    append_section(&mut module, 5, &[1, 0, 1]);

    let mut exports = Vec::new();
    push_uleb(&mut exports, 2);
    append_name(&mut exports, "memory");
    exports.extend_from_slice(&[2, 0]);
    append_name(&mut exports, "run");
    exports.extend_from_slice(&[0, 0]);
    append_section(&mut module, 7, &exports);

    let mut code = Vec::new();
    push_uleb(&mut code, 1);
    push_uleb(&mut code, body.len() + 1);
    code.push(0);
    code.extend_from_slice(&body);
    append_section(&mut module, 10, &code);

    let mut data = vec![1, 0, 0x41, 0, 0x0b];
    push_uleb(&mut data, state.len());
    data.extend_from_slice(state);
    append_section(&mut module, 11, &data);
    module
}

fn append_section(module: &mut Vec<u8>, id: u8, contents: &[u8]) {
    module.push(id);
    push_uleb(module, contents.len());
    module.extend_from_slice(contents);
}

fn append_name(bytes: &mut Vec<u8>, name: &str) {
    push_uleb(bytes, name.len());
    bytes.extend_from_slice(name.as_bytes());
}

fn push_uleb(bytes: &mut Vec<u8>, mut value: usize) {
    loop {
        let mut byte = (value & 0x7f) as u8;
        value >>= 7;
        if value != 0 {
            byte |= 0x80;
        }
        bytes.push(byte);
        if value == 0 {
            break;
        }
    }
}

fn push_sleb(bytes: &mut Vec<u8>, mut value: i64) {
    loop {
        let byte = (value as u8) & 0x7f;
        value >>= 7;
        let done = (value == 0 && byte & 0x40 == 0) || (value == -1 && byte & 0x40 != 0);
        bytes.push(if done { byte } else { byte | 0x80 });
        if done {
            break;
        }
    }
}
