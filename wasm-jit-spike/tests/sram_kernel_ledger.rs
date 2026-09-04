use std::process::Command;

use backend_api::{Backend, CoreId};
use esp32s3::{Esp32Backend, MeasuredMachine, MeasuredStep};
use wasm_jit_spike::{emit_sram, REGISTER_COUNT};

const KERNEL: &[u8] = include_bytes!("../../esp32s3/tests/fixtures/tinydraw-sram-kernel.bin");
const KERNEL_START: u32 = 0x4038_645b;
const KERNEL_INSTRUCTIONS: usize = 7;
const KERNEL_BYTES: usize = 19;
const SRAM_BASE: u32 = 0x3fc8_9000;
const SRAM_LEN: usize = 0x400;

#[test]
fn sram_kernel_jit_ledger_is_byte_identical_to_measured_interpreter() {
    let (expected_ledger, expected_cycles) = measured_interpreter_result();
    let mut registers = [0; REGISTER_COUNT];
    registers[2] = SRAM_BASE;
    registers[3] = 7;
    let mut sram = vec![0; SRAM_LEN];
    sram[4..8].copy_from_slice(&0x3fc8_9100u32.to_le_bytes());
    sram[0x2c4..0x2c8].copy_from_slice(&0x1234_5678u32.to_le_bytes());

    let module = emit_sram(
        KERNEL_START,
        &KERNEL[..KERNEL_BYTES],
        registers,
        SRAM_BASE,
        &sram,
    )
    .expect("the committed SRAM kernel emits");
    let runtime_cycles = execute_node(&module.bytes);
    assert_eq!(runtime_cycles, expected_cycles);
    assert_eq!(module.cycle_cost, expected_cycles);
    assert_eq!(module.canonical_ledger, expected_ledger);
}

fn measured_interpreter_result() -> (Vec<u8>, u64) {
    let mut machine = esp32s3::machine([0; 6]);
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
    machine.cores[0].pc = KERNEL_START;
    machine.cores[0].set_ar(2, SRAM_BASE);
    machine.cores[0].set_ar(3, 7);
    let mut backend = Esp32Backend::default();
    for _ in 0..KERNEL_INSTRUCTIONS {
        assert_eq!(
            machine.step_measured(&mut backend, CoreId::Core0),
            Ok(MeasuredStep::Instruction)
        );
    }
    let cycles = backend.engine().state().cores[0].cycle;
    let ledger = backend
        .run_trace(&[])
        .expect("scalar ledger")
        .canonical_ledger;
    (ledger, cycles)
}

fn receipt_config(machine: &mut esp32s3::Machine) {
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

fn execute_node(module: &[u8]) -> u64 {
    const SCRIPT: &str = r#"
const fs = require('fs');
WebAssembly.instantiate(fs.readFileSync(process.argv[1])).then(({instance}) => {
  instance.exports.run();
  const view = new DataView(instance.exports.memory.buffer);
  process.stdout.write(view.getBigUint64(72, true).toString());
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
        .output()
        .expect("run wasm");
    std::fs::remove_file(path).expect("remove wasm");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout)
        .expect("wasm cycle output is UTF-8")
        .parse()
        .expect("wasm cycle output is an integer")
}
