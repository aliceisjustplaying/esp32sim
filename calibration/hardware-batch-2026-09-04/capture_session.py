#!/usr/bin/env -S uv run --script
# /// script
# requires-python = ">=3.11"
# dependencies = ["pyserial==3.5"]
# ///
"""Run the bound three-image hardware session and retain every receipt."""

from __future__ import annotations

import argparse
import datetime
import hashlib
import json
import shutil
import subprocess
import sys
import time
from pathlib import Path


SCRIPT_DIR = Path(__file__).resolve().parent
REPO_ROOT = SCRIPT_DIR.parents[1]
TOOLS_DIR = REPO_ROOT / "calibration" / "tools"
sys.path.insert(0, str(TOOLS_DIR))
sys.path.insert(0, str(SCRIPT_DIR))

from capture import failure_marker  # noqa: E402
from ndjson import ManifestContract, ValidationError, validate_calibration_lines  # noqa: E402
from verify_session import verify_session  # noqa: E402


IDF_PYTHON = Path("/Users/sarah/.espressif/tools/python/v6.1/venv/bin/python")
ESPTOOL_VERSION = "esptool v5.3.1"
FRAME_TOOL_SHA256 = "3db294f4c22f38f076c40efbe1b1e204d999bba71a81dba5febf50e6a93500d7"
TINYDRAW_FAILURES = (
    b"TINYDRAW_LIVE_FAIL",
    b"TINYDRAW_FRAME_TRACE_FAIL",
    b"TINYDRAW_DEMO_FAIL",
)
TINYDRAW_BOOT_MARKERS = (
    "mode:DIO, clock div:1",
    "qio_mode: Enabling default flash chip QIO",
    "Boot SPI Speed : 80MHz",
    "SPI Mode       : QIO",
    "spi_flash: flash io: qio",
    "TINYDRAW_VECTOR_V2_READY",
    "TINYDRAW_FRAME_TRACE_REPLAY_BEGIN count=20 core1_touch_stopped=1",
    "TINYDRAW_DEMO_REPLAY_END count=20",
)


class SessionError(ValueError):
    pass


