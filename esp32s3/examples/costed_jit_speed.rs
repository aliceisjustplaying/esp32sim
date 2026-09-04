use std::process::Command;
use std::time::Instant;

use emu_core::{Bus, Core};
use esp32s3::backend::Esp32CostModel;
use esp32s3::Machine;
use esp_soc::Stop;

const IRAM: u32 = 0x4038_0000;
const WARMUP_EVENTS: u64 = 20_000;
const SAMPLE_EVENTS: u64 = 200_000;
const SAMPLES: usize = 5;
const BUDGET_MIPS: f64 = 480.0;

struct Sample {
    core_events: [u64; 2],
    elapsed_seconds: f64,
    mips: f64,
}

fn main() -> Result<(), String> {
    let cpu = command("sysctl", &["-n", "machdep.cpu.brand_string"])?;
    if std::env::consts::ARCH != "aarch64" || !cpu.contains("Apple M1 Pro") {
        return Err(format!(
            "target host must be an arm64 Apple M1 Pro, found {} {cpu}",
            std::env::consts::ARCH
        ));
    }
    if !xtensa_lx7::jit::AVAILABLE {
        return Err("native Xtensa JIT is unavailable".into());
    }

    let os = command("sw_vers", &["-productVersion"])?;
    let mut samples = Vec::with_capacity(SAMPLES);
    for _ in 0..SAMPLES {
        samples.push(run_sample()?);
    }
    let mut ranked = samples.iter().map(|sample| sample.mips).collect::<Vec<_>>();
    ranked.sort_by(f64::total_cmp);
    let median_mips = ranked[SAMPLES / 2];
    let margin_mips = median_mips - BUDGET_MIPS;
    let margin_percent = 100.0 * (median_mips / BUDGET_MIPS - 1.0);

    println!("{{");
    println!("  \"schemaVersion\": 1,");
    println!("  \"host\": {{\"cpu\": \"{cpu}\", \"architecture\": \"aarch64\", \"macOS\": \"{os}\"}},");
    println!("  \"workload\": {{\"memory\": \"IRAM\", \"cores\": 2, \"blockInstructions\": 32, \"issueInstructions\": 31, \"jumpInstructions\": 1, \"warmupEvents\": {WARMUP_EVENTS}, \"sampleEvents\": {SAMPLE_EVENTS}, \"samples\": {SAMPLES}}},");
    println!("  \"raw\": [");
    for (index, sample) in samples.iter().enumerate() {
        let comma = if index + 1 == samples.len() { "" } else { "," };
        println!(
            "    {{\"core0Events\": {}, \"core1Events\": {}, \"elapsedSeconds\": {:.9}, \"mips\": {:.6}}}{comma}",
            sample.core_events[0], sample.core_events[1], sample.elapsed_seconds, sample.mips
        );
    }
    println!("  ],");
    println!("  \"medianMips\": {median_mips:.6},");
    println!("  \"dualCoreBudgetMips\": {BUDGET_MIPS:.1},");
    println!("  \"clearsBudget\": {},", median_mips >= BUDGET_MIPS);
    println!("  \"marginMips\": {margin_mips:.6},");
    println!("  \"marginPercent\": {margin_percent:.6}");
    println!("}}");
    Ok(())
}

fn run_sample() -> Result<Sample, String> {
    let mut machine = measured_machine()?;
    assert!(matches!(machine.run(WARMUP_EVENTS), Stop::MaxInsns));
    let before = [machine.cores[0].insn_count(), machine.cores[1].insn_count()];
    let native_before = [
        machine.cores[0].blocks.costed_native_insns,
        machine.cores[1].blocks.costed_native_insns,
    ];

    let started = Instant::now();
    assert!(matches!(machine.run(SAMPLE_EVENTS), Stop::MaxInsns));
    let elapsed_seconds = started.elapsed().as_secs_f64();
    let after = [machine.cores[0].insn_count(), machine.cores[1].insn_count()];
    let core_events = [after[0] - before[0], after[1] - before[1]];
    let expected_per_core = SAMPLE_EVENTS / 2;
    if core_events != [expected_per_core; 2] {
        return Err(format!("dual-core scheduler retired {core_events:?}"));
    }
    for (index, core) in machine.cores.iter().enumerate() {
        let native_events = core.blocks.costed_native_insns - native_before[index];
        if native_events != core_events[index] {
            return Err(format!(
                "core {index} retired {} of {} timed instructions in costed native code",
                native_events, core_events[index]
            ));
        }
        let (_, _, compiled, _) = core
            .code_cache_stats()
            .ok_or_else(|| "LX7 code-cache statistics unavailable".to_string())?;
        if compiled < 2 {
            return Err(format!("costed JIT did not compile both runs: {compiled}"));
        }
    }

    Ok(Sample {
        core_events,
        elapsed_seconds,
        mips: SAMPLE_EVENTS as f64 / elapsed_seconds / 1_000_000.0,
    })
}

fn measured_machine() -> Result<Machine, String> {
    let mut machine = esp32s3::machine([0; 6]);
    receipt_config(&mut machine);
    let kernel = kernel();
    machine.bus.load_bytes(IRAM, &kernel)?;
    machine.cores[0].pc = IRAM;
    machine.bus.write32(0x600c_0000, 0b010).map_err(|fault| format!("release core 1: {fault:?}"))?;
    machine.set_cost_model(Box::new(Esp32CostModel::default()))?;
    assert!(matches!(machine.run(0), Stop::MaxInsns));
    machine.cores[0].pc = IRAM;
    machine.cores[1].pc = IRAM;
    Ok(machine)
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
    machine.bus.periph.extmem.ram.write(0x60, (1 << 3) | (1 << 1));
}

fn kernel() -> Vec<u8> {
    let mut bytes = Vec::with_capacity(32 * 3);
    for immediate in 0..31 {
        bytes.extend_from_slice(&encode_movi(2, immediate));
    }
    let jump_pc = IRAM + bytes.len() as u32;
    bytes.extend_from_slice(&encode_j(jump_pc, IRAM));
    bytes
}

fn encode_movi(register: u8, immediate: i32) -> [u8; 3] {
    let immediate = immediate as u32 & 0xfff;
    [
        (register << 4) | 2,
        0xa0 | ((immediate >> 8) as u8 & 0xf),
        immediate as u8,
    ]
}

fn encode_j(pc: u32, target: u32) -> [u8; 3] {
    let offset = target.wrapping_sub(pc + 4) as i32;
    let word = ((offset as u32 & 0x3ffff) << 6) | 6;
    [word as u8, (word >> 8) as u8, (word >> 16) as u8]
}

fn command(program: &str, args: &[&str]) -> Result<String, String> {
    let output = Command::new(program)
        .args(args)
        .output()
        .map_err(|error| format!("run {program}: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "{program} failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}
