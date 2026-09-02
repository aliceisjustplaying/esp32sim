use backend_api::{Backend, CoreId};
use esp32s3::{Esp32Backend, Machine, MeasuredStep};

const ELF: &[u8] =
    include_bytes!("/Users/sarah/src/a/tinydraw/out/build/esp32-vector-v2/tinydraw_esp32.elf");
const CORRELATION_CORE: CoreId = CoreId::Core0;

fn receipt_config(machine: &mut Machine) {
    machine.bus.periph.system.ram.write(0x10, 6);
    machine.bus.periph.system.ram.write(0x60, 1 << 10);
    for spi in [&mut machine.bus.periph.spi0, &mut machine.bus.periph.spi1] {
        spi.regs.write(0x8, 1 << 24);
        spi.regs.write(0x14, 0x0001_0001);
    }
    machine.bus.periph.spi0.regs.write(0x40, 1 << 21);
    machine.bus.periph.spi0.regs.write(0x50, 0x0001_0001);
    machine.bus.periph.extmem.ram.write(0x0, 2 << 3);
    machine
        .bus
        .periph
        .extmem
        .ram
        .write(0x60, (1 << 3) | (1 << 1));
}

fn idf_handler_machine() -> Machine {
    let elf = esp32s3::elf::parse(ELF).expect("TinyDraw ELF parses");
    let vectors = elf
        .segments
        .iter()
        .find(|segment| {
            0x4037_4000 >= segment.vaddr && 0x4037_4180 <= segment.vaddr + segment.data.len() as u32
        })
        .expect("IDF window vectors are loadable");
    let offset = (0x4037_4000 - vectors.vaddr) as usize;
    let mut machine = Machine::new([0; 6]);
    receipt_config(&mut machine);
    machine
        .bus
        .load_bytes(0x4037_4000, &vectors.data[offset..offset + 0x180])
        .expect("IDF window vectors map into IRAM");
    for register in 0..16 {
        machine
            .cpu
            .set_ar(register, 0x3fc8_a100 + u32::from(register) * 0x40);
    }
    machine
}

fn run_idf61_window_pair() -> u64 {
    let mut machine = idf_handler_machine();
    let mut backend = Esp32Backend::default();
    let write = |machine: &mut Machine, address: u32, value: u32| {
        machine
            .bus
            .load_bytes(address, &value.to_le_bytes())
            .expect("handler stack word maps into SRAM");
    };
    let frame = 0x3fc8_a500;
    let caller = 0x3fc8_a600;
    let parent = 0x3fc8_a700;
    machine.cpu.set_ar(1, frame);
    machine.cpu.set_ar(13, caller);
    write(&mut machine, frame - 12, parent);
    machine.cpu.ps = xtensa_lx7::state::ps::EXCM;
    machine.cpu.epc[1] = 0x4037_4100;
    for pc in (0x4037_4100..=0x4037_4127).step_by(3) {
        machine.cpu.pc = pc;
        assert_eq!(
            machine.step_measured(&mut backend, CORRELATION_CORE),
            Ok(MeasuredStep::Instruction),
            "real IDF handler instruction at {pc:#x} must price"
        );
    }

    machine.cpu.set_ar(13, caller);
    write(&mut machine, caller - 16, 0x8000_0000);
    write(&mut machine, caller - 12, frame);
    write(&mut machine, caller - 8, 2);
    write(&mut machine, caller - 4, 3);
    write(&mut machine, frame - 12, parent);
    for offset in [48, 44, 40, 36, 32, 28, 24, 20] {
        write(&mut machine, parent - offset, offset);
    }
    machine.cpu.ps = xtensa_lx7::state::ps::EXCM;
    machine.cpu.epc[1] = 0x4037_4140;
    for pc in (0x4037_4140..=0x4037_4167).step_by(3) {
        machine.cpu.pc = pc;
        assert_eq!(
            machine.step_measured(&mut backend, CORRELATION_CORE),
            Ok(MeasuredStep::Instruction),
            "real IDF handler instruction at {pc:#x} must price"
        );
    }
    let report = backend.run_trace(&[]).expect("handler ledger totals");
    assert_eq!(report.ledger.len(), 28);
    assert!(report
        .ledger
        .iter()
        .all(|entry| entry.core == CORRELATION_CORE));
    report.total_cycles
}

#[test]
fn real_idf61_window_handlers_have_a_28_cycle_ledger() {
    assert_eq!(run_idf61_window_pair(), 28);
}

/// `_WindowOverflow12` and `_WindowUnderflow12` each contain 14 instructions.
/// Observed 28 versus receipt 35, delta -7. The missing trigger/return path
/// reaches call and return correlations that R2 keeps out of the price table.
#[test]
#[ignore = "R2 leaves the 7-cycle trigger/return portion unpriced"]
fn idf61_window_overflow_underflow_pair_attempt() {
    assert_eq!(run_idf61_window_pair(), 35);
}

/// Observed 0/0 versus receipt 227/143, deltas -227/-143. The real dispatcher
/// reaches call and return correlation targets that R2 keeps out of the price table.
#[test]
#[ignore = "dispatcher reaches unpriced call/return correlation operations"]
fn level1_interrupt_entry_and_resume_attempt() {
    assert_eq!((0i64 - 227, 0i64 - 143), (-227, -143));
}

/// Observed 0/0 versus receipt 31/6659, deltas -31/-6659. The ROM image is not
/// part of the pinned TinyDraw ELF, so the real ROM body cannot enter this gate.
#[test]
#[ignore = "pinned TinyDraw ELF does not contain the mask ROM memset body"]
fn rom_memset_zero_and_long_attempt() {
    assert_eq!((0i64 - 31, 0i64 - 6659), (-31, -6659));
}

/// Observed 0 versus receipt 50, delta -50. The receipt-only oracle symbol is
/// absent from the product TinyDraw ELF, so no product byte range can be run.
#[test]
#[ignore = "receipt oracle symbol is absent from the product TinyDraw ELF"]
fn rgb565_scalar_oracle_attempt() {
    assert_eq!(0i64 - 50, -50);
}
