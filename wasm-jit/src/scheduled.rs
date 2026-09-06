//! Bounded two-core scheduling experiment, not a general firmware execution path.
//! The fixture has direct loops, read-only shared SRAM and one synthetic device edge.
//! Its only unpriced instruction is an optional terminal store used to test refusal.
use super::*;
use esp32s3::timing::LedgerEntry;

pub const PCS: [u32; 2] = [0x4038_1000, 0x4038_2000];
pub const SRAM_BASE: u32 = 0x3fc8_9000;
pub const SRAM_LEN: usize = 64;
pub const NOW: usize = 256;
pub const FLAG: usize = 264;
pub const EVENTS: usize = 268;
pub const DEADLINE: usize = 272;
pub const TRACE_COUNT: usize = 280;
pub const STOPPED: usize = 284;
pub const TRACE_BASE: usize = 8192;
pub const TRACE_STRIDE: usize = 96;
pub const TRACE_CAPACITY: usize = 4096;
pub const MEMORY_PAGES: usize = 8;

pub fn programs(refusal: bool) -> [Vec<u8>; 2] {
    if refusal {
        // s32i a4, a2, 0; l32i a4, a2, 0. The store has effects but no price.
        return [vec![0x42, 0x62, 0], vec![0x42, 0x22, 0]];
    }
    let mut programs = [
        vec![0x42, 0x22, 0, 0x30, 0x54, 0xc0, 0xc0, 0x20, 0,
             0x62, 0x22, 1, 0xc0, 0x20, 0],
        vec![0x72, 0x22, 1, 0xc0, 0x20, 0, 0xc0, 0x20, 0,
             0x82, 0x22, 0, 0xc0, 0x20, 0],
    ];
    for bytes in &mut programs {
        let offset = -(bytes.len() as i32) - 4;
        let jump = (((offset as u32) & 0x3ffff) << 6) | 6;
        bytes.extend_from_slice(&jump.to_le_bytes()[..3]);
    }
    programs
}

#[derive(Clone, Copy, Default)]
pub struct Mutations {
    pub reverse_ties: bool,
    pub late_deadline: bool,
    pub omit_load_use: bool,
}

pub struct FixtureModule {
    pub bytes: Vec<u8>,
    /// A compiler cost plan, independently checked against Machine's runtime ledger.
    pub plans: [Vec<LedgerEntry>; 2],
}

