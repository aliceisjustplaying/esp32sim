#[path = "../../test-support/jit_conformance.rs"]
mod support;

use std::collections::BTreeSet;
use xtensa_lx7::bus::{Bus, Fault};
use xtensa_lx7::{block, jit, Cpu};

const CORPUS: &str = include_str!("corpus/jit-interpreter-conformance.json");
const SRAM_BASE: u32 = 0x4000_0000;
const SRAM_SIZE: usize = 0x2000;
const DATA_BASE: u32 = SRAM_BASE + 0x800;
const RANDOM_BLOCKS: usize = 128;

struct TrackingRam {
    mem: Vec<u8>,
    touched: BTreeSet<u32>,
    version: u32,
}

impl TrackingRam {
    fn with_code(code: &[u8], seed: u32) -> Self {
        assert!(code.len() < 0x100, "conformance block exceeds code slot");
        let mut random = support::XorShift32::new(seed);
        let mut mem = vec![0; SRAM_SIZE];
        for byte in &mut mem {
            *byte = random.next_u32() as u8;
        }
        mem[..0x100].fill(0);
        mem[..code.len()].copy_from_slice(code);
        Self {
            mem,
            touched: BTreeSet::new(),
            version: seed,
        }
    }

    fn offset(&self, address: u32, size: usize) -> Result<usize, Fault> {
        let offset = address.wrapping_sub(SRAM_BASE) as usize;
        match offset.checked_add(size) {
            Some(end) if end <= self.mem.len() => Ok(offset),
            _ => Err(Fault::Unmapped),
        }
    }

    fn touch(&mut self, address: u32, size: usize) {
        self.touched
            .extend((0..size).map(|offset| address.wrapping_add(offset as u32)));
    }
}

impl Bus for TrackingRam {
    fn read8(&mut self, address: u32) -> Result<u8, Fault> {
        let offset = self.offset(address, 1)?;
        self.touch(address, 1);
        Ok(self.mem[offset])
    }

    fn read16(&mut self, address: u32) -> Result<u16, Fault> {
        let offset = self.offset(address, 2)?;
        self.touch(address, 2);
        Ok(u16::from_le_bytes([self.mem[offset], self.mem[offset + 1]]))
    }

    fn read32(&mut self, address: u32) -> Result<u32, Fault> {
        let offset = self.offset(address, 4)?;
        self.touch(address, 4);
        Ok(u32::from_le_bytes([
            self.mem[offset],
            self.mem[offset + 1],
            self.mem[offset + 2],
            self.mem[offset + 3],
        ]))
    }

    fn write8(&mut self, address: u32, value: u8) -> Result<(), Fault> {
        let offset = self.offset(address, 1)?;
        self.touch(address, 1);
        self.mem[offset] = value;
        self.version = self.version.wrapping_add(1);
        Ok(())
    }

