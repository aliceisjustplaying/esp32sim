use backend_api::{Backend, CoreId};
use esp32s3::{machine, Esp32Backend, Machine, MeasuredMachine, MeasuredStep};

const KERNEL: &[u8] = include_bytes!("fixtures/tinydraw-sram-kernel.bin");
const KERNEL_START: u32 = 0x4038_645b;
const KERNEL_INSTRUCTIONS: usize = 7;
const MEASURED_CORE: CoreId = CoreId::Core0;
const LEDGER: &str = include_str!("../../tests/correlation/tinydraw-sram-kernel-ledger.json");

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

fn run_kernel() -> Vec<u8> {
    let mut machine = machine([0; 6]);
    receipt_config(&mut machine);
    machine
        .bus
        .load_bytes(KERNEL_START, KERNEL)
        .expect("committed TinyDraw SRAM kernel fixture maps into IRAM");
    machine
        .bus
        .load_bytes(0x3fc8_9004, &0x3fc8_9100u32.to_le_bytes())
        .expect("kernel input pointer maps into SRAM");
    machine
        .bus
        .load_bytes(0x3fc8_92c4, &0x1234_5678u32.to_le_bytes())
        .expect("kernel input value maps into SRAM");
    machine.cores[0].pc = KERNEL_START;
    machine.cores[0].set_ar(2, 0x3fc8_9000);
    machine.cores[0].set_ar(3, 7);

    let mut backend = Esp32Backend::default();
    for _ in 0..KERNEL_INSTRUCTIONS {
        assert_eq!(
            machine.step_measured(&mut backend, MEASURED_CORE),
            Ok(MeasuredStep::Instruction)
        );
    }
    let report = backend
        .run_trace(&[])
        .expect("completed kernel ledger has a scalar total");
    assert!(report
        .ledger
        .iter()
        .all(|entry| entry.core == MEASURED_CORE));
    report.canonical_ledger
}

#[test]
fn tinydraw_sram_kernel_has_deterministic_receipt_priced_ledger() {
    let first = run_kernel();
    let second = run_kernel();
    assert_eq!(first, second);
    let hex = first
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    assert!(
        LEDGER.contains(&format!("\"canonical_ledger_hex\": \"{hex}\"")),
        "committed ledger differs: {hex}"
    );
}
