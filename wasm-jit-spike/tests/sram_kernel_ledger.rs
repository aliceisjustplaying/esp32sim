use std::process::Command;

use backend_api::{Backend, CoreId};
use esp32s3::{Esp32Backend, Machine, MeasuredStep};
use wasm_jit_spike::{emit_sram, REGISTER_COUNT, SRAM_IMAGE_OFFSET};

const KERNEL: &[u8] = include_bytes!("../../esp32s3/tests/fixtures/tinydraw-sram-kernel.bin");
const KERNEL_START: u32 = 0x4038_645b;
const KERNEL_INSTRUCTIONS: usize = 7;
const SRAM_BASE: u32 = 0x3fc8_9000;
const SRAM_LEN: usize = 0x400;

#[test]
fn sram_kernel_jit_ledger_is_byte_identical_to_measured_interpreter() {
    let expected = measured_interpreter_ledger();
    let mut registers = [0; REGISTER_COUNT];
    registers[2] = SRAM_BASE;
    registers[3] = 7;
    let mut sram = vec![0; SRAM_LEN];
    sram[4..8].copy_from_slice(&0x3fc8_9100u32.to_le_bytes());
    sram[0x2c4..0x2c8].copy_from_slice(&0x1234_5678u32.to_le_bytes());

    let module = emit_sram(KERNEL_START, KERNEL, registers, SRAM_BASE, &sram)
        .expect("the committed SRAM kernel emits");
    execute_node(&module.bytes);
    assert_eq!(module.canonical_ledger, expected);
}

fn measured_interpreter_ledger() -> Vec<u8> {
    let mut machine = Machine::new([0; 6]);
    receipt_config(&mut machine);
    machine
        .bus
        .load_bytes(KERNEL_START, KERNEL)
        .expect("load kernel");
    machine
        .bus
        .load_bytes(SRAM_BASE + 4, &0x3fc8_9100u32.to_le_bytes())
        .expect("load pointer");
    machine
        .bus
        .load_bytes(SRAM_BASE + 0x2c4, &0x1234_5678u32.to_le_bytes())
        .expect("load value");
    machine.cpu.pc = KERNEL_START;
    machine.cpu.set_ar(2, SRAM_BASE);
    machine.cpu.set_ar(3, 7);
    let mut backend = Esp32Backend::default();
    for _ in 0..KERNEL_INSTRUCTIONS {
        assert_eq!(
            machine.step_measured(&mut backend, CoreId::Core0),
            Ok(MeasuredStep::Instruction)
        );
    }
    backend
        .run_trace(&[])
        .expect("scalar ledger")
        .canonical_ledger
}

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

fn execute_node(module: &[u8]) {
    const SCRIPT: &str = r#"
const fs = require('fs');
WebAssembly.instantiate(fs.readFileSync(process.argv[1])).then(({instance}) => {
  instance.exports.run();
  const view = new DataView(instance.exports.memory.buffer);
  if (view.getUint32(Number(process.argv[2]), true) === 0) process.exit(2);
}).catch(error => { console.error(error); process.exit(1); });
"#;
    let path = std::path::Path::new("/tmp").join(format!(
        "esp32sim-wasm-jit-kernel-{}.wasm",
        std::process::id()
    ));
    std::fs::write(&path, module).expect("write wasm");
    let output = Command::new("node")
        .args(["-e", SCRIPT])
        .arg(&path)
        .arg((SRAM_IMAGE_OFFSET + 0x100).to_string())
        .output()
        .expect("run wasm");
    std::fs::remove_file(path).expect("remove wasm");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}
