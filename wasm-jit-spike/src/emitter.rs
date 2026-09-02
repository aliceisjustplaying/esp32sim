use std::fmt;

use xtensa_lx7::{decode, Insn, Op};

pub const REGISTER_COUNT: usize = 16;
const REGISTER_BYTES: usize = REGISTER_COUNT * 4;
const PC_OFFSET: usize = REGISTER_BYTES;
const CYCLE_OFFSET: usize = 72;
const LBEG_OFFSET: usize = 80;
const LEND_OFFSET: usize = 84;
const LCOUNT_OFFSET: usize = 88;
const GUEST_BYTES_OFFSET: usize = 128;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CompileError {
    EmptyBlock,
    TruncatedInstruction { offset: usize, decoded_len: usize },
    LiteralOutsideBlock { pc: u32, address: u32 },
    IntervalCost { pc: u32, op: Op },
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
            Self::LiteralOutsideBlock { pc, address } => write!(
                f,
                "l32r at {pc:#010x} reads outside the emitted block at {address:#010x}"
            ),
            Self::IntervalCost { pc, op } => {
                write!(f, "{op:?} at {pc:#010x} has no adopted scalar cost")
            }
            Self::UnsupportedInstruction { pc, op } => {
                write!(f, "unsupported instruction {op:?} at {pc:#010x}")
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
    pub canonical_ledger: Vec<u8>,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct EmitOptions {
    /// Resolve the adopted 1 to 2 cycle `l32r` interval for an observed run.
    pub literal_load_cycles: Option<u64>,
}

pub fn price_table_cost(instruction: &Insn) -> Result<u64, CompileError> {
    match instruction.op {
        Op::Movi | Op::Addi | Op::Add | Op::Sub | Op::And | Op::Or | Op::Xor => Ok(1),
        op if is_conditional_branch(op) => Ok(1),
        Op::J => Ok(3),
        Op::Loop | Op::Loopnez | Op::Loopgtz => Ok(5),
        Op::L32r => Err(CompileError::IntervalCost {
            pc: 0,
            op: Op::L32r,
        }),
        op => Err(CompileError::UnsupportedInstruction { pc: 0, op }),
    }
}

pub fn emit(
    base_pc: u32,
    block: &[u8],
    initial_registers: [u32; REGISTER_COUNT],
) -> Result<EmittedModule, CompileError> {
    emit_with_options(base_pc, block, initial_registers, EmitOptions::default())
}

pub fn emit_with_options(
    base_pc: u32,
    block: &[u8],
    initial_registers: [u32; REGISTER_COUNT],
    options: EmitOptions,
) -> Result<EmittedModule, CompileError> {
    if block.is_empty() {
        return Err(CompileError::EmptyBlock);
    }

    let mut offset = 0usize;
    let mut instructions = Vec::new();
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

        cycle_cost += price_at(pc, &instruction, options)?;
        validate_instruction(base_pc, block.len(), pc, &instruction)?;
        instructions.push((pc, instruction));
        instruction_count += 1;
        offset += decoded_len;
    }

    let mut body = Vec::new();
    body.extend_from_slice(&[0x02, 0x40, 0x03, 0x40]);
    for (pc, instruction) in instructions {
        emit_state_load(&mut body, PC_OFFSET);
        emit_i32_const(&mut body, pc as i32);
        body.push(0x46);
        body.extend_from_slice(&[0x04, 0x40]);
        emit_instruction(&mut body, base_pc, block.len(), pc, &instruction, options)?;
        body.extend_from_slice(&[0x0c, 0x01, 0x0b]);
    }
    body.extend_from_slice(&[0x0c, 0x01, 0x0b, 0x0b]);
    body.push(0x0b);

    let state = initial_state(initial_registers, base_pc, block);
    Ok(EmittedModule {
        bytes: wasm_module(body, &state),
        instruction_count,
        cycle_cost,
        canonical_ledger: Vec::new(),
    })
}

fn price_at(pc: u32, instruction: &Insn, options: EmitOptions) -> Result<u64, CompileError> {
    if instruction.op == Op::L32r {
        return match options.literal_load_cycles {
            Some(cost @ 1..=2) => Ok(cost),
            _ => Err(CompileError::IntervalCost {
                pc,
                op: instruction.op,
            }),
        };
    }
    price_table_cost(instruction).map_err(|error| match error {
        CompileError::UnsupportedInstruction { op, .. } => {
            CompileError::UnsupportedInstruction { pc, op }
        }
        CompileError::IntervalCost { op, .. } => CompileError::IntervalCost { pc, op },
        other => other,
    })
}

fn validate_instruction(
    base_pc: u32,
    block_len: usize,
    pc: u32,
    instruction: &Insn,
) -> Result<(), CompileError> {
    if instruction.op == Op::L32r {
        let address = instruction.imm as u32;
        let end = address
            .checked_add(4)
            .ok_or(CompileError::LiteralOutsideBlock { pc, address })?;
        if address < base_pc || end > base_pc.wrapping_add(block_len as u32) {
            return Err(CompileError::LiteralOutsideBlock { pc, address });
        }
    }
    Ok(())
}

fn emit_instruction(
    body: &mut Vec<u8>,
    base_pc: u32,
    block_len: usize,
    pc: u32,
    instruction: &Insn,
    options: EmitOptions,
) -> Result<(), CompileError> {
    let next = pc.wrapping_add(instruction.len as u32);
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
        op if is_conditional_branch(op) => {
            emit_branch_condition(body, instruction);
            body.extend_from_slice(&[0x04, 0x40]);
            emit_state_store_const(body, PC_OFFSET, instruction.imm);
            emit_cycle_charge(body, 3);
            body.push(0x05);
            emit_state_store_const(body, PC_OFFSET, next as i32);
            emit_cycle_charge(body, 1);
            emit_loop_backedge(body);
            body.push(0x0b);
            return Ok(());
        }
        Op::J => {
            emit_state_store_const(body, PC_OFFSET, instruction.imm);
            emit_cycle_charge(body, 3);
            return Ok(());
        }
        Op::Loop | Op::Loopnez | Op::Loopgtz => {
            emit_i32_const(body, LCOUNT_OFFSET as i32);
            emit_register_load(body, instruction.s);
            emit_i32_const(body, 1);
            body.push(0x6b);
            emit_i32_store(body);
            emit_state_store_const(body, LBEG_OFFSET, next as i32);
            emit_state_store_const(body, LEND_OFFSET, instruction.imm);
            if instruction.op == Op::Loop {
                emit_state_store_const(body, PC_OFFSET, next as i32);
                emit_loop_backedge(body);
            } else {
                emit_register_load(body, instruction.s);
                if instruction.op == Op::Loopnez {
                    body.push(0x45);
                } else {
                    emit_i32_const(body, 0);
                    body.push(0x4c);
                }
                body.extend_from_slice(&[0x04, 0x40]);
                emit_state_store_const(body, PC_OFFSET, instruction.imm);
                body.push(0x05);
                emit_state_store_const(body, PC_OFFSET, next as i32);
                emit_loop_backedge(body);
                body.push(0x0b);
            }
            emit_cycle_charge(body, 5);
            return Ok(());
        }
        Op::L32r => {
            let address = instruction.imm as u32;
            debug_assert!(address >= base_pc);
            debug_assert!(address.wrapping_add(4) <= base_pc.wrapping_add(block_len as u32));
            emit_i32_const(body, register_offset(instruction.t));
            emit_i32_const(
                body,
                (GUEST_BYTES_OFFSET as u32 + address.wrapping_sub(base_pc)) as i32,
            );
            body.extend_from_slice(&[0x28, 0x02, 0x00]);
            emit_i32_store(body);
            emit_state_store_const(body, PC_OFFSET, next as i32);
            emit_cycle_charge(
                body,
                options
                    .literal_load_cycles
                    .expect("l32r cost validated before emission"),
            );
            emit_loop_backedge(body);
            return Ok(());
        }
        op => return Err(CompileError::UnsupportedInstruction { pc, op }),
    }
    emit_state_store_const(body, PC_OFFSET, next as i32);
    emit_cycle_charge(body, 1);
    emit_loop_backedge(body);
    Ok(())
}

