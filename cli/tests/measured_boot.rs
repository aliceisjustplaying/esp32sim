use std::{path::PathBuf, process::Command};

const BUILD_ENVIRONMENT: &str = "TINYDRAW_IDF61_RECEIPT_BUILD";
const PRODUCT_BUILD_ENVIRONMENT: &str = "TINYDRAW_VECTOR_V2_BUILD";
const ROM_ENVIRONMENT: &str = "ESP32S3_ROM_ELF";
const EXPECTED_RECEIPT_BUILD: &str =
    include_str!("../../tests/correlation/measured-boot-refusal.json");
const EXPECTED_PRODUCT_BUILD: &str =
    include_str!("../../tests/correlation/measured-boot-product-refusal.json");

fn required_path(variable: &str) -> Result<PathBuf, String> {
    std::env::var_os(variable)
        .map(PathBuf::from)
        .ok_or_else(|| format!("{variable} must name the required measured-boot fixture"))
}

fn run_measured_boot(build_environment: &str) -> Result<Vec<u8>, String> {
    let build = required_path(build_environment)?;
    let rom = required_path(ROM_ENVIRONMENT)?;
    let output = Command::new(env!("CARGO_BIN_EXE_esp32sim"))
        .args(["--rom"])
        .arg(rom)
        .args(["--bootloader"])
        .arg(build.join("bootloader/bootloader.bin"))
        .args(["--ptable"])
        .arg(build.join("partition_table/partition-table.bin"))
        .args(["--app"])
        .arg(build.join("tinydraw_esp32.bin"))
        .args(["--elf"])
        .arg(build.join("tinydraw_esp32.elf"))
        .args([
            "--boot",
            "rom",
            "--board",
            "waveshare-amoled18-v2",
            "--console",
            "none",
            "--no-dump",
            "--measured",
        ])
        .env(PRODUCT_BUILD_ENVIRONMENT, &build)
        .output()
        .map_err(|error| format!("measured boot command must start: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "measured boot failed: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    Ok(output.stdout)
}

#[test]
#[ignore = "external receipt: run scripts/test-external-receipts.sh with the pinned build and ROM"]
fn real_tinydraw_measured_boot_stops_deterministically_at_committed_outcome() -> Result<(), String>
{
    let first = run_measured_boot(BUILD_ENVIRONMENT)?;
    let second = run_measured_boot(BUILD_ENVIRONMENT)?;
    assert_eq!(first, second);
    assert_eq!(first, EXPECTED_RECEIPT_BUILD.as_bytes());
    Ok(())
}

#[test]
fn current_tinydraw_measured_boot_stops_deterministically_at_committed_outcome(
) -> Result<(), String> {
    let first = run_measured_boot(PRODUCT_BUILD_ENVIRONMENT)?;
    let second = run_measured_boot(PRODUCT_BUILD_ENVIRONMENT)?;
    assert_eq!(first, second);
    assert_eq!(first, EXPECTED_PRODUCT_BUILD.as_bytes());
    Ok(())
}
