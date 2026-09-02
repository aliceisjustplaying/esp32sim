use backend_api::{Backend, CoreId};
use esp32s3::backend::{ExceptionEntryClass, UnpricedTimingClass};
use esp32s3::{machine, Esp32Backend, Machine, MeasuredMachine, MeasuredStep, MeasuredStepError};
use xtensa_lx7::state::ps;

const WINDOW_VECTORS: &[u8] = include_bytes!("fixtures/idf61-window-vectors.bin");
const CALLER: u32 = 0x4038_9000;
const RECURSE: u32 = 0x4038_9010;
const CALLX8_A2: [u8; 3] = [0xe0, 0x02, 0x00];
const RECURSION_BODY: [u8; 10] = [
    0x36, 0x21, 0x00, // entry a1, 16
    0xad, 0x02, // mov.n a10, a2
    0xe0, 0x02, 0x00, // callx8 a2
    0x1d, 0xf0, // retw.n
];
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

fn callx8_recursion_machine() -> Machine {
    let mut machine = machine([0; 6]);
    receipt_config(&mut machine);
    machine
        .bus
        .load_bytes(0x4037_4000, WINDOW_VECTORS)
        .expect("committed IDF 6.1 window vector fixture maps into IRAM");
    machine
        .bus
        .load_bytes(CALLER, &CALLX8_A2)
        .expect("callx8 caller maps into IRAM");
    machine
        .bus
        .load_bytes(RECURSE, &RECURSION_BODY)
        .expect("recursive callx8 body maps into IRAM");
    machine.cores[0].pc = CALLER;
    machine.cores[0].vecbase = 0x4037_4000;
    machine.cores[0].ps = ps::WOE;
    machine.cores[0].set_ar(1, 0x3fc8_b000);
    machine.cores[0].set_ar(2, RECURSE);
    machine.cores[0].set_ar(10, RECURSE);
    machine
}

/// The receipt target is 35 cycles for the `_WindowOverflow8` and
/// `_WindowUnderflow8` pair. The unknowns are exception-entry delay and the
/// costs of `rfwo` and `rfwu`.
#[test]
#[ignore = "R8 residual unavailable: l32r blocks exact E/R; handler ledger 18; H1"]
fn idf61_callx8_window_overflow_underflow_pair() {
    let mut machine = callx8_recursion_machine();
    let mut backend = Esp32Backend::default();
    for _ in 0..32 {
        match machine.step_measured(&mut backend, CORRELATION_CORE) {
            Ok(MeasuredStep::Instruction) => {}
            Err(MeasuredStepError::Unpriced(UnpricedTimingClass::ExceptionEntry(
                ExceptionEntryClass::WindowOverflow8,
            ))) => {
                assert_eq!(machine.cores[0].pc, 0x4037_4080);
                assert_eq!(backend.engine().ledger().len(), 20);
                return;
            }
            other => panic!("callx8 recursion stopped unexpectedly: {other:?}"),
        }
    }
    panic!("callx8 recursion did not cross the register-file knee");
}