fn is_conditional_branch(op: Op) -> bool {
    matches!(
        op,
        Op::Beqz
            | Op::Bnez
            | Op::Bltz
            | Op::Bgez
            | Op::BeqzN
            | Op::BnezN
            | Op::Beqi
            | Op::Bnei
            | Op::Blti
            | Op::Bgei
            | Op::Bltui
            | Op::Bgeui
            | Op::Bnone
            | Op::Beq
            | Op::Blt
            | Op::Bltu
            | Op::Ball
            | Op::Bbc
            | Op::Bbci
            | Op::Bany
            | Op::Bne
            | Op::Bge
            | Op::Bgeu
            | Op::Bnall
            | Op::Bbs
            | Op::Bbsi
    )
}

fn emit_branch_condition(body: &mut Vec<u8>, instruction: &Insn) {
    use Op::*;
    let comparison = |body: &mut Vec<u8>, opcode: u8| {
        emit_register_load(body, instruction.s);
        emit_register_load(body, instruction.t);
        body.push(opcode);
    };
    let immediate_comparison = |body: &mut Vec<u8>, opcode: u8| {
        emit_register_load(body, instruction.s);
        emit_i32_const(body, instruction.imm2);
        body.push(opcode);
    };
    match instruction.op {
        Beqz | BeqzN => {
            emit_register_load(body, instruction.s);
            body.push(0x45);
        }
        Bnez | BnezN => {
            emit_register_load(body, instruction.s);
            emit_i32_const(body, 0);
            body.push(0x47);
        }
        Bltz => immediate_comparison(body, 0x48),
        Bgez => immediate_comparison(body, 0x4e),
        Beqi => immediate_comparison(body, 0x46),
        Bnei => immediate_comparison(body, 0x47),
        Blti => immediate_comparison(body, 0x48),
        Bgei => immediate_comparison(body, 0x4e),
        Bltui => immediate_comparison(body, 0x49),
        Bgeui => immediate_comparison(body, 0x4f),
        Beq => comparison(body, 0x46),
        Bne => comparison(body, 0x47),
        Blt => comparison(body, 0x48),
        Bge => comparison(body, 0x4e),
        Bltu => comparison(body, 0x49),
        Bgeu => comparison(body, 0x4f),
        Bnone | Bany | Ball | Bnall => {
            emit_register_load(body, instruction.s);
            if matches!(instruction.op, Ball | Bnall) {
                emit_i32_const(body, -1);
                body.push(0x73);
            }
            emit_register_load(body, instruction.t);
            body.push(0x71);
            body.push(0x45);
            if matches!(instruction.op, Bany | Bnall) {
                body.push(0x45);
            }
        }
        Bbc | Bbs => {
            emit_register_load(body, instruction.s);
            emit_i32_const(body, 1);
            emit_register_load(body, instruction.t);
            emit_i32_const(body, 31);
            body.push(0x71);
            body.push(0x74);
            body.push(0x71);
            body.push(0x45);
            if instruction.op == Bbs {
                body.push(0x45);
            }
        }
        Bbci | Bbsi => {
            emit_register_load(body, instruction.s);
            emit_i32_const(body, 1i32.wrapping_shl(instruction.imm2 as u32));
            body.push(0x71);
            body.push(0x45);
            if instruction.op == Bbsi {
                body.push(0x45);
            }
        }
        _ => unreachable!("only conditional branches reach branch emission"),
    }
}

