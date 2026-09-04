use backend_api::{
    price_operation, ChipConfig, CoreId, ExecutionOutcome, InstructionCost, MmioTier, Operation,
    TransactionEngine,
};
use xtensa_lx7::state::ps;
use xtensa_lx7::{decode, Cpu, Insn, Op};

use crate::{CompileError, EmittedModule, REGISTER_COUNT};

const PC_OFFSET: usize = 64;
const CYCLE_OFFSET: usize = 72;
pub const PHYSICAL_REGISTER_COUNT: usize = 64;
pub const PHYSICAL_AR_OFFSET: usize = 256;
pub const WINDOWBASE_OFFSET: usize = 512;
pub const WINDOWSTART_OFFSET: usize = 516;
pub const PS_OFFSET: usize = 520;
pub const FALLBACK_OFFSET: usize = 524;
const TEMP_OFFSET: usize = 528;
const TEMP2_OFFSET: usize = 532;
const HOST_RESULT_OFFSET: usize = 536;
const HOST_ADDRESS_OFFSET: usize = 544;
pub const POSTED_WRITES_OFFSET: usize = 548;
pub const SRAM_IMAGE_OFFSET: usize = 4096;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HostMemoryClass {
    Mmio(MmioTier),
    Flash,
    Psram,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum WindowFallback {
    #[default]
    None,
    Overflow {
        increment: u8,
    },
    Underflow {
        increment: u8,
    },
    Alloca,
    Illegal,
}

impl WindowFallback {
    pub const fn from_code(code: u32) -> Self {
        match code {
            1..=3 => Self::Overflow {
                increment: code as u8,
            },
            11..=13 => Self::Underflow {
                increment: (code - 10) as u8,
            },
            20 => Self::Alloca,
            21 => Self::Illegal,
            _ => Self::None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WindowState {
    pub ar: [u32; PHYSICAL_REGISTER_COUNT],
    pub pc: u32,
    pub cycles: u64,
    pub windowbase: u32,
    pub windowstart: u32,
    pub ps: u32,
}

impl From<&Cpu> for WindowState {
    fn from(cpu: &Cpu) -> Self {
        Self {
            ar: cpu.ar,
            pc: cpu.pc,
            cycles: 0,
            windowbase: cpu.windowbase,
            windowstart: cpu.windowstart,
            ps: cpu.ps,
        }
    }
}

pub fn emit_windowed(
    base_pc: u32,
    block: &[u8],
    initial: WindowState,
) -> Result<EmittedModule, CompileError> {
    let instructions = decode_block(base_pc, block)?;
    let mut body = function_prefix();
    let mut cycle_cost = 0;
    for (pc, instruction) in &instructions {
        emit_current_pc_guard(&mut body, *pc);
        let cost = emit_window_instruction(&mut body, *pc, instruction)?;
        cycle_cost += cost;
        emit_continue(&mut body);
    }
    finish_function(&mut body);
    let state = window_initial_state(&initial);
    Ok(EmittedModule {
        bytes: wasm_module(body, &state),
        instruction_count: instructions.len(),
        cycle_cost,
        canonical_ledger: Vec::new(),
    })
}

pub fn emit_sram(
    base_pc: u32,
    block: &[u8],
    initial_registers: [u32; REGISTER_COUNT],
    sram_base: u32,
    sram: &[u8],
) -> Result<EmittedModule, CompileError> {
    let instructions = decode_block(base_pc, block)?;
    let costs = sram_costs(&instructions)?;
    let mut body = function_prefix();
    for ((pc, instruction), cost) in instructions.iter().zip(&costs) {
        emit_current_pc_guard(&mut body, *pc);
        emit_sram_instruction(&mut body, *pc, instruction, sram_base)?;
        emit_state_store_const(
            &mut body,
            PC_OFFSET,
            pc.wrapping_add(u32::from(instruction.len)) as i32,
        );
        emit_cycle_charge(&mut body, *cost);
        emit_continue(&mut body);
    }
    finish_function(&mut body);

    let mut state = vec![0; SRAM_IMAGE_OFFSET + sram.len()];
    for (index, value) in initial_registers.into_iter().enumerate() {
        store_u32(&mut state, index * 4, value);
    }
    store_u32(&mut state, PC_OFFSET, base_pc);
    state[SRAM_IMAGE_OFFSET..].copy_from_slice(sram);
    let canonical_ledger = sram_ledger(&instructions)?;
    Ok(EmittedModule {
        bytes: wasm_module(body, &state),
        instruction_count: instructions.len(),
        cycle_cost: costs.iter().sum(),
        canonical_ledger,
    })
}

/// Emit external-memory operations. MMIO values come from the `env.mmio`
/// import. Flash and PSRAM values and cache-fill cycles come from the single
/// `env.cache_access` import, leaving replacement and fill state with the host
/// cache model.
pub fn emit_host_memory(
    base_pc: u32,
    block: &[u8],
    initial_registers: [u32; REGISTER_COUNT],
    classes: &[HostMemoryClass],
    posted_mmio_writes: u8,
    config: ChipConfig,
) -> Result<EmittedModule, CompileError> {
    let instructions = decode_block(base_pc, block)?;
    if instructions.len() != classes.len() {
        return Err(CompileError::TruncatedInstruction {
            offset: classes.len(),
            decoded_len: instructions.len(),
        });
    }
    let mut body = function_prefix();
    for ((pc, instruction), class) in instructions.iter().zip(classes) {
        if !is_memory(instruction.op) {
            return Err(CompileError::UnsupportedInstruction {
                pc: *pc,
                op: instruction.op,
            });
        }
        validate_external_config(config, *pc, instruction.op, *class)?;
        emit_current_pc_guard(&mut body, *pc);
        emit_host_access(&mut body, *pc, instruction, *class, config)?;
        emit_state_store_const(
            &mut body,
            PC_OFFSET,
            pc.wrapping_add(u32::from(instruction.len)) as i32,
        );
        emit_continue(&mut body);
    }
    finish_function(&mut body);
    let mut state = vec![0; POSTED_WRITES_OFFSET + 4];
    for (index, value) in initial_registers.into_iter().enumerate() {
        store_u32(&mut state, index * 4, value);
    }
    store_u32(&mut state, PC_OFFSET, base_pc);
    store_u32(
        &mut state,
        POSTED_WRITES_OFFSET,
        u32::from(posted_mmio_writes),
    );
    Ok(EmittedModule {
        bytes: wasm_module_with_host_imports(body, &state),
        instruction_count: instructions.len(),
        cycle_cost: 0,
        canonical_ledger: Vec::new(),
    })
}

fn decode_block(base_pc: u32, block: &[u8]) -> Result<Vec<(u32, Insn)>, CompileError> {
    if block.is_empty() {
        return Err(CompileError::EmptyBlock);
    }
    let mut offset = 0;
    let mut instructions = Vec::new();
    while offset < block.len() {
        let mut bytes = [0; 4];
        let available = (block.len() - offset).min(bytes.len());
        bytes[..available].copy_from_slice(&block[offset..offset + available]);
        let pc = base_pc.wrapping_add(offset as u32);
        let instruction = decode(pc, bytes);
        let len = instruction.len as usize;
        if len > block.len() - offset {
            return Err(CompileError::TruncatedInstruction {
                offset,
                decoded_len: len,
            });
        }
        instructions.push((pc, instruction));
        offset += len;
    }
    Ok(instructions)
}

fn emit_window_instruction(
    body: &mut Vec<u8>,
    pc: u32,
    instruction: &Insn,
) -> Result<u64, CompileError> {
    let next = pc.wrapping_add(u32::from(instruction.len));
    emit_overflow_fallback(body, window_max_ar(instruction));
    match instruction.op {
        Op::Call4 | Op::Call8 | Op::Call12 | Op::Callx4 | Op::Callx8 | Op::Callx12 => {
            let increment = match instruction.op {
                Op::Call4 | Op::Callx4 => 1,
                Op::Call8 | Op::Callx8 => 2,
                _ => 3,
            };
            emit_require_windows(body);
            if matches!(instruction.op, Op::Callx4 | Op::Callx8 | Op::Callx12) {
                emit_i32_const(body, TEMP_OFFSET as i32);
                emit_window_ar_load(body, instruction.s);
                emit_i32_store(body);
            }
            emit_i32_const(body, PS_OFFSET as i32);
            emit_state_load(body, PS_OFFSET);
            emit_i32_const(body, !(ps::CALLINC_MASK as i32));
            body.push(0x71);
            emit_i32_const(body, (increment << ps::CALLINC_SHIFT) as i32);
            body.push(0x72);
            emit_i32_store(body);
            emit_window_ar_addr(body, (increment * 4) as u8);
            emit_i32_const(
                body,
                ((increment << 30) | u32::from(instruction.len).wrapping_add(pc) & 0x3fff_ffff)
                    as i32,
            );
            emit_i32_store(body);
            if matches!(instruction.op, Op::Callx4 | Op::Callx8 | Op::Callx12) {
                emit_i32_const(body, PC_OFFSET as i32);
                emit_state_load(body, TEMP_OFFSET);
                emit_i32_store(body);
            } else {
                emit_state_store_const(body, PC_OFFSET, instruction.imm);
            }
        }
        Op::Entry => {
            emit_require_windows(body);
            if instruction.s > 3 {
                emit_state_store_const(body, FALLBACK_OFFSET, 21);
                body.push(0x0f);
            }
            emit_i32_const(body, TEMP_OFFSET as i32);
            emit_window_ar_load(body, instruction.s);
            emit_i32_const(body, instruction.imm);
            body.push(0x6b);
            emit_i32_store(body);
            emit_i32_const(body, WINDOWBASE_OFFSET as i32);
            emit_state_load(body, WINDOWBASE_OFFSET);
            emit_state_load(body, PS_OFFSET);
            emit_i32_const(body, ps::CALLINC_SHIFT as i32);
            body.push(0x76);
            emit_i32_const(body, 3);
            body.push(0x71);
            body.push(0x6a);
            emit_i32_const(body, 15);
            body.push(0x71);
            emit_i32_store(body);
            emit_i32_const(body, WINDOWSTART_OFFSET as i32);
            emit_state_load(body, WINDOWSTART_OFFSET);
            emit_i32_const(body, 1);
            emit_state_load(body, WINDOWBASE_OFFSET);
            body.push(0x74);
            body.push(0x72);
            emit_i32_store(body);
            emit_window_ar_addr(body, instruction.s);
            emit_state_load(body, TEMP_OFFSET);
            emit_i32_store(body);
            emit_state_store_const(body, PC_OFFSET, next as i32);
        }
        Op::Retw | Op::RetwN => emit_retw(body, pc),
        Op::Movsp => {
            emit_movsp_guard(body);
            emit_window_ar_addr(body, instruction.t);
            emit_window_ar_load(body, instruction.s);
            emit_i32_store(body);
            emit_state_store_const(body, PC_OFFSET, next as i32);
        }
        Op::Movi | Op::MoviN => {
            let register = if instruction.op == Op::Movi {
                instruction.t
            } else {
                instruction.s
            };
            emit_window_ar_addr(body, register);
            emit_i32_const(body, instruction.imm);
            emit_i32_store(body);
            emit_state_store_const(body, PC_OFFSET, next as i32);
        }
        Op::Addi => {
            emit_window_ar_addr(body, instruction.t);
            emit_window_ar_load(body, instruction.s);
            emit_i32_const(body, instruction.imm);
            body.push(0x6a);
            emit_i32_store(body);
            emit_state_store_const(body, PC_OFFSET, next as i32);
        }
        Op::Add | Op::Sub | Op::And | Op::Or | Op::Xor => {
            emit_window_ar_addr(body, instruction.r);
            emit_window_ar_load(body, instruction.s);
            emit_window_ar_load(body, instruction.t);
            body.push(match instruction.op {
                Op::Add => 0x6a,
                Op::Sub => 0x6b,
                Op::And => 0x71,
                Op::Or => 0x72,
                Op::Xor => 0x73,
                _ => unreachable!(),
            });
            emit_i32_store(body);
            emit_state_store_const(body, PC_OFFSET, next as i32);
        }
        Op::J => emit_state_store_const(body, PC_OFFSET, instruction.imm),
        Op::Nop | Op::NopN => emit_state_store_const(body, PC_OFFSET, next as i32),
        op => return Err(CompileError::UnsupportedInstruction { pc, op }),
    }
    let cycles = if instruction.op == Op::J { 3 } else { 1 };
    emit_cycle_charge(body, cycles);
    Ok(cycles)
}

fn emit_require_windows(body: &mut Vec<u8>) {
    emit_state_load(body, PS_OFFSET);
    emit_i32_const(body, ps::WOE as i32);
    body.push(0x71);
    body.push(0x45);
    body.extend_from_slice(&[0x04, 0x40]);
    emit_state_store_const(body, FALLBACK_OFFSET, 21);
    body.push(0x0f);
    body.push(0x0b);
}

fn emit_overflow_fallback(body: &mut Vec<u8>, max_ar: u8) {
    for increment in 1..=u32::from(max_ar / 4) {
        emit_state_load(body, PS_OFFSET);
        emit_i32_const(body, ps::WOE as i32);
        body.push(0x71);
        emit_i32_const(body, 0);
        body.push(0x47);
        emit_state_load(body, PS_OFFSET);
        emit_i32_const(body, ps::EXCM as i32);
        body.push(0x71);
        body.push(0x45);
        body.push(0x71);
        emit_state_load(body, WINDOWSTART_OFFSET);
        emit_i32_const(body, 1);
        emit_state_load(body, WINDOWBASE_OFFSET);
        emit_i32_const(body, increment as i32);
        body.push(0x6a);
        emit_i32_const(body, 15);
        body.push(0x71);
        body.push(0x74);
        body.push(0x71);
        emit_i32_const(body, 0);
        body.push(0x47);
        body.push(0x71);
        body.extend_from_slice(&[0x04, 0x40]);
        emit_state_store_const(body, FALLBACK_OFFSET, increment as i32);
        body.push(0x0f);
        body.push(0x0b);
    }
}

fn window_max_ar(instruction: &Insn) -> u8 {
    match instruction.op {
        Op::Call4 => 4,
        Op::Call8 => 8,
        Op::Call12 => 12,
        Op::Callx4 => instruction.s.max(4),
        Op::Callx8 => instruction.s.max(8),
        Op::Callx12 => instruction.s.max(12),
        Op::Movsp | Op::Add | Op::Sub | Op::And | Op::Or | Op::Xor => {
            instruction.r.max(instruction.s).max(instruction.t)
        }
        Op::Addi => instruction.s.max(instruction.t),
        Op::Movi => instruction.t,
        Op::MoviN | Op::Entry => instruction.s,
        _ => 0,
    }
}

fn emit_retw(body: &mut Vec<u8>, pc: u32) {
    emit_require_windows(body);
    emit_i32_const(body, TEMP_OFFSET as i32);
    emit_window_ar_load(body, 0);
    emit_i32_store(body);
    emit_i32_const(body, TEMP2_OFFSET as i32);
    emit_state_load(body, TEMP_OFFSET);
    emit_i32_const(body, 30);
    body.push(0x76);
    emit_i32_store(body);
    emit_state_load(body, TEMP2_OFFSET);
    body.push(0x45);
    body.extend_from_slice(&[0x04, 0x40]);
    emit_state_store_const(body, FALLBACK_OFFSET, 21);
    body.push(0x0f);
    body.push(0x0b);
    emit_i32_const(body, TEMP2_OFFSET as i32);
    emit_state_load(body, WINDOWBASE_OFFSET);
    emit_state_load(body, TEMP2_OFFSET);
    body.push(0x6b);
    emit_i32_const(body, 15);
    body.push(0x71);
    emit_i32_store(body);
    emit_state_load(body, WINDOWSTART_OFFSET);
    emit_i32_const(body, 1);
    emit_state_load(body, TEMP2_OFFSET);
    body.push(0x74);
    body.push(0x71);
    body.push(0x45);
    body.extend_from_slice(&[0x04, 0x40]);
    emit_i32_const(body, FALLBACK_OFFSET as i32);
    emit_state_load(body, TEMP_OFFSET);
    emit_i32_const(body, 30);
    body.push(0x76);
    emit_i32_const(body, 10);
    body.push(0x6a);
    emit_i32_store(body);
    body.push(0x0f);
    body.push(0x0b);
    emit_i32_const(body, WINDOWSTART_OFFSET as i32);
    emit_state_load(body, WINDOWSTART_OFFSET);
    emit_i32_const(body, 1);
    emit_state_load(body, WINDOWBASE_OFFSET);
    body.push(0x74);
    emit_i32_const(body, -1);
    body.push(0x73);
    body.push(0x71);
    emit_i32_store(body);
    emit_i32_const(body, WINDOWBASE_OFFSET as i32);
    emit_state_load(body, TEMP2_OFFSET);
    emit_i32_store(body);
    emit_i32_const(body, PS_OFFSET as i32);
    emit_state_load(body, PS_OFFSET);
    emit_i32_const(body, !(ps::CALLINC_MASK as i32));
    body.push(0x71);
    emit_state_load(body, TEMP_OFFSET);
    emit_i32_const(body, 30);
    body.push(0x76);
    emit_i32_const(body, ps::CALLINC_SHIFT as i32);
    body.push(0x74);
    body.push(0x72);
    emit_i32_store(body);
    emit_i32_const(body, PC_OFFSET as i32);
    emit_state_load(body, TEMP_OFFSET);
    emit_i32_const(body, 0x3fff_ffff);
    body.push(0x71);
    emit_i32_const(body, (pc & 0xc000_0000) as i32);
    body.push(0x72);
    emit_i32_store(body);
}

fn emit_movsp_guard(body: &mut Vec<u8>) {
    emit_state_load(body, WINDOWSTART_OFFSET);
    for distance in 1..=3 {
        emit_i32_const(body, 1);
        emit_state_load(body, WINDOWBASE_OFFSET);
        emit_i32_const(body, distance);
        body.push(0x6b);
        emit_i32_const(body, 15);
        body.push(0x71);
        body.push(0x74);
        if distance != 1 {
            body.push(0x72);
        }
    }
    body.push(0x71);
    body.push(0x45);
    body.extend_from_slice(&[0x04, 0x40]);
    emit_state_store_const(body, FALLBACK_OFFSET, 20);
    body.push(0x0f);
    body.push(0x0b);
}

fn emit_sram_instruction(
    body: &mut Vec<u8>,
    pc: u32,
    instruction: &Insn,
    sram_base: u32,
) -> Result<(), CompileError> {
    match instruction.op {
        Op::L8ui | Op::L16ui | Op::L16si | Op::L32i | Op::L32iN => {
            emit_i32_const(body, i32::from(instruction.t) * 4);
            emit_sram_address(body, instruction, sram_base);
            match instruction.op {
                Op::L8ui => body.extend_from_slice(&[0x2d, 0x00, 0x00]),
                Op::L16ui => body.extend_from_slice(&[0x2f, 0x01, 0x00]),
                Op::L16si => body.extend_from_slice(&[0x2e, 0x01, 0x00]),
                _ => body.extend_from_slice(&[0x28, 0x02, 0x00]),
            }
            emit_i32_store(body);
        }
        Op::S8i | Op::S16i | Op::S32i | Op::S32iN => {
            emit_sram_address(body, instruction, sram_base);
            emit_legacy_ar_load(body, instruction.t);
            match instruction.op {
                Op::S8i => body.extend_from_slice(&[0x3a, 0x00, 0x00]),
                Op::S16i => body.extend_from_slice(&[0x3b, 0x01, 0x00]),
                _ => emit_i32_store(body),
            }
        }
        Op::Movi | Op::MoviN => {
            let register = if instruction.op == Op::Movi {
                instruction.t
            } else {
                instruction.s
            };
            emit_i32_const(body, i32::from(register) * 4);
            emit_i32_const(body, instruction.imm);
            emit_i32_store(body);
        }
        Op::Add | Op::Sub | Op::And | Op::Or | Op::Xor | Op::Saltu => {
            emit_i32_const(body, i32::from(instruction.r) * 4);
            emit_legacy_ar_load(body, instruction.s);
            emit_legacy_ar_load(body, instruction.t);
            body.push(match instruction.op {
                Op::Add => 0x6a,
                Op::Sub => 0x6b,
                Op::And => 0x71,
                Op::Or => 0x72,
                Op::Xor => 0x73,
                Op::Saltu => 0x49,
                _ => unreachable!(),
            });
            emit_i32_store(body);
        }
        Op::Memw | Op::Nop | Op::NopN => {}
        op => return Err(CompileError::UnsupportedInstruction { pc, op }),
    }
    Ok(())
}

fn emit_host_access(
    body: &mut Vec<u8>,
    pc: u32,
    instruction: &Insn,
    class: HostMemoryClass,
    config: ChipConfig,
) -> Result<(), CompileError> {
    let store = is_store(instruction.op);
    emit_i32_const(body, HOST_ADDRESS_OFFSET as i32);
    emit_legacy_ar_load(body, instruction.s);
    emit_i32_const(body, instruction.imm);
    body.push(0x6a);
    emit_i32_store(body);

    emit_i32_const(body, HOST_RESULT_OFFSET as i32);
    emit_i32_const(
        body,
        match class {
            HostMemoryClass::Mmio(tier) => mmio_tier_code(tier),
            HostMemoryClass::Flash => 5,
            HostMemoryClass::Psram => 6,
        },
    );
    emit_i32_const(body, i32::from(store));
    emit_state_load(body, HOST_ADDRESS_OFFSET);
    if store {
        emit_legacy_ar_load(body, instruction.t);
    } else {
        emit_i32_const(body, 0);
    }
    body.push(match class {
        HostMemoryClass::Mmio(_) => 0x10,
        HostMemoryClass::Flash | HostMemoryClass::Psram => 0x10,
    });
    body.push(match class {
        HostMemoryClass::Mmio(_) => 0,
        HostMemoryClass::Flash | HostMemoryClass::Psram => 1,
    });
    body.extend_from_slice(&[0x37, 0x03, 0x00]);

    if !store {
        emit_i32_const(body, i32::from(instruction.t) * 4);
        emit_i64_state_load(body, HOST_RESULT_OFFSET);
        body.push(0xa7);
        emit_i32_store(body);
    }
    match class {
        HostMemoryClass::Mmio(tier) => {
            emit_mmio_cost(body, pc, instruction.op, tier, store, config)?;
        }
        HostMemoryClass::Flash | HostMemoryClass::Psram => {
            emit_cycle_charge(body, 1);
            emit_dynamic_host_cycles(body);
        }
    }
    Ok(())
}

fn emit_mmio_cost(
    body: &mut Vec<u8>,
    pc: u32,
    op: Op,
    tier: MmioTier,
    store: bool,
    config: ChipConfig,
) -> Result<(), CompileError> {
    if !store {
        let cycles = priced_cycles(config, pc, op, Operation::MmioRead { tier })?;
        emit_cycle_charge(body, cycles);
        return Ok(());
    }
    let enqueue = priced_cycles(
        config,
        pc,
        op,
        Operation::MmioWrite {
            tier,
            buffer_has_room: true,
        },
    )?;
    let drain = priced_cycles(
        config,
        pc,
        op,
        Operation::MmioWrite {
            tier,
            buffer_has_room: false,
        },
    )?;
    emit_state_load(body, POSTED_WRITES_OFFSET);
    emit_i32_const(body, 8);
    body.push(0x49);
    body.extend_from_slice(&[0x04, 0x40]);
    emit_cycle_charge(body, enqueue);
    body.push(0x05);
    emit_cycle_charge(body, drain);
    body.push(0x0b);
    emit_i32_const(body, POSTED_WRITES_OFFSET as i32);
    emit_state_load(body, POSTED_WRITES_OFFSET);
    emit_i32_const(body, 1);
    body.push(0x6a);
    emit_i32_store(body);
    Ok(())
}

fn validate_external_config(
    config: ChipConfig,
    pc: u32,
    op: Op,
    class: HostMemoryClass,
) -> Result<(), CompileError> {
    match class {
        HostMemoryClass::Mmio(tier) => {
            let _cycles = priced_cycles(config, pc, op, Operation::MmioRead { tier })?;
        }
        HostMemoryClass::Flash | HostMemoryClass::Psram => {
            let _cycles = priced_cycles(config, pc, op, Operation::HotCacheHit)?;
        }
    }
    Ok(())
}

fn priced_cycles(
    config: ChipConfig,
    pc: u32,
    op: Op,
    operation: Operation,
) -> Result<u64, CompileError> {
    match price_operation(config, CoreId::Core0, operation) {
        Ok((component, _mutation)) => component
            .cycles()
            .ok_or(CompileError::IntervalCost { pc, op }),
        Err(refusal) if refusal.configuration.is_some() => {
            Err(CompileError::UnpricedConfiguration { config })
        }
        Err(_refusal) => Err(CompileError::IntervalCost { pc, op }),
    }
}

fn emit_dynamic_host_cycles(body: &mut Vec<u8>) {
    emit_i32_const(body, CYCLE_OFFSET as i32);
    emit_i32_const(body, CYCLE_OFFSET as i32);
    body.extend_from_slice(&[0x29, 0x03, 0x00]);
    emit_i64_state_load(body, HOST_RESULT_OFFSET);
    body.push(0x42);
    push_sleb(body, 32);
    body.push(0x88);
    body.push(0x7c);
    body.extend_from_slice(&[0x37, 0x03, 0x00]);
}

fn emit_i64_state_load(body: &mut Vec<u8>, offset: usize) {
    emit_i32_const(body, offset as i32);
    body.extend_from_slice(&[0x29, 0x03, 0x00]);
}

const fn mmio_tier_code(tier: MmioTier) -> i32 {
    match tier {
        MmioTier::Fast => 0,
        MmioTier::Apb => 1,
        MmioTier::Nrx => 2,
        MmioTier::Rtc => 3,
        MmioTier::Efuse => 4,
    }
}

fn emit_sram_address(body: &mut Vec<u8>, instruction: &Insn, sram_base: u32) {
    emit_legacy_ar_load(body, instruction.s);
    emit_i32_const(body, instruction.imm);
    body.push(0x6a);
    emit_i32_const(body, sram_base as i32);
    body.push(0x6b);
    emit_i32_const(body, SRAM_IMAGE_OFFSET as i32);
    body.push(0x6a);
}

fn sram_costs(instructions: &[(u32, Insn)]) -> Result<Vec<u64>, CompileError> {
    let mut previous_load = None;
    instructions
        .iter()
        .map(|(pc, instruction)| {
            if !is_sram_instruction(instruction.op) {
                return Err(CompileError::UnsupportedInstruction {
                    pc: *pc,
                    op: instruction.op,
                });
            }
            let dependency = previous_load
                .is_some_and(|register| read_registers(instruction) & (1u16 << register) != 0);
            previous_load = load_destination(instruction);
            let issue = xtensa_lx7::measured::compiled_internal_cost(instruction)
                .or_else(|| {
                    price_operation(
                        backend_api::ChipConfig::RECEIPT_SCOPE,
                        CoreId::Core0,
                        Operation::Instruction(InstructionCost::Issue),
                    )
                    .ok()
                    .map(|(component, _)| component)
                })
                .and_then(|component| component.cycles())
                .ok_or(CompileError::UnsupportedInstruction {
                    pc: *pc,
                    op: instruction.op,
                })?;
            let load_use = if dependency {
                price_operation(
                    backend_api::ChipConfig::RECEIPT_SCOPE,
                    CoreId::Core0,
                    Operation::Instruction(InstructionCost::LoadUse),
                )
                .ok()
                .and_then(|(component, _)| component.cycles())
                .ok_or(CompileError::UnsupportedInstruction {
                    pc: *pc,
                    op: instruction.op,
                })?
            } else {
                0
            };
            issue
                .checked_add(load_use)
                .ok_or(CompileError::UnsupportedInstruction {
                    pc: *pc,
                    op: instruction.op,
                })
        })
        .collect()
}

fn sram_ledger(instructions: &[(u32, Insn)]) -> Result<Vec<u8>, CompileError> {
    let mut engine = TransactionEngine::default();
    let mut previous_load = None;
    for (pc, instruction) in instructions {
        let mut operations = vec![Operation::Instruction(InstructionCost::Issue)];
        if is_memory(instruction.op) {
            operations.push(Operation::IndependentSramAccess);
        }
        if previous_load
            .is_some_and(|register| read_registers(instruction) & (1u16 << register) != 0)
        {
            operations.push(Operation::Instruction(InstructionCost::LoadUse));
        }
        previous_load = load_destination(instruction);
        let mut components = Vec::new();
        let mut mutations = Vec::new();
        for operation in operations {
            let (component, mutation) = price_operation(
                backend_api::ChipConfig::RECEIPT_SCOPE,
                CoreId::Core0,
                operation,
            )
            .map_err(|_| CompileError::UnsupportedInstruction {
                pc: *pc,
                op: instruction.op,
            })?;
            components.push(component);
            mutations.extend(mutation);
        }
        engine
            .execute_priced(
                CoreId::Core0,
                *pc,
                ExecutionOutcome::Committed,
                components,
                mutations,
            )
            .map_err(|_| CompileError::UnsupportedInstruction {
                pc: *pc,
                op: instruction.op,
            })?;
    }
    engine
        .run_trace(&[])
        .map(|report| report.canonical_ledger)
        .map_err(|_| CompileError::UnsupportedInstruction {
            pc: instructions[0].0,
            op: instructions[0].1.op,
        })
}

fn is_sram_instruction(op: Op) -> bool {
    matches!(
        op,
        Op::L8ui
            | Op::L16ui
            | Op::L16si
            | Op::L32i
            | Op::L32iN
            | Op::S8i
            | Op::S16i
            | Op::S32i
            | Op::S32iN
            | Op::Movi
            | Op::MoviN
            | Op::Add
            | Op::Sub
            | Op::And
            | Op::Or
            | Op::Xor
            | Op::Saltu
            | Op::Memw
            | Op::Nop
            | Op::NopN
    )
}

fn is_memory(op: Op) -> bool {
    matches!(
        op,
        Op::L8ui
            | Op::L16ui
            | Op::L16si
            | Op::L32i
            | Op::L32iN
            | Op::S8i
            | Op::S16i
            | Op::S32i
            | Op::S32iN
    )
}

fn is_store(op: Op) -> bool {
    matches!(op, Op::S8i | Op::S16i | Op::S32i | Op::S32iN)
}

fn load_destination(instruction: &Insn) -> Option<u8> {
    matches!(
        instruction.op,
        Op::L8ui | Op::L16ui | Op::L16si | Op::L32i | Op::L32iN
    )
    .then_some(instruction.t)
}

fn read_registers(instruction: &Insn) -> u16 {
    let bit = |register: u8| 1u16 << register;
    match instruction.op {
        Op::L8ui | Op::L16ui | Op::L16si | Op::L32i | Op::L32iN => bit(instruction.s),
        Op::S8i | Op::S16i | Op::S32i | Op::S32iN => bit(instruction.s) | bit(instruction.t),
        Op::Movi | Op::MoviN | Op::Memw | Op::Nop | Op::NopN => 0,
        _ => bit(instruction.s) | bit(instruction.t),
    }
}

fn function_prefix() -> Vec<u8> {
    vec![0x02, 0x40, 0x03, 0x40]
}

fn emit_current_pc_guard(body: &mut Vec<u8>, pc: u32) {
    emit_state_load(body, PC_OFFSET);
    emit_i32_const(body, pc as i32);
    body.push(0x46);
    body.extend_from_slice(&[0x04, 0x40]);
}

fn emit_continue(body: &mut Vec<u8>) {
    body.extend_from_slice(&[0x0c, 0x01, 0x0b]);
}

fn finish_function(body: &mut Vec<u8>) {
    body.extend_from_slice(&[0x0c, 0x01, 0x0b, 0x0b, 0x0b]);
}

fn emit_window_ar_load(body: &mut Vec<u8>, register: u8) {
    emit_window_ar_addr(body, register);
    body.extend_from_slice(&[0x28, 0x02, 0x00]);
}

fn emit_window_ar_addr(body: &mut Vec<u8>, register: u8) {
    emit_state_load(body, WINDOWBASE_OFFSET);
    emit_i32_const(body, 4);
    body.push(0x6c);
    emit_i32_const(body, i32::from(register));
    body.push(0x6a);
    emit_i32_const(body, 63);
    body.push(0x71);
    emit_i32_const(body, 4);
    body.push(0x6c);
    emit_i32_const(body, PHYSICAL_AR_OFFSET as i32);
    body.push(0x6a);
}

fn emit_legacy_ar_load(body: &mut Vec<u8>, register: u8) {
    emit_i32_const(body, i32::from(register) * 4);
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

fn emit_i32_store(body: &mut Vec<u8>) {
    body.extend_from_slice(&[0x36, 0x02, 0x00]);
}

fn emit_cycle_charge(body: &mut Vec<u8>, cycles: u64) {
    emit_i32_const(body, CYCLE_OFFSET as i32);
    emit_i32_const(body, CYCLE_OFFSET as i32);
    body.extend_from_slice(&[0x29, 0x03, 0x00]);
    body.push(0x42);
    push_sleb(body, cycles as i64);
    body.push(0x7c);
    body.extend_from_slice(&[0x37, 0x03, 0x00]);
}

fn emit_i32_const(body: &mut Vec<u8>, value: i32) {
    body.push(0x41);
    push_sleb(body, i64::from(value));
}

fn window_initial_state(initial: &WindowState) -> Vec<u8> {
    let mut state = vec![0; TEMP2_OFFSET + 4];
    for (index, value) in initial.ar.iter().copied().enumerate() {
        store_u32(&mut state, PHYSICAL_AR_OFFSET + index * 4, value);
    }
    store_u32(&mut state, PC_OFFSET, initial.pc);
    state[CYCLE_OFFSET..CYCLE_OFFSET + 8].copy_from_slice(&initial.cycles.to_le_bytes());
    store_u32(&mut state, WINDOWBASE_OFFSET, initial.windowbase);
    store_u32(&mut state, WINDOWSTART_OFFSET, initial.windowstart);
    store_u32(&mut state, PS_OFFSET, initial.ps);
    state
}

fn store_u32(bytes: &mut [u8], offset: usize, value: u32) {
    bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

fn wasm_module(body: Vec<u8>, state: &[u8]) -> Vec<u8> {
    let mut module = b"\0asm\x01\0\0\0".to_vec();
    append_section(&mut module, 1, &[1, 0x60, 0, 0]);
    append_section(&mut module, 3, &[1, 0]);
    let pages = state.len().div_ceil(65_536).max(1);
    let mut memory = vec![1, 0];
    push_uleb(&mut memory, pages);
    append_section(&mut module, 5, &memory);
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

fn wasm_module_with_host_imports(body: Vec<u8>, state: &[u8]) -> Vec<u8> {
    let mut module = b"\0asm\x01\0\0\0".to_vec();
    let types = [2, 0x60, 4, 0x7f, 0x7f, 0x7f, 0x7f, 1, 0x7e, 0x60, 0, 0];
    append_section(&mut module, 1, &types);
    let mut imports = Vec::new();
    push_uleb(&mut imports, 2);
    for name in ["mmio", "cache_access"] {
        append_name(&mut imports, "env");
        append_name(&mut imports, name);
        imports.extend_from_slice(&[0, 0]);
    }
    append_section(&mut module, 2, &imports);
    append_section(&mut module, 3, &[1, 1]);
    let pages = state.len().div_ceil(65_536).max(1);
    let mut memory = vec![1, 0];
    push_uleb(&mut memory, pages);
    append_section(&mut module, 5, &memory);
    let mut exports = Vec::new();
    push_uleb(&mut exports, 2);
    append_name(&mut exports, "memory");
    exports.extend_from_slice(&[2, 0]);
    append_name(&mut exports, "run");
    exports.extend_from_slice(&[0, 2]);
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