pub fn compile(refusal: bool, mutations: Mutations) -> Result<FixtureModule, String> {
    let code = programs(refusal);
    let decoded = [decode_block(PCS[0], &code[0]), decode_block(PCS[1], &code[1])];
    let [a, b] = decoded;
    let decoded = [a.map_err(|e| e.to_string())?, b.map_err(|e| e.to_string())?];
    let mut plans: [Vec<LedgerEntry>; 2] = Default::default();
    for core in 0..2 {
        let mut model = Esp32S3SramCostModel::new();
        model.lifecycle(&LifecycleFacts { kind: LifecycleKind::Attach, chip: "esp32s3", cores: 2, cpu_hz: 240_000_000 })?;
        for d in &decoded[core] {
            let i = d.instruction;
            let mut accesses = vec![MemoryAccess { kind: MemoryAccessKind::Fetch, address: d.pc, width: 4, value: u32::from_le_bytes(d.bytes), fault: None }];
            if matches!(i.op, Op::L32i | Op::S32i) {
                accesses.push(MemoryAccess { kind: if i.op == Op::S32i { MemoryAccessKind::Write } else { MemoryAccessKind::Read }, address: SRAM_BASE + i.imm as u32, width: 4, value: 0, fault: None });
            }
            let result = model.cycles(&ExecutionFacts { core, outcome: StepOutcome { pc: d.pc, next_pc: next_pc(d), bytes: Some(d.bytes), length: i.len, kind: StepKind::Retired, control: None }, accesses: &accesses });
            if i.op == Op::S32i {
                assert!(refusal && result.as_ref().is_err_and(|e| e.contains("S32i")));
            } else { result?; }
        }
        plans[core] = model.ledger();
    }
    let mut body = Vec::new();
    // run(budget, tracing) -> sticky refusal core+1 or zero. Locals: core, trace address.
    load32(&mut body, STOPPED);
    body.extend_from_slice(&[0x04, 0x40]);
    load32(&mut body, STOPPED); body.push(0x0f); body.push(0x0b);
    body.extend_from_slice(&[0x02, 0x40, 0x03, 0x40]); // exit block, loop
    body.extend_from_slice(&[0x20, 0, 0x45, 0x0d, 1]); // budget==0 -> exit
    load64(&mut body, CYCLE_OFFSET); load64(&mut body, 128 + CYCLE_OFFSET);
    body.push(if mutations.reverse_ties { 0x54 } else { 0x58 }); // i64.lt_u / le_u
    body.extend_from_slice(&[0x04, 0x7f]); iconst(&mut body, 0); body.push(0x05); iconst(&mut body, 1); body.push(0x0b);
    body.extend_from_slice(&[0x21, 2]);
    body.extend_from_slice(&[0x20, 2, 0x45, 0x04, 0x40]);
    emit_core(&mut body, 0, &decoded[0], &plans[0], mutations)?;
    body.push(0x05);
    emit_core(&mut body, 1, &decoded[1], &plans[1], mutations)?;
    body.push(0x0b);
    // Match Machine's settle after every successful instruction, even at budget end.
    iconst(&mut body, NOW as i32);
    load64(&mut body, CYCLE_OFFSET); load64(&mut body, 128 + CYCLE_OFFSET);
    load64(&mut body, CYCLE_OFFSET); load64(&mut body, 128 + CYCLE_OFFSET);
    body.extend_from_slice(&[0x58, 0x1b, 0x37, 3, 0]); // min, i64.store
    load64(&mut body, NOW); load64(&mut body, DEADLINE);
    body.push(if mutations.late_deadline { 0x56 } else { 0x5a }); // le / ge (late mutant replaced below)
    if mutations.late_deadline {
        // Deliberately deliver only once NOW is strictly greater than DEADLINE.
        body.pop(); body.push(0x56); // i64.gt_u
    }
    body.extend_from_slice(&[0x04, 0x40]); store_const(&mut body, FLAG, 1); iconst(&mut body, 288); load64(&mut body, DEADLINE); body.extend_from_slice(&[0x37, 3, 0]); body.push(0x0b);
    increment(&mut body, EVENTS, 1);
    body.extend_from_slice(&[0x20, 0]); iconst(&mut body, 1); body.extend_from_slice(&[0x6b, 0x21, 0, 0x0c, 0, 0x0b, 0x0b]);
    iconst(&mut body, 0); body.push(0x0b);
    Ok(FixtureModule { bytes: module(body), plans })
}

fn next_pc(d: &DecodedInstruction) -> u32 {
    if d.instruction.op == Op::J { d.instruction.imm as u32 } else { d.pc + u32::from(d.instruction.len) }
}

fn emit_core(body: &mut Vec<u8>, core: usize, steps: &[DecodedInstruction], prices: &[LedgerEntry], mutations: Mutations) -> Result<(), String> {
    let state = core * 128;
    let layout = MemoryLayout { registers: state, pc: state + PC_OFFSET, cycles: state + CYCLE_OFFSET, sram_image: SRAM_IMAGE_OFFSET };
    // One matching dispatch arm exits this block. Unknown PCs trap rather than silently run.
    body.extend_from_slice(&[0x02, 0x40]);
    for d in steps {
        load32(body, layout.pc); iconst(body, d.pc as i32); body.extend_from_slice(&[0x46, 0x04, 0x40]);
        let refused = d.instruction.op == Op::S32i;
        let cost = if refused { 0 } else { prices.iter().find(|p| p.pc == d.pc).unwrap().cycles };
        let charged = if mutations.omit_load_use && cost == 2 { 1 } else { cost };
        // The only load base is a2, whose identity is guarded for this specialized fixture.
        if matches!(d.instruction.op, Op::L32i | Op::S32i) {
            emit_register_load(body, 2, layout); iconst(body, SRAM_BASE as i32); body.push(0x47); emit_trap_if_true(body);
        }
        match d.instruction.op {
            Op::J => {},
            Op::S32i => {
                emit_sram_address(body, &d.instruction, SRAM_BASE, layout);
                emit_register_load(body, d.instruction.t, layout);
                emit_i32_store(body);
            },
            _ => emit_sram_instruction(body, d.pc, &d.instruction, SRAM_BASE, SRAM_LEN, layout).map_err(|e| e.to_string())?,
        }
        store_const(body, layout.pc, next_pc(d) as i32);
        // Architectural CCOUNT still advances once on an unpriced retired instruction.
        increment(body, state + 80, if refused { 1 } else { charged } as i32);
        increment(body, state + 84, 1);
        emit_trace(body, core, d.pc, charged, state);
        if refused {
            store_const(body, STOPPED, (core + 1) as i32);
            iconst(body, (core + 1) as i32); body.push(0x0f);
        } else {
            emit_cycle_charge(body, u64::from(charged), layout);
        }
        body.extend_from_slice(&[0x0c, 1, 0x0b]);
    }
    body.extend_from_slice(&[0x00, 0x0b]);
    Ok(())
}