fn emit_register_load(body: &mut Vec<u8>, register: u8) {
    emit_i32_const(body, register_offset(register));
    body.extend_from_slice(&[0x28, 0x02, 0x00]);
}

fn emit_state_load(body: &mut Vec<u8>, offset: usize) {
    emit_i32_const(body, offset as i32);
    body.extend_from_slice(&[0x28, 0x02, 0x00]);
}

fn emit_state_store_const(body: &mut Vec<u8>, offset: usize, value: i32) {
    emit_i32_const(body, offset as i32);
    emit_i32_const(body, value);
    emit_i32_store(body);
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

fn emit_loop_backedge(body: &mut Vec<u8>) {
    emit_state_load(body, PC_OFFSET);
    emit_state_load(body, LEND_OFFSET);
    body.push(0x46);
    emit_state_load(body, LCOUNT_OFFSET);
    emit_i32_const(body, 0);
    body.push(0x47);
    body.push(0x71);
    body.extend_from_slice(&[0x04, 0x40]);
    emit_i32_const(body, LCOUNT_OFFSET as i32);
    emit_state_load(body, LCOUNT_OFFSET);
    emit_i32_const(body, 1);
    body.push(0x6b);
    emit_i32_store(body);
    emit_i32_const(body, PC_OFFSET as i32);
    emit_state_load(body, LBEG_OFFSET);
    emit_i32_store(body);
    body.push(0x0b);
}

fn emit_i32_const(body: &mut Vec<u8>, value: i32) {
    body.push(0x41);
    push_sleb(body, i64::from(value));
}

fn initial_state(registers: [u32; REGISTER_COUNT], initial_pc: u32, block: &[u8]) -> Vec<u8> {
    let mut state = vec![0u8; GUEST_BYTES_OFFSET + block.len()];
    for (index, value) in registers.into_iter().enumerate() {
        let start = index * 4;
        state[start..start + 4].copy_from_slice(&value.to_le_bytes());
    }
    state[PC_OFFSET..PC_OFFSET + 4].copy_from_slice(&initial_pc.to_le_bytes());
    state[GUEST_BYTES_OFFSET..].copy_from_slice(block);
    state
}

fn wasm_module(body: Vec<u8>, state: &[u8]) -> Vec<u8> {
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
