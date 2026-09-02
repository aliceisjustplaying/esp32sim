use backend_api::{Backend, CoreId};
use esp32s3::{Esp32Backend, Machine, MeasuredStep};
use std::process::Command;

const ELF_PATH: &str = "/Users/sarah/src/a/tinydraw/out/build/esp32-vector-v2/tinydraw_esp32.elf";
const ELF_SHA256: &str = "7f598fd3580cf52078fb6aa04a5f6fe5179b0de9d89bb6468fdb06ed5e40e424";
const SYMBOL: &str = "_ZNK8tinydraw5esp3220Co5300PanelTransport16complete_time_usEm";
const KERNEL_OFFSET: u32 = 3;
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

fn run_kernel(elf_bytes: &[u8]) -> Vec<u8> {
    let elf = esp32s3::elf::parse(elf_bytes).expect("TinyDraw ELF parses");
    let symbol = *elf
        .by_name
        .get(SYMBOL)
        .expect("kernel source symbol exists");
    let start = symbol + KERNEL_OFFSET;
    let segment = elf
        .segments
        .iter()
        .find(|segment| {
            start >= segment.vaddr && start + 24 <= segment.vaddr + segment.data.len() as u32
        })
        .expect("kernel bytes are in a loadable SRAM segment");
    let offset = (start - segment.vaddr) as usize;

    let mut machine = Machine::new([0; 6]);
    receipt_config(&mut machine);
    machine
        .bus
        .load_bytes(start, &segment.data[offset..offset + 24])
        .expect("extracted kernel maps into SRAM");
    machine
        .bus
        .load_bytes(0x3fc8_9004, &0x3fc8_9100u32.to_le_bytes())
        .expect("kernel input pointer maps into SRAM");
    machine
        .bus
        .load_bytes(0x3fc8_92c4, &0x1234_5678u32.to_le_bytes())
        .expect("kernel input value maps into SRAM");
    machine.cpu.pc = start;
    machine.cpu.set_ar(2, 0x3fc8_9000);
    machine.cpu.set_ar(3, 7);

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
    let elf_bytes = std::fs::read(ELF_PATH).expect("pinned TinyDraw ELF is present");
    let output = Command::new("shasum")
        .args(["-a", "256", ELF_PATH])
        .output()
        .expect("shasum executes");
    assert!(output.status.success());
    assert_eq!(
        String::from_utf8_lossy(&output.stdout)
            .split_whitespace()
            .next(),
        Some(ELF_SHA256)
    );

    let first = run_kernel(&elf_bytes);
    let second = run_kernel(&elf_bytes);
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