def sha256(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def atomic_json(path: Path, value: dict) -> None:
    temporary = path.with_suffix(path.suffix + ".tmp")
    temporary.write_text(json.dumps(value, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    temporary.replace(path)


def read_contract(path: Path) -> dict:
    value = json.loads(path.read_text(encoding="utf-8"))
    if not isinstance(value, dict):
        raise SessionError("session contract must be a JSON object")
    return value


def check_tool(path: Path, expected_sha256: str, label: str) -> None:
    if not path.is_file():
        raise SessionError(f"missing {label}: {path}")
    if sha256(path) != expected_sha256:
        raise SessionError(f"{label} SHA-256 mismatch")


def check_esptool() -> str:
    if not IDF_PYTHON.is_file():
        raise SessionError(f"missing IDF 6.1 Python: {IDF_PYTHON}")
    result = subprocess.run(
        [str(IDF_PYTHON), "-m", "esptool", "version"],
        check=True,
        capture_output=True,
        text=True,
    )
    if ESPTOOL_VERSION not in result.stdout:
        raise SessionError(f"unexpected esptool version: {result.stdout.strip()}")
    return result.stdout.strip()


def require_unowned_port(port: str) -> None:
    result = subprocess.run(["lsof", port], capture_output=True, text=True)
    if result.returncode == 0 and result.stdout.strip():
        raise SessionError(f"serial port is already owned:\n{result.stdout.strip()}")
    if result.returncode not in (0, 1):
        raise SessionError(f"cannot check serial-port ownership: {result.stderr.strip()}")


def run_logged(command: list[str], output: Path) -> None:
    with output.open("wb") as log:
        process = subprocess.run(command, stdout=log, stderr=subprocess.STDOUT)
    if process.returncode != 0:
        raise SessionError(f"command failed with status {process.returncode}; see {output}")


def flash_command(bundle: Path, port: str) -> list[str]:
    flasher = json.loads((bundle / "flasher_args.json").read_text(encoding="utf-8"))
    extra = flasher["extra_esptool_args"]
    command = [
        str(IDF_PYTHON),
        "-m",
        "esptool",
        "--chip",
        extra["chip"],
        "--port",
        port,
        "--before",
        extra["before"],
        "--after",
        extra["after"],
        "write-flash",
        *flasher["write_flash_args"],
    ]
    for offset, relative in sorted(
        flasher["flash_files"].items(), key=lambda item: int(item[0], 16)
    ):
        command.extend((offset, str(bundle / relative)))
    return command


def capture_boot(port: str, output: Path, terminal: str, timeout_s: float) -> list[str]:
    import serial

    terminal_bytes = terminal.encode()
    buffer = b""
    terminal_seen = False
    done_at: float | None = None
    boot_boundaries = {b"ESP-ROM:": 0, b"rst:": 0}
    with output.open("wb") as raw:
        with serial.Serial(port, 115200, timeout=0.25) as device:
            device.dtr = False
            device.rts = True
            time.sleep(0.2)
            device.rts = False
            deadline = time.monotonic() + timeout_s
            while time.monotonic() < deadline:
                if done_at is not None and time.monotonic() >= done_at + 1.0:
                    break
                data = device.read(4096)
                if not data:
                    continue
                raw.write(data)
                raw.flush()
                buffer += data
                while b"\n" in buffer:
                    line, buffer = buffer.split(b"\n", 1)
                    for marker in boot_boundaries:
                        if marker in line:
                            boot_boundaries[marker] += 1
                            if boot_boundaries[marker] > 1:
                                raise SessionError(f"repeated boot boundary in {output}")
                    if failure_marker(line) is not None or any(
                        marker in line for marker in TINYDRAW_FAILURES
                    ):
                        raise SessionError(f"hardware failure marker in {output}")
                    if terminal_bytes in line:
                        terminal_seen = True
                        done_at = time.monotonic()
    if not terminal_seen:
        raise SessionError(f"capture timed out before {terminal}; raw log retained at {output}")
    if set(boot_boundaries.values()) != {1}:
        raise SessionError(f"capture does not contain one clean boot boundary: {output}")
    payload = output.read_bytes()
    if failure_marker(payload) is not None or any(
        marker in payload for marker in TINYDRAW_FAILURES
    ):
        raise SessionError(f"hardware failure marker in {output}")
    try:
        return payload.decode("utf-8").splitlines()
    except UnicodeError as error:
        raise SessionError(f"capture is not valid UTF-8: {output}") from error


def validate_h1(bundle: Path, raw: Path, lines: list[str]) -> dict:
    contract = ManifestContract.load(bundle / "probe-cells.json")
    tally = validate_calibration_lines(lines, contract, "normal", "all", False)
    value = {"ok": True, **tally.as_dict()}
    atomic_json(raw.with_suffix(".validation.json"), value)
    return value


def validate_tinydraw(raw: Path, lines: list[str]) -> None:
    text = "\n".join(lines)
    missing = [marker for marker in TINYDRAW_BOOT_MARKERS if marker not in text]
    if missing:
        raise SessionError(f"{raw} is missing boot markers: {', '.join(missing)}")


def normalize_frame(frame_tool: Path, bundle: Path, raw: Path, output: Path) -> None:
    run_logged(
        [
            sys.executable,
            str(frame_tool),
            "normalize",
            str(bundle / "MANIFEST.json"),
            str(raw),
            "--source",
            "hardware",
        ],
        output,
    )


def analyze_pair(frame_tool: Path, slow: Path, fast: Path, output: Path) -> dict:
    run_logged(
        [sys.executable, str(frame_tool), "psram-candidate", str(slow), str(fast)],
        output,
    )
    result = json.loads(output.read_text(encoding="utf-8"))
    if (
        result.get("disposition") != "candidate-evidence-only"
        or result.get("classification") != "distribution"
        or result.get("nonPsramPartition") is not None
        or result.get("onePercentClaim") != "refused"
        or result.get("cycles40MHz", {}).get("samples") != 21
        or result.get("cycles80MHz", {}).get("samples") != 21
    ):
        raise SessionError(f"paired analysis did not preserve the fail-closed contract: {output}")
    return result


def write_sha256sums(directory: Path) -> None:
    entries = []
    for path in sorted(item for item in directory.rglob("*") if item.is_file()):
        if path.name != "SHA256SUMS":
            entries.append(f"{sha256(path)}  {path.relative_to(directory)}")
    (directory / "SHA256SUMS").write_text("\n".join(entries) + "\n", encoding="utf-8")


def run_session(args: argparse.Namespace) -> Path | None:
    contract_path = args.session.resolve(strict=True)
    verification = verify_session(contract_path)
    contract = read_contract(contract_path)
    frame_tool = args.frame_tool.resolve(strict=True)
    check_tool(frame_tool, FRAME_TOOL_SHA256, "frame-correlation tool")
    esptool_version = check_esptool()
    if args.verify_only:
        print(json.dumps(verification, sort_keys=True))
        return None
    require_unowned_port(args.port)
    stamp = datetime.datetime.now().strftime("%Y%m%d-%H%M%S")
    archive = args.archive_root.expanduser() / f"hardware-batch-2026-09-04-{stamp}"
    archive.mkdir(parents=True, exist_ok=False)
    shutil.copy2(contract_path, archive / "session-contract.json")
    shutil.copy2(Path(__file__), archive / "capture_session.py")
    shutil.copy2(SCRIPT_DIR / "verify_session.py", archive / "verify_session.py")
    shutil.copy2(frame_tool, archive / "frame_correlation.py")
    frame_tool = archive / "frame_correlation.py"
    state = {
        "schemaVersion": 1,
        "status": "running",
        "startedAt": datetime.datetime.now(datetime.timezone.utc).isoformat(),
        "port": args.port,
        "contractVerification": verification,
        "tools": {
            "captureSessionSha256": sha256(archive / "capture_session.py"),
            "verifySessionSha256": sha256(archive / "verify_session.py"),
            "frameCorrelationSha256": sha256(archive / "frame_correlation.py"),
            "idfPython": str(IDF_PYTHON),
            "esptoolVersion": esptool_version,
        },
        "images": [],
        "installedImage": None,
    }
    state_path = archive / "session-state.json"
    atomic_json(state_path, state)
    try:
        normalized: dict[tuple[int, int], Path] = {}
        for image_index, image in enumerate(contract["captureOrder"]):
            verify_session(contract_path)
            bundle = Path(image["bundlePath"])
            image_dir = archive / image["id"]
            image_dir.mkdir()
            command = flash_command(bundle, args.port)
            image_state = {
                "id": image["id"],
                "bundlePath": str(bundle),
                "manifestSha256": image["manifestSha256"],
                "status": "flashing",
                "flashCommand": command,
                "boots": [],
            }
            state["images"].append(image_state)
            state["installedImage"] = None
            state["flashInProgress"] = image["id"]
            atomic_json(state_path, state)
            run_logged(command, image_dir / "flash.log")
            state["installedImage"] = image["id"]
            state["flashInProgress"] = None
            image_state["status"] = "flashed"
            image_state["flashLogSha256"] = sha256(image_dir / "flash.log")
            atomic_json(state_path, state)
            for boot in range(1, image["boots"] + 1):
                raw = image_dir / f"boot-{boot}.log"
                boot_state = {"boot": boot, "raw": raw.name, "status": "capturing"}
                image_state["boots"].append(boot_state)
                atomic_json(state_path, state)
                try:
                    lines = capture_boot(args.port, raw, image["terminal"], args.timeout_s)
                    boot_state["rawSha256"] = sha256(raw)
                    if image_index == 0:
                        boot_state["validation"] = validate_h1(bundle, raw, lines)
                    else:
                        validate_tinydraw(raw, lines)
                        normalized_path = image_dir / f"boot-{boot}.normalized.ndjson"
                        normalize_frame(frame_tool, bundle, raw, normalized_path)
                        boot_state["normalized"] = normalized_path.name
                        boot_state["normalizedSha256"] = sha256(normalized_path)
                        normalized[(image_index, boot)] = normalized_path
                    boot_state["status"] = "complete"
                except (Exception, KeyboardInterrupt):
                    boot_state["status"] = "failed"
                    if raw.is_file():
                        boot_state["rawSha256"] = sha256(raw)
                    atomic_json(state_path, state)
                    raise
                atomic_json(state_path, state)
            image_state["status"] = "complete"
            atomic_json(state_path, state)
        if state["installedImage"] != "tinydraw-frame-40mhz":
            raise SessionError("successful session must leave the 40 MHz image installed")
        reports = []
        for boot in (1, 2):
            report_path = archive / f"frame-pair-boot-{boot}.json"
            analyze_pair(frame_tool, normalized[(2, boot)], normalized[(1, boot)], report_path)
            reports.append({"boot": boot, "path": report_path.name, "sha256": sha256(report_path)})
        state["framePairReports"] = reports
        state["status"] = "complete"
        state["completedAt"] = datetime.datetime.now(datetime.timezone.utc).isoformat()
        atomic_json(state_path, state)
        write_sha256sums(archive)
    except (Exception, KeyboardInterrupt) as error:
        state["status"] = "failed"
        state["failedAt"] = datetime.datetime.now(datetime.timezone.utc).isoformat()
        state["error"] = str(error) or type(error).__name__
        atomic_json(state_path, state)
        write_sha256sums(archive)
        raise
    return archive


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--port")
    parser.add_argument("--session", type=Path, default=SCRIPT_DIR / "session.json")
    parser.add_argument(
        "--frame-tool", type=Path, default=REPO_ROOT / "tools" / "frame_correlation.py"
    )
    parser.add_argument(
        "--archive-root", type=Path, default=Path("~/Archives/esp32s3")
    )
    parser.add_argument("--timeout-s", type=float, default=180.0)
    parser.add_argument("--verify-only", action="store_true")
    args = parser.parse_args()
    if not args.verify_only and not args.port:
        parser.error("--port is required unless --verify-only is used")
    try:
        archive = run_session(args)
    except (
        KeyboardInterrupt,
        OSError,
        SessionError,
        ValidationError,
        subprocess.SubprocessError,
        json.JSONDecodeError,
    ) as error:
        print(json.dumps({"ok": False, "error": str(error)}, sort_keys=True))
        return 2
    print(json.dumps({"ok": True, "archive": str(archive) if archive else None}, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
