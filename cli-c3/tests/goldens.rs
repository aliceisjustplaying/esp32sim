//! Golden-output tests for the ESP32-C3 (see `cli/tests/goldens.rs` and `tests/README.md`).
#[path = "../../tests/common.rs"]
mod common;
use common::*;

const BIN: &str = env!("CARGO_BIN_EXE_esp32sim-c3");

/// hello_world from the C3 mask ROM, with the MAC / reset cause / straps of the real module the
/// boot log was compared against (`hw/c3-hello-world-real.txt`, `docs/esp32c3.md`).
#[test] #[ignore = "needs the ESP32-C3 mask ROM ELF"]
fn hello_world_c3() {
    let h = "examples/hello_world-c3/build"; let rom = rom("esp32c3_rev3");
    let r = run(BIN, &["--rom", rom.to_str().unwrap(), "--boot", "rom", "--flash-mb", "4",
        "--mac", "3c:84:27:b6:a7:1c", "--reset-cause", "0x15", "--strap", "0xd",
        "--bootloader", &format!("{h}/bootloader/bootloader.bin"), "--ptable", &format!("{h}/partition_table/partition-table.bin"), "--app", &format!("{h}/hello_world.bin"),
        "--elf", &format!("{h}/hello_world.elf"), "--max-seconds", "3"]);
    assert!(r.stdout.contains("Hello world!"), "app_main never printed:\n{}", r.stdout);
    expect_text("hello-c3.console.txt", &r.stdout);
    expect_u64("hello-c3.insns", r.insns);
}
