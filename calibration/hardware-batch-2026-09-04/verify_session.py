#!/usr/bin/env python3
"""Verify the exact three-image hardware batch without touching a board."""

import hashlib
import json
import sys
from pathlib import Path


EXPECTED_IDS = [
    "h1-exception-ladders",
    "tinydraw-frame-80mhz",
    "tinydraw-frame-40mhz",
]
EXPECTED_LAYOUT = {
    "0x0": "bootloader/bootloader.bin",
    "0x8000": "partition_table/partition-table.bin",
}


class VerificationError(ValueError):
    pass


def sha256(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def load_json(path: Path) -> dict:
    value = json.loads(path.read_text(encoding="utf-8"))
    if not isinstance(value, dict):
        raise VerificationError(f"{path} must contain a JSON object")
    return value


def require_equal(actual, expected, label: str) -> None:
    if actual != expected:
        raise VerificationError(f"{label} mismatch: {actual!r} != {expected!r}")


def verify_hash(path: Path, expected: str, label: str) -> None:
    if not path.is_file():
        raise VerificationError(f"missing {label}: {path}")
    require_equal(sha256(path), expected, f"{label} SHA-256")


def verify_flash_layout(bundle: Path, app_name: str) -> None:
    flasher = load_json(bundle / "flasher_args.json")
    layout = {**EXPECTED_LAYOUT, "0x10000": app_name}
    require_equal(flasher.get("flash_files"), layout, "flash layout")
    require_equal(
        flasher.get("write_flash_args"),
        ["--flash-mode", "dio", "--flash-size", "16MB", "--flash-freq", "80m"],
        "write-flash arguments",
    )
    require_equal(
        flasher.get("extra_esptool_args"),
        {"after": "hard-reset", "before": "default-reset", "stub": True, "chip": "esp32s3"},
        "esptool arguments",
    )


def verify_h1(bundle: Path, manifest: dict) -> None:
    artifacts = manifest.get("artifacts")
    if not isinstance(artifacts, dict):
        raise VerificationError("H1 manifest has no artifact map")
    for label, artifact in artifacts.items():
        if not isinstance(artifact, dict):
            raise VerificationError(f"H1 artifact {label} is invalid")
        verify_hash(bundle / artifact["path"], artifact["sha256"], f"H1 {label}")
    require_equal(manifest.get("toolchain", {}).get("idfVersion"), "v6.1", "H1 IDF")
    require_equal(len(manifest.get("cells", {})), 7, "H1 cell count")
    verify_flash_layout(bundle, "esp32s3_exception_ladders_calibration.bin")


def require_sdkconfig(bundle: Path, speed: int) -> None:
    lines = set((bundle / "sdkconfig").read_text(encoding="utf-8").splitlines())
    required = {
        'CONFIG_IDF_TARGET="esp32s3"',
        "CONFIG_ESP_DEFAULT_CPU_FREQ_MHZ=240",
        'CONFIG_ESPTOOLPY_FLASHMODE="dio"',
        "CONFIG_FLASHMODE_QIO=y",
        "CONFIG_SPIRAM_MODE_OCT=y",
        f"CONFIG_SPIRAM_SPEED_{speed}M=y",
    }
    missing = required - lines
    if missing:
        raise VerificationError(f"{bundle.name} sdkconfig missing: {', '.join(sorted(missing))}")


def verify_tinydraw(bundle: Path, manifest: dict, speed: int) -> None:
    require_equal(manifest.get("schema"), "tinydraw-frame-build-v1", "TinyDraw schema")
    require_equal(manifest.get("bundleId"), bundle.name, "TinyDraw bundle ID")
    require_equal(manifest.get("board"), "waveshare-esp32-s3-touch-amoled-1.8-v2", "board")
    require_equal(manifest.get("toolchain", {}).get("espIdf"), "v6.1", "TinyDraw IDF")
    require_equal(manifest.get("configuration", {}).get("flashMode"), "qio", "runtime flash mode")
    require_equal(manifest.get("configuration", {}).get("psramHz"), speed * 1_000_000, "PSRAM clock")
    require_equal(manifest.get("workload", {}).get("inputEvents"), 20, "input count")
    require_equal(manifest.get("workload", {}).get("expectedFrameKeys"), 21, "frame count")
    require_equal(
        manifest.get("workload", {}).get("core1TouchStoppedDuringReplay"), True, "core-1 touch state"
    )
    require_equal(manifest.get("unknownComponents"), ["psram"], "unknown components")
    require_equal(
        manifest.get("emulatorDryRun"),
        {
            "esp32simSourceCommit": "1e214e8cc7a0afcaed00623b3834b7e9f91b79a3",
            "esp32simExecutableSha256": "7011f188df79bcff294a8544026f73156342058f15203d1a6e63684f7b7475f9",
            "ready": True,
            "psramClockReadbackVerified": True,
            "replayComplete": True,
            "frameKeysContiguous": True,
        },
        "emulator dry-run receipt",
    )
    artifacts = manifest.get("artifacts")
    if not isinstance(artifacts, dict):
        raise VerificationError("TinyDraw manifest has no artifact map")
    for relative, digest in artifacts.items():
        verify_hash(bundle / relative, digest, f"TinyDraw {relative}")
    require_equal(set(path.name for path in bundle.iterdir()), {
        "MANIFEST.json", "bootloader", "partition_table", "rom", "sdkconfig",
        "flasher_args.json", "project_description.json", "tinydraw_esp32.bin",
        "tinydraw_esp32.elf",
    }, "TinyDraw bundle contents")
    require_sdkconfig(bundle, speed)
    verify_flash_layout(bundle, "tinydraw_esp32.bin")


def verify_pair(slow: dict, fast: dict) -> None:
    for key in (
        "sourceCommit", "baseCommit", "board", "toolchain", "workload", "markerSchemas",
        "tierCandidate", "unknownComponents",
    ):
        require_equal(slow.get(key), fast.get(key), f"paired {key}")
    slow_config = dict(slow["configuration"])
    fast_config = dict(fast["configuration"])
    for key in ("variant", "psramHz", "psramCoreHz", "psramCoreClockRegister"):
        slow_config.pop(key)
        fast_config.pop(key)
    require_equal(slow_config, fast_config, "paired invariant configuration")
    require_equal(slow["configuration"]["psramCoreHz"], 80_000_000, "40 MHz core clock")
    require_equal(fast["configuration"]["psramCoreHz"], 160_000_000, "80 MHz core clock")
    require_equal(slow["configuration"]["psramCoreClockRegister"], 0, "40 MHz clock selector")
    require_equal(fast["configuration"]["psramCoreClockRegister"], 2, "80 MHz clock selector")


def verify_session(contract_path: Path) -> dict:
    contract = load_json(contract_path)
    require_equal(contract.get("schemaVersion"), 1, "session schema")
    require_equal(contract.get("board"), "waveshare-esp32-s3-touch-amoled-1.8-v2", "session board")
    require_equal(contract.get("idf"), "v6.1", "session IDF")
    require_equal(
        contract.get("flashModeContract"),
        {"imageHeaderAndWrite": "dio", "runtimeAfterBootloader": "qio"},
        "flash-mode contract",
    )
    require_equal(
        contract.get("offlineTools"),
        {
            "esp32sim": {
                "path": "/Users/sarah/src/a/esp32sim/target/release/esp32sim",
                "sha256": "7011f188df79bcff294a8544026f73156342058f15203d1a6e63684f7b7475f9",
            },
            "frameCorrelation": {
                "sourceCommit": "28970f6d8fd1ddb49ef02087931ab64e88fe34cd",
                "sha256": "3db294f4c22f38f076c40efbe1b1e204d999bba71a81dba5febf50e6a93500d7",
            },
        },
        "offline tool pins",
    )
    require_equal(contract.get("restoreImage"), None, "restore image")
    images = contract.get("captureOrder")
    if not isinstance(images, list):
        raise VerificationError("captureOrder must be an array")
    require_equal([item.get("id") for item in images], EXPECTED_IDS, "capture order")
    require_equal([item.get("boots") for item in images], [2, 2, 2], "boot counts")
    require_equal(
        [item.get("terminal") for item in images],
        ["CALIBRATION_DONE", "TINYDRAW_DEMO_REPLAY_END", "TINYDRAW_DEMO_REPLAY_END"],
        "capture terminals",
    )
    manifests = []
    for item in images:
        bundle = Path(item["bundlePath"])
        manifest_path = bundle / "MANIFEST.json"
        verify_hash(manifest_path, item["manifestSha256"], f"{item['id']} manifest")
        manifest = load_json(manifest_path)
        manifests.append(manifest)
    verify_h1(Path(images[0]["bundlePath"]), manifests[0])
    verify_tinydraw(Path(images[1]["bundlePath"]), manifests[1], 80)
    verify_tinydraw(Path(images[2]["bundlePath"]), manifests[2], 40)
    verify_pair(manifests[2], manifests[1])
    return {"ok": True, "images": 3, "flashes": 3, "capturedBoots": 6}


def main() -> int:
    path = Path(sys.argv[1]) if len(sys.argv) > 1 else Path(__file__).with_name("session.json")
    try:
        print(json.dumps(verify_session(path), sort_keys=True))
    except (KeyError, OSError, TypeError, VerificationError, json.JSONDecodeError) as error:
        print(json.dumps({"ok": False, "error": str(error)}, sort_keys=True))
        return 2
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
