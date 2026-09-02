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
const FW: &str = "web/wasm/fw/public";

fn atech(extra: &[&str]) -> (Run, Vec<u8>) {
    let wav = tmp("atech.wav"); let rom = rom("esp32s3_rev0");
    let mut args: Vec<String> = ["--rom", rom.to_str().unwrap(), "--board", "atech14", "--boot", "rom", "--no-dump",
        "--bootloader", &format!("{FW}/atech-bootloader.bin"), "--ptable", &format!("{FW}/atech-ptable.bin"), "--app", &format!("{FW}/atech-firmware.bin"),
        "--script", &format!("{FW}/atech-script1.txt"), "--wav", wav.to_str().unwrap(), "--max-seconds", "5"].iter().map(|s| s.to_string()).collect();
    args.extend(extra.iter().map(|s| s.to_string()));
    let args: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
    let r = run(BIN, &args);
    let data = std::fs::read(&wav).expect("wav written");
    (r, data)
}

/// The Pocket Synth scenario: buttons, encoder, a serial command, the SID voice on I2S.
#[test] #[ignore = "needs the ESP32-S3 mask ROM ELF"]
fn atech_script1() {
    let (r, wav) = atech(&[]);
    expect_text("atech-script1.console.txt", &r.stdout);
    expect_sha("atech-script1.wav.sha256", &wav);
    expect_u64("atech-script1.insns", r.insns);
    // the committed fixture from the board directory must be the same audio
    assert_eq!(wav, std::fs::read(root().join("boards/atech14/regression.wav")).unwrap(), "boards/atech14/regression.wav drifted from the emulator's output");
}

/// `--no-jit` is the oracle: the block interpreter and the JIT must agree bit for bit.
#[test] #[ignore = "needs the ESP32-S3 mask ROM ELF"]
fn atech_script1_no_jit() {
    let (r, wav) = atech(&["--no-jit"]);
    expect_text("atech-script1.console.txt", &r.stdout);
    expect_sha("atech-script1.wav.sha256", &wav);
    expect_u64("atech-script1.insns", r.insns);
}

/// The C64 SID jukebox (cRSID): a 6502 + SID emulated inside the emulated S3, 3 s of Commando.
#[test] #[ignore = "needs the ESP32-S3 mask ROM ELF"]
fn atech_sid_jukebox() {
    let wav = tmp("sid.wav"); let rom = rom("esp32s3_rev0");
    let r = run(BIN, &["--rom", rom.to_str().unwrap(), "--board", "atech14", "--boot", "rom", "--no-dump",
        "--bootloader", &format!("{FW}/atech-bootloader.bin"), "--ptable", &format!("{FW}/atech-ptable.bin"), "--app", &format!("{FW}/atech-firmware.bin"),
        "--script", &format!("{FW}/atech-sid.txt"), "--wav", wav.to_str().unwrap(), "--max-seconds", "6"]);
    expect_text("atech-sid.console.txt", &r.stdout);
    expect_sha("atech-sid.wav.sha256", &std::fs::read(&wav).unwrap());
    expect_u64("atech-sid.insns", r.insns);
}

/// The Touch-LCD-4B energy panel in demo mode: PSRAM, LCD_CAM RGB frames, GT911 touch over I2C,
/// the ES8311 codec on I2S, swipes and a play tap from the script.
#[test] #[ignore = "needs the ESP32-S3 mask ROM ELF"]
fn panel_sid() {
    let wav = tmp("panel.wav"); let rom = rom("esp32s3_rev0");
    let r = run(BIN, &["--rom", rom.to_str().unwrap(), "--board", "waveshare-lcd4b", "--boot", "rom", "--no-dump", "--flash-mb", "16", "--psram-mb", "8", "--console", "usb",
        "--bootloader", &format!("{FW}/panel-bootloader.bin"), "--ptable", &format!("{FW}/panel-ptable.bin"), "--app", &format!("{FW}/panel-demo.bin"),
        "--flash-at", &format!("0x610000={FW}/energydata.json"),
        "--script", &format!("{FW}/panel-sid.txt"), "--wav", wav.to_str().unwrap(), "--max-seconds", "7"]);
    expect_text("panel-sid.console.txt", &r.stdout);
    expect_sha("panel-sid.wav.sha256", &std::fs::read(&wav).unwrap());
    expect_u64("panel-sid.insns", r.insns);
}

/// Stock ESP-IDF hello_world from the mask ROM through the bootloader into app_main, on UART0.
#[test] #[ignore = "needs the ESP32-S3 mask ROM ELF"]
fn hello_world_s3() {
    let h = "examples/hello_world/build"; let rom = rom("esp32s3_rev0");
    let r = run(BIN, &["--rom", rom.to_str().unwrap(), "--board", "none", "--boot", "rom", "--no-dump", "--console", "uart0",
        "--bootloader", &format!("{h}/bootloader/bootloader.bin"), "--ptable", &format!("{h}/partition_table/partition-table.bin"), "--app", &format!("{h}/hello_world.bin"),
        "--elf", &format!("{h}/hello_world.elf"), "--max-seconds", "3"]);
    assert!(r.stdout.contains("Hello world!"), "app_main never printed:\n{}", r.stdout);
    expect_text("hello-s3.console.txt", &r.stdout);
    expect_u64("hello-s3.insns", r.insns);
}
