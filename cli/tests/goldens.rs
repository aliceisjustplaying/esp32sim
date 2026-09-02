//! Golden-output tests: the emulator must produce byte-identical console text, audio and
//! instruction counts for the committed demo firmware. This is the regression bar every
//! refactor and every speed change is held to (`docs/decisions.md`, "Performance").
//!
//! They need the ESP32-S3 mask ROM ELF, which ships with ESP-IDF and is not checked in, so they
//! are `#[ignore]`d: run `cargo test --release --workspace -- --include-ignored` (CI does; see
//! `tests/README.md`). Regenerate after an intentional change with `UPDATE_GOLDENS=1`.
#[path = "../../tests/common.rs"]
mod common;
use common::*;

const BIN: &str = env!("CARGO_BIN_EXE_esp32sim");
const BIN_C3: &str = env!("CARGO_BIN_EXE_esp32sim-c3");
const FW: &str = "web/wasm/fw/public";

fn atech(extra: &[&str]) -> (Run, Vec<u8>) {
    let wav = tmp("atech.wav");
    let rom = rom("esp32s3_rev0");
    let mut args: Vec<String> = [
        "--rom",
        rom.to_str().expect("the ROM path is valid UTF-8"),
        "--board",
        "atech14",
        "--boot",
        "rom",
        "--no-dump",
        "--bootloader",
        &format!("{FW}/atech-bootloader.bin"),
        "--ptable",
        &format!("{FW}/atech-ptable.bin"),
        "--app",
        &format!("{FW}/atech-firmware.bin"),
        "--script",
        &format!("{FW}/atech-script1.txt"),
        "--wav",
        wav.to_str().expect("the audio output path is valid UTF-8"),
        "--max-seconds",
        "5",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    args.extend(extra.iter().map(|s| s.to_string()));
    let args: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
    let r = run(BIN, &args);
    let data = std::fs::read(&wav).expect("wav written");
    (r, data)
}

/// The Pocket Synth scenario: buttons, encoder, a serial command, the SID voice on I2S.
#[test]
#[ignore = "needs the ESP32-S3 mask ROM ELF"]
fn atech_script1() {
    let (r, wav) = atech(&[]);
    expect_text("atech-script1.console.txt", &r.stdout);
    expect_sha("atech-script1.wav.sha256", &wav);
    expect_u64("atech-script1.insns", r.insns);
    // the committed fixture from the board directory must be the same audio
    assert_eq!(
        wav,
        std::fs::read(root().join("boards/atech14/regression.wav")).unwrap(),
        "boards/atech14/regression.wav drifted from the emulator's output"
    );
}

/// `--no-jit` is the oracle: the block interpreter and the JIT must agree bit for bit.
#[test]
#[ignore = "needs the ESP32-S3 mask ROM ELF"]
fn atech_script1_no_jit() {
    let (r, wav) = atech(&["--no-jit"]);
    expect_text("atech-script1.console.txt", &r.stdout);
    expect_sha("atech-script1.wav.sha256", &wav);
    expect_u64("atech-script1.insns", r.insns);
}

/// The C64 SID jukebox (cRSID): a 6502 + SID emulated inside the emulated S3, 3 s of Commando.
#[test]
#[ignore = "needs the ESP32-S3 mask ROM ELF"]
fn atech_sid_jukebox() {
    let wav = tmp("sid.wav");
    let rom = rom("esp32s3_rev0");
    let r = run(
        BIN,
        &[
            "--rom",
            rom.to_str().unwrap(),
            "--board",
            "atech14",
            "--boot",
            "rom",
            "--no-dump",
            "--bootloader",
            &format!("{FW}/atech-bootloader.bin"),
            "--ptable",
            &format!("{FW}/atech-ptable.bin"),
            "--app",
            &format!("{FW}/atech-firmware.bin"),
            "--script",
            &format!("{FW}/atech-sid.txt"),
            "--wav",
            wav.to_str().unwrap(),
            "--max-seconds",
            "6",
        ],
    );
    expect_text("atech-sid.console.txt", &r.stdout);
    expect_sha("atech-sid.wav.sha256", &std::fs::read(&wav).unwrap());
    expect_u64("atech-sid.insns", r.insns);
}

/// The Touch-LCD-4B energy panel in demo mode: PSRAM, LCD_CAM RGB frames, GT911 touch over I2C,
/// the ES8311 codec on I2S, swipes and a play tap from the script.
#[test]
#[ignore = "needs the ESP32-S3 mask ROM ELF"]
fn panel_sid() {
    let wav = tmp("panel.wav");
    let rom = rom("esp32s3_rev0");
    let r = run(
        BIN,
        &[
            "--rom",
            rom.to_str().unwrap(),
            "--board",
            "waveshare-lcd4b",
            "--boot",
            "rom",
            "--no-dump",
            "--flash-mb",
            "16",
            "--psram-mb",
            "8",
            "--console",
            "usb",
            "--bootloader",
            &format!("{FW}/panel-bootloader.bin"),
            "--ptable",
            &format!("{FW}/panel-ptable.bin"),
            "--app",
            &format!("{FW}/panel-demo.bin"),
            "--flash-at",
            &format!("0x610000={FW}/energydata.json"),
            "--script",
            &format!("{FW}/panel-sid.txt"),
            "--wav",
            wav.to_str().unwrap(),
            "--max-seconds",
            "7",
        ],
    );
    expect_text("panel-sid.console.txt", &r.stdout);
    expect_sha("panel-sid.wav.sha256", &std::fs::read(&wav).unwrap());
    expect_u64("panel-sid.insns", r.insns);
}

/// Observers attached must not change the run (the block-path ones run at full speed), and
/// each must produce its report.
#[test]
#[ignore = "needs the ESP32-S3 mask ROM ELF"]
fn atech_script1_with_observers() {
    let (cov, vcd) = (tmp("cov.txt"), tmp("atech.vcd"));
    let (r, wav) = atech(&[
        "--profile-blocks",
        "--coverage-file",
        cov.to_str().unwrap(),
        "--irq-latency",
        "--vcd",
        vcd.to_str().unwrap(),
    ]);
    expect_text("atech-script1.console.txt", &r.stdout);
    expect_sha("atech-script1.wav.sha256", &wav);
    expect_u64("atech-script1.insns", r.insns);
    for tag in [
        "[profile-blocks] top",
        "[coverage] ",
        "[irq-latency] per core",
        "[vcd] wrote",
    ] {
        assert!(r.stderr.contains(tag), "missing {} in:\n{}", tag, r.stderr);
    }
    assert!(
        r.stderr.contains("core0 int9"),
        "the systimer line should show up in the latency table"
    );
    let v = std::fs::read_to_string(&vcd).unwrap();
    assert!(
        v.starts_with("$timescale 1ps $end")
            && v.contains("gpio2 $end")
            && v.contains("core0_int9 $end"),
        "vcd header"
    );
    let c = std::fs::read_to_string(&cov).unwrap();
    assert!(
        c.lines().count() > 5000,
        "coverage rows: {}",
        c.lines().count()
    );
}

/// Stock ESP-IDF hello_world from the mask ROM through the bootloader into app_main, on UART0.
#[test]
#[ignore = "needs the ESP32-S3 mask ROM ELF"]
fn hello_world_s3() {
    let rom = rom("esp32s3_rev0");
    let r = run(
        BIN,
        &[
            "--rom",
            rom.to_str().unwrap(),
            "--board",
            "none",
            "--boot",
            "rom",
            "--no-dump",
            "--console",
            "uart0",
            "--bootloader",
            &format!("{FW}/hello-bootloader.bin"),
            "--ptable",
            &format!("{FW}/hello-ptable.bin"),
            "--app",
            &format!("{FW}/hello_world.bin"),
            "--max-seconds",
            "3",
        ],
    );
    assert!(
        r.stdout.contains("Hello world!"),
        "app_main never printed:\n{}",
        r.stdout
    );
    expect_text("hello-s3.console.txt", &r.stdout);
    expect_u64("hello-s3.insns", r.insns);
}

/// The block profile attributes time to symbols (the ROM's here), and the per-instruction
/// `--profile` (slow path, idle cores stepping) still agrees on the hottest function.
#[test]
#[ignore = "needs the ESP32-S3 mask ROM ELF"]
fn hello_world_s3_profiles() {
    let rom = rom("esp32s3_rev0");
    let base = [
        "--rom",
        rom.to_str().unwrap(),
        "--board",
        "none",
        "--boot",
        "rom",
        "--no-dump",
        "--console",
        "none",
        "--bootloader",
        &format!("{FW}/hello-bootloader.bin"),
        "--ptable",
        &format!("{FW}/hello-ptable.bin"),
        "--app",
        &format!("{FW}/hello_world.bin"),
        "--max-seconds",
        "1",
    ];
    let mut a = base.to_vec();
    a.push("--profile-blocks");
    let r = run(BIN, &a);
    let line = r
        .stderr
        .lines()
        .skip_while(|l| !l.starts_with("[profile-blocks]"))
        .nth(1)
        .unwrap_or("");
    assert!(
        line.contains("ets_delay_us"),
        "hottest function should be the ROM's delay loop, got: {:?}",
        line
    );
    let mut b = base.to_vec();
    b.push("--profile");
    let r = run(BIN, &b);
    assert!(r.stderr.contains("[profile] top 12 pcs"), "{}", r.stderr);
}

/// hello_world from the C3 mask ROM, with the MAC / reset cause / straps of the real module the
/// boot log was compared against (`hw/c3-hello-world-real.txt`, `docs/esp32c3.md`).
#[test]
#[ignore = "needs the ESP32-C3 mask ROM ELF"]
fn hello_world_c3() {
    let rom = rom("esp32c3_rev3");
    let r = run(
        BIN_C3,
        &[
            "--rom",
            rom.to_str().unwrap(),
            "--boot",
            "rom",
            "--flash-mb",
            "4",
            "--mac",
            "3c:84:27:b6:a7:1c",
            "--reset-cause",
            "0x15",
            "--strap",
            "0xd",
            "--bootloader",
            &format!("{FW}/c3-hello-bootloader.bin"),
            "--ptable",
            &format!("{FW}/c3-hello-ptable.bin"),
            "--app",
            &format!("{FW}/c3-hello_world.bin"),
            "--max-seconds",
            "3",
        ],
    );
    assert!(
        r.stdout.contains("Hello world!"),
        "app_main never printed:\n{}",
        r.stdout
    );
    expect_text("hello-c3.console.txt", &r.stdout);
    expect_u64("hello-c3.insns", r.insns);
    // the same run through the one binary
    let r2 = run(
        BIN,
        &[
            "--chip",
            "c3",
            "--rom",
            rom.to_str().unwrap(),
            "--boot",
            "rom",
            "--flash-mb",
            "4",
            "--mac",
            "3c:84:27:b6:a7:1c",
            "--reset-cause",
            "0x15",
            "--strap",
            "0xd",
            "--bootloader",
            &format!("{FW}/c3-hello-bootloader.bin"),
            "--ptable",
            &format!("{FW}/c3-hello-ptable.bin"),
            "--app",
            &format!("{FW}/c3-hello_world.bin"),
            "--max-seconds",
            "3",
        ],
    );
    assert_eq!(r.stdout, r2.stdout);
    assert_eq!(r.insns, r2.insns);
}
