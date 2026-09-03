use backend_api::{Backend, CoreId, CostClass, InstructionCost};
use esp32s3::backend::{ExceptionReturnClass, UnpricedTimingClass};
use esp32s3::{Esp32Backend, Machine, MeasuredMachine, MeasuredStep, MeasuredStepError};
use std::path::PathBuf;
use xtensa_lx7::measured::PlanError;

const BUILD_ENVIRONMENT: &str = "TINYDRAW_VECTOR_V2_BUILD";

#[derive(Clone, Copy)]
struct Attempt {
    name: &'static str,
    start_pc: u32,
    stop_pc: u32,
    known_cycles: u64,
    ledger_entries: usize,
    stop: &'static str,
}

fn required_elf() -> Result<PathBuf, String> {
    std::env::var_os(BUILD_ENVIRONMENT)
        .map(PathBuf::from)
        .map(|build| build.join("tinydraw_esp32.elf"))
        .ok_or_else(|| format!("{BUILD_ENVIRONMENT} must name the TinyDraw product build"))
}

fn idf61_machine() -> Result<Machine, String> {
    let bytes = std::fs::read(required_elf()?).map_err(|error| error.to_string())?;
    let elf = esp32s3::elf::parse(&bytes)?;
    let mut machine = esp32s3::machine([0; 6]);
    for section in elf
        .sections
        .iter()
        .filter(|section| section.name == ".iram0.vectors" || section.name == ".iram0.text")
    {
        machine.bus.load_bytes(section.addr, &section.data)?;
    }
    machine.symbols.extend(elf.symbols);
    machine.cores[0].set_ar(1, 0x3fca_b000);
    Ok(machine)
}

fn classify(name: &str, error: MeasuredStepError) -> Result<&'static str, String> {
    match error {
        MeasuredStepError::Plan(PlanError::Timing(refusal))
            if refusal.class == CostClass::Instruction(InstructionCost::LiteralLoad) =>
        {
            Ok("l32r_interval")
        }
        MeasuredStepError::Unpriced(UnpricedTimingClass::ExceptionReturn(
            ExceptionReturnClass::Rfwo,
        )) => Ok("rfwo_zero_placeholder"),
        MeasuredStepError::Unpriced(UnpricedTimingClass::ExceptionReturn(
            ExceptionReturnClass::Rfwu,
        )) => Ok("rfwu_zero_placeholder"),
        other => Err(format!(
            "{name} unexpected measured derivation stop: {other:?}"
        )),
    }
}

fn run_attempt(
    name: &'static str,
    start_pc: u32,
    setup: impl FnOnce(&mut Machine),
) -> Result<Attempt, String> {
    let mut machine = idf61_machine()?;
    machine.cores[0].pc = start_pc;
    setup(&mut machine);
    let mut backend = Esp32Backend::default();
    for _ in 0..128 {
        match machine.step_measured(&mut backend, CoreId::Core0) {
            Ok(MeasuredStep::Instruction) => {}
            Ok(other) => return Err(format!("{name} stopped without a refusal: {other:?}")),
            Err(error) => {
                return Ok(Attempt {
                    name,
                    start_pc,
                    stop_pc: machine.cores[0].pc,
                    known_cycles: backend.engine().state().cores[0].cycle,
                    ledger_entries: backend.engine().ledger().len(),
                    stop: classify(name, error)?,
                });
            }
        }
    }
    Err(format!("{name} did not stop within 128 instructions"))
}

fn print_attempts(attempts: &[Attempt]) {
    let entries = attempts
        .iter()
        .map(|attempt| {
            format!(
                concat!(
                    "{{\"name\":\"{}\",\"start_pc\":\"{:#010x}\",",
                    "\"stop_pc\":\"{:#010x}\",\"known_cycles\":{},",
                    "\"ledger_entries\":{},\"stop\":\"{}\"}}"
                ),
                attempt.name,
                attempt.start_pc,
                attempt.stop_pc,
                attempt.known_cycles,
                attempt.ledger_entries,
                attempt.stop
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    println!("EXCEPTION_DERIVATION_ATTEMPTS=[{entries}]");
}

#[test]
fn real_idf61_exception_paths_expose_the_incomplete_known_ledgers() -> Result<(), String> {
    let attempts = [
        run_attempt("level1_entry", 0x4037_4340, |machine| {
            machine.cores[0].exccause = xtensa_lx7::state::exc::LEVEL1_INTERRUPT;
        })?,
        run_attempt("level3_entry", 0x4037_41c0, |machine| {
            machine.cores[0].eps[3] = machine.cores[0].ps;
            machine.cores[0].epc[3] = 0x4038_0000;
        })?,
        run_attempt("level1_resume", 0x4037_9214, |_| {})?,
        run_attempt("level3_resume", 0x4037_9360, |_| {})?,
        run_attempt("window_overflow8", 0x4037_4080, |machine| {
            machine.cores[0].set_ar(1, 0x3fca_af00);
            machine.cores[0].set_ar(9, 0x3fca_b000);
            machine
                .bus
                .load_bytes(0x3fca_aef4, &0x3fca_b100_u32.to_le_bytes())
                .expect("seed overflow handler spill base");
        })?,
        run_attempt("window_underflow8", 0x4037_40c0, |machine| {
            machine.cores[0].set_ar(1, 0x3fca_af00);
            machine.cores[0].set_ar(9, 0x3fca_b000);
            machine
                .bus
                .load_bytes(0x3fca_aff4, &0x3fca_b100_u32.to_le_bytes())
                .expect("seed underflow handler restored stack pointer");
            machine
                .bus
                .load_bytes(0x3fca_b0f4, &0x3fca_b200_u32.to_le_bytes())
                .expect("seed underflow handler caller stack pointer");
        })?,
    ];
    assert_eq!(
        attempts.map(|attempt| (attempt.stop_pc, attempt.known_cycles, attempt.stop)),
        [
            (0x4037_91a4, 17, "l32r_interval"),
            (0x4037_92f0, 12, "l32r_interval"),
            (0x4037_91c0, 5, "l32r_interval"),
            (0x4037_930c, 5, "l32r_interval"),
            (0x4037_409b, 9, "rfwo_zero_placeholder"),
            (0x4037_40db, 9, "rfwu_zero_placeholder"),
        ]
    );
    print_attempts(&attempts);
    Ok(())
}
