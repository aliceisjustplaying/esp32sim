use std::path::{Path, PathBuf};

const BUILD_ENVIRONMENT: &str = "TINYDRAW_VECTOR_V2_BUILD";
const ROM_ENVIRONMENT: &str = "ESP32S3_ROM_ELF";
const READY_MARKER: &[u8] = b"TINYDRAW_VECTOR_V2_READY";
const MAX_INSTRUCTIONS: u64 = 3_000_000_000;

fn required_path(variable: &str) -> Result<PathBuf, String> {
    std::env::var_os(variable)
        .map(PathBuf::from)
        .ok_or_else(|| format!("{variable} must name the required fast-boot fixture"))
}

fn read(path: &Path) -> Result<Vec<u8>, String> {
    std::fs::read(path).map_err(|error| format!("{}: {error}", path.display()))
}

#[test]
fn tinydraw_reaches_ready_in_fast_mode() -> Result<(), String> {
    let build = required_path(BUILD_ENVIRONMENT)?;
    let rom = required_path(ROM_ENVIRONMENT)?;
    let mut machine = esp32s3::machine([0x44, 0x1b, 0xf6, 0x75, 0xdc, 0xe0]);
    machine.bus.board = esp32s3::board::make_board("waveshare-amoled18-v2")
        .ok_or_else(|| "waveshare-amoled18-v2 board must exist".to_string())?;
    for (controller, address, device) in machine.bus.board.i2c_devices() {
        machine.bus.periph.i2c[controller as usize].attach(address, device);
    }
    machine.bus.flash = vec![0xff; 16 * 1024 * 1024];
    let flash_capacity = (16usize * 1024 * 1024).trailing_zeros() as u8;
    machine.bus.periph.spi1.jedec[2] = flash_capacity;
    machine.bus.periph.spi0.jedec[2] = flash_capacity;
    machine.bus.psram = vec![0; 8 * 1024 * 1024];
    machine.bus.rebuild_page_table();
    machine.load_rom(&read(&rom)?)?;
    machine.write_flash(0, &read(&build.join("bootloader/bootloader.bin"))?)?;
    machine.write_flash(
        0x8000,
        &read(&build.join("partition_table/partition-table.bin"))?,
    )?;
    machine.write_flash(0x10000, &read(&build.join("tinydraw_esp32.bin"))?)?;
    machine.add_symbols(&read(&build.join("tinydraw_esp32.elf"))?)?;
    machine.boot_rom();
    machine.console.mask = 1;

    let _stop = machine.run(MAX_INSTRUCTIONS);

    assert!(
        machine
            .console
            .usb
            .windows(READY_MARKER.len())
            .any(|window| window == READY_MARKER),
        "USB console did not contain TINYDRAW_VECTOR_V2_READY within {MAX_INSTRUCTIONS} fast-mode instructions"
    );
    Ok(())
}