fn emit_trace(body: &mut Vec<u8>, core: usize, pc: u32, cost: u32, state: usize) {
    body.extend_from_slice(&[0x20, 1, 0x04, 0x40]);
    load32(body, TRACE_COUNT); iconst(body, TRACE_CAPACITY as i32); body.push(0x4f); emit_trap_if_true(body);
    load32(body, TRACE_COUNT); iconst(body, TRACE_STRIDE as i32); body.push(0x6c); iconst(body, TRACE_BASE as i32); body.extend_from_slice(&[0x6a, 0x21, 3]);
    body.extend_from_slice(&[0x20, 3]); load64(body, NOW); body.extend_from_slice(&[0x37, 3, 0]);
    for (offset, value) in [(8, core as i32), (12, pc as i32), (16, cost as i32)] {
        trace_addr(body, offset); iconst(body, value); emit_i32_store(body);
    }
    trace_addr(body, 20); load32(body, FLAG); emit_i32_store(body);
    for register in 0..16 {
        trace_addr(body, 24 + register * 4); load32(body, state + register * 4); emit_i32_store(body);
    }
    trace_addr(body, 88); load32(body, state + PC_OFFSET); emit_i32_store(body);
    trace_addr(body, 92); iconst(body, 0); emit_i32_store(body);
    increment(body, TRACE_COUNT, 1);
    body.push(0x0b);
}
fn trace_addr(body: &mut Vec<u8>, offset: usize) { body.extend_from_slice(&[0x20, 3]); iconst(body, offset as i32); body.push(0x6a); }
fn iconst(body: &mut Vec<u8>, value: i32) { emit_i32_const(body, value); }
fn load32(body: &mut Vec<u8>, offset: usize) { emit_state_load(body, offset); }
fn load64(body: &mut Vec<u8>, offset: usize) { iconst(body, offset as i32); body.extend_from_slice(&[0x29, 3, 0]); }
fn store_const(body: &mut Vec<u8>, offset: usize, value: i32) { emit_state_store_const(body, offset, value); }
fn increment(body: &mut Vec<u8>, offset: usize, value: i32) { iconst(body, offset as i32); load32(body, offset); iconst(body, value); body.push(0x6a); emit_i32_store(body); }
fn module(body: Vec<u8>) -> Vec<u8> {
    let mut module = b"\0asm\x01\0\0\0".to_vec();
    append_section(&mut module, 1, &[1, 0x60, 2, 0x7f, 0x7f, 1, 0x7f]);
    let mut imports = vec![1]; append_name(&mut imports, "env"); append_name(&mut imports, "memory"); imports.extend_from_slice(&[2, 0, MEMORY_PAGES as u8]);
    append_section(&mut module, 2, &imports); append_section(&mut module, 3, &[1, 0]);
    let mut exports = vec![1]; append_name(&mut exports, "run"); exports.extend_from_slice(&[0, 0]); append_section(&mut module, 7, &exports);
    let mut code = vec![1]; push_uleb(&mut code, body.len() + 3); code.extend_from_slice(&[1, 2, 0x7f]); code.extend_from_slice(&body); append_section(&mut module, 10, &code);
    module
}