    fn write16(&mut self, address: u32, value: u16) -> Result<(), Fault> {
        let offset = self.offset(address, 2)?;
        self.touch(address, 2);
        self.mem[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
        self.version = self.version.wrapping_add(1);
        Ok(())
    }

    fn write32(&mut self, address: u32, value: u32) -> Result<(), Fault> {
        let offset = self.offset(address, 4)?;
        self.touch(address, 4);
        self.mem[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
        self.version = self.version.wrapping_add(1);
        Ok(())
    }

    fn fetch(&mut self, pc: u32) -> Result<[u8; 4], Fault> {
        let offset = self.offset(pc, 1)?;
        let mut bytes = [0; 4];
        let available = (self.mem.len() - offset).min(bytes.len());
        bytes[..available].copy_from_slice(&self.mem[offset..offset + available]);
        Ok(bytes)
    }

    fn page_versions(&self) -> &[u32] {
        std::slice::from_ref(&self.version)
    }
}

fn seed_cpu(cpu: &mut Cpu, seed: u32, jit_enabled: bool) {
    let mut random = support::XorShift32::new(seed);
    for register in &mut cpu.ar {
        *register = random.next_u32();
    }
    cpu.pc = SRAM_BASE;
    cpu.windowbase = 0;
    cpu.windowstart = 1;
    cpu.ps = 0;
    cpu.sar = random.next_u32() & 31;
    cpu.lbeg = 0;
    cpu.lend = 0;
    cpu.lcount = 0;
    cpu.br = random.next_u32();
    cpu.scompare1 = random.next_u32();
    cpu.acclo = random.next_u32();
    cpu.acchi = random.next_u32();
    cpu.m.fill_with(|| random.next_u32());
    cpu.epc.fill_with(|| random.next_u32());
    cpu.eps.fill_with(|| random.next_u32());
    cpu.excsave.fill_with(|| random.next_u32());
    cpu.depc = random.next_u32();
    cpu.vecbase = SRAM_BASE;
    cpu.exccause = 0;
    cpu.excvaddr = 0;
    cpu.debugcause = 0;
    cpu.interrupt = 0;
    cpu.intenable = 0;
    cpu.ccount = 0;
    cpu.ccompare = [0; 3];
    cpu.cpenable = 1;
    cpu.prid = 0x51a7;
    cpu.threadptr = random.next_u32();
    cpu.misc.fill_with(|| random.next_u32());
    cpu.icount = 0;
    cpu.icountlevel = 0;
    cpu.ibreakenable = 0;
    cpu.ibreaka = [0; 2];
    cpu.dbreaka = [0; 2];
    cpu.dbreakc = [0; 2];
    cpu.memctl = 1;
    cpu.atomctl = 0;
    cpu.ddr = random.next_u32();
    cpu.fr.fill_with(|| random.next_u32());
    cpu.fcr = random.next_u32();
    cpu.fsr = random.next_u32();
    cpu.qr.fill_with(|| u128::from(random.next_u32()));
    cpu.accx.fill_with(|| random.next_u32());
    cpu.qacc_h.fill_with(|| random.next_u32());
    cpu.qacc_l.fill_with(|| random.next_u32());
    cpu.sar_byte = random.next_u32() & 31;
    cpu.fft_bit_width = random.next_u32();
    cpu.ua_state.fill_with(|| random.next_u32());
    cpu.gpio_out = random.next_u32();
    cpu.waiting = false;
    cpu.ext_level_lines = 0;
    cpu.insn_count = 0;
    cpu.boundary_bloom = 0;
    cpu.jit_trap = None;
    cpu.set_ar(1, DATA_BASE);
    cpu.blocks.flush();
    cpu.blocks.jit_enabled = jit_enabled;
}

fn assert_architectural_state(case: &str, interpreter: &Cpu, native: &Cpu) {
    assert_eq!(interpreter.pc, native.pc, "{case}: PC");
    assert_eq!(interpreter.ar, native.ar, "{case}: address registers");
    assert_eq!(
        [
            interpreter.windowbase,
            interpreter.windowstart,
            interpreter.ps,
            interpreter.sar,
            interpreter.lbeg,
            interpreter.lend,
            interpreter.lcount,
            interpreter.br,
            interpreter.scompare1,
            interpreter.acclo,
            interpreter.acchi,
            interpreter.depc,
            interpreter.vecbase,
            interpreter.exccause,
            interpreter.excvaddr,
            interpreter.debugcause,
            interpreter.interrupt,
            interpreter.intenable,
            interpreter.ccount,
            interpreter.cpenable,
            interpreter.prid,
            interpreter.threadptr,
            interpreter.icount,
            interpreter.icountlevel,
            interpreter.ibreakenable,
            interpreter.memctl,
            interpreter.atomctl,
            interpreter.ddr,
            interpreter.fcr,
            interpreter.fsr,
            interpreter.sar_byte,
            interpreter.fft_bit_width,
            interpreter.gpio_out,
            interpreter.ext_level_lines,
        ],
        [
            native.windowbase,
            native.windowstart,
            native.ps,
            native.sar,
            native.lbeg,
            native.lend,
            native.lcount,
            native.br,
            native.scompare1,
            native.acclo,
            native.acchi,
            native.depc,
            native.vecbase,
            native.exccause,
            native.excvaddr,
            native.debugcause,
            native.interrupt,
            native.intenable,
            native.ccount,
            native.cpenable,
            native.prid,
            native.threadptr,
            native.icount,
            native.icountlevel,
            native.ibreakenable,
            native.memctl,
            native.atomctl,
            native.ddr,
            native.fcr,
            native.fsr,
            native.sar_byte,
            native.fft_bit_width,
            native.gpio_out,
            native.ext_level_lines,
        ],
        "{case}: scalar registers"
    );
    assert_eq!(interpreter.m, native.m, "{case}: MAC registers");
    assert_eq!(interpreter.epc, native.epc, "{case}: EPC registers");
    assert_eq!(interpreter.eps, native.eps, "{case}: EPS registers");
    assert_eq!(
        interpreter.excsave, native.excsave,
        "{case}: EXCSAVE registers"
    );
    assert_eq!(
        interpreter.ccompare, native.ccompare,
        "{case}: CCOMPARE registers"
    );
    assert_eq!(interpreter.misc, native.misc, "{case}: MISC registers");
    assert_eq!(
        interpreter.ibreaka, native.ibreaka,
        "{case}: IBREAKA registers"
    );
    assert_eq!(
        interpreter.dbreaka, native.dbreaka,
        "{case}: DBREAKA registers"
    );
    assert_eq!(
        interpreter.dbreakc, native.dbreakc,
        "{case}: DBREAKC registers"
    );
    assert_eq!(
        interpreter.configid, native.configid,
        "{case}: CONFIGID registers"
    );
    assert_eq!(
        interpreter.fr, native.fr,
        "{case}: floating-point registers"
    );
    assert_eq!(interpreter.qr, native.qr, "{case}: PIE Q registers");
    assert_eq!(interpreter.accx, native.accx, "{case}: PIE ACCX registers");
    assert_eq!(
        interpreter.qacc_h, native.qacc_h,
        "{case}: PIE QACC high registers"
    );
    assert_eq!(
        interpreter.qacc_l, native.qacc_l,
        "{case}: PIE QACC low registers"
    );
    assert_eq!(
        interpreter.ua_state, native.ua_state,
        "{case}: unaligned-access state"
    );
    assert_eq!(interpreter.waiting, native.waiting, "{case}: WAITI state");
    assert_eq!(
        interpreter.insn_count, native.insn_count,
        "{case}: instruction count"
    );
}

fn run_case(
    name: &str,
    code: &[u8],
    instruction_count: usize,
    seed: u32,
    interpreter_cpu: &mut Cpu,
    native_cpu: &mut Cpu,
) {
    let mut interpreter_ram = TrackingRam::with_code(code, seed);
    let mut native_ram = TrackingRam::with_code(code, seed);
    seed_cpu(interpreter_cpu, seed ^ 0x1357_9bdf, false);
    seed_cpu(native_cpu, seed ^ 0x1357_9bdf, true);

    let compiled_before = native_cpu.blocks.compiled;
    let (interpreter_used, interpreter_trap) = block::run_block(
        interpreter_cpu,
        &mut interpreter_ram,
        instruction_count as u32,
    );
    let (native_used, native_trap) =
        block::run_block(native_cpu, &mut native_ram, instruction_count as u32);

    assert_eq!(interpreter_trap, native_trap, "{name}: trap");
    assert_eq!(interpreter_trap, None, "{name}: unexpected trap");
    assert_eq!(
        interpreter_used, instruction_count as u32,
        "{name}: interpreter count"
    );
    assert_eq!(
        native_used, instruction_count as u32,
        "{name}: native count"
    );
    assert!(
        native_cpu.blocks.compiled > compiled_before,
        "{name}: block did not enter the native JIT"
    );
    assert_architectural_state(name, interpreter_cpu, native_cpu);

    assert_eq!(
        interpreter_ram.touched, native_ram.touched,
        "{name}: touched addresses"
    );
    for address in &interpreter_ram.touched {
        let offset = address.wrapping_sub(SRAM_BASE) as usize;
        assert_eq!(
            interpreter_ram.mem[offset], native_ram.mem[offset],
            "{name}: touched memory at {address:#010x}"
        );
    }
    assert_eq!(interpreter_ram.mem, native_ram.mem, "{name}: SRAM image");
}

#[test]
fn committed_and_random_sram_blocks_match_interpreter_and_jit_state() {
    let corpus = support::parse_corpus(CORPUS);
    if !jit::AVAILABLE {
        eprintln!("native JIT unavailable on this host; committed corpus parsed");
        return;
    }

    let mut interpreter_cpu = Cpu::new(0);
    let mut native_cpu = Cpu::new(0);
    let mut instruction_pool = Vec::new();
    for (index, case) in corpus.iter().enumerate() {
        let code: Vec<u8> = case.instructions.iter().flatten().copied().collect();
        run_case(
            &format!("corpus/{}", case.name),
            &code,
            case.instructions.len(),
            0xc0de_0001u32.wrapping_add(index as u32),
            &mut interpreter_cpu,
            &mut native_cpu,
        );
        instruction_pool.extend(case.instructions.iter().cloned());
    }

    let mut random = support::XorShift32::new(0x5eed_1a77);
    for index in 0..RANDOM_BLOCKS {
        let instruction_count = 8 + random.index(17);
        let instructions: Vec<&[u8]> = (0..instruction_count)
            .map(|_| instruction_pool[random.index(instruction_pool.len())].as_slice())
            .collect();
        let code: Vec<u8> = instructions.into_iter().flatten().copied().collect();
        run_case(
            &format!("random/{index}"),
            &code,
            instruction_count,
            random.next_u32(),
            &mut interpreter_cpu,
            &mut native_cpu,
        );
    }
}
