"""Stubbed tests for capture.py's session flow: no hardware, no eim, no esptool.

Run: uv run --with pytest --python 3.12 python -m pytest calibration/tools/test_capture_session.py
"""
import hashlib
import json
import sys
from pathlib import Path

import pytest

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
import capture  # noqa: E402

IMAGE = HERE.parent / "esp32s3-rom-fetch-ladder-v2"


def make_build(tmp: Path, elf_bytes: bytes, embed_hash: bytes | None = None) -> Path:
    build = tmp / "build"
    (build / "bootloader").mkdir(parents=True)
    (build / "partition_table").mkdir()
    (build / "bootloader" / "bootloader.bin").write_bytes(b"BL")
    (build / "partition_table" / "partition-table.bin").write_bytes(b"PT")
    (build / "sdkconfig").write_text("CONFIG_FREERTOS_NUMBER_OF_CORES=2\n")
    (build / "flash_args").write_text("--flash-mode dio\n")
    (build / "flasher_args.json").write_text(json.dumps({
        "flash_files": {"0x0": "bootloader/bootloader.bin", "0x8000": "partition_table/partition-table.bin", "0x10000": "app.bin"},
        "flash_settings": {"flash_mode": "dio", "flash_freq": "80m", "flash_size": "16MB"},
    }))
    (build / "app.elf").write_bytes(elf_bytes)
    digest = embed_hash if embed_hash is not None else hashlib.sha256(elf_bytes).digest()
    (build / "app.bin").write_bytes(b"\0" * 176 + digest + b"\0" * 64)
    return build


class Stub:
    """Fake subprocess.run: writes a verification receipt for the eim verify call, records esptool calls."""

    def __init__(self, ok=True):
        self.calls = []
        self.ok = ok

    def __call__(self, cmd, **kw):
        self.calls.append(cmd)
        if cmd[0] == "eim":
            out = Path(cmd[2].split()[3])
            elf = Path(cmd[2].split()[2])
            manifest = IMAGE / "probe-cells.json"
            out.write_text(json.dumps({
                "ok": self.ok,
                "appElfSha256": hashlib.sha256(elf.read_bytes()).hexdigest(),
                "manifest": {"manifestSha256": hashlib.sha256(manifest.read_bytes()).hexdigest()},
            }))

        class R:
            returncode = 0
            stdout = "stub esptool\n"
            stderr = ""
        return R()


def run_session(tmp, monkeypatch, build, stub):
    monkeypatch.setattr(capture.subprocess, "run", stub)
    monkeypatch.setattr(capture, "_capture_boot", lambda port, cap, t, term: (cap.write_text("x\n"), ["x"])[1])
    monkeypatch.setattr(capture, "validate_calibration_lines", lambda *a, **k: type("T", (), {"as_dict": lambda self: {"complete": True}})())
    monkeypatch.setattr(sys, "argv", ["capture.py", "--image", str(IMAGE), "--build", str(build), "--boots", "1", "--port", "/dev/null", "--archive-root", str(tmp / "archive")])
    return capture.session_main()


def test_verified_archived_elf_and_esptool_from_archive(tmp_path, monkeypatch, capsys):
    build = make_build(tmp_path, b"ELF-A")
    stub = Stub()
    assert run_session(tmp_path, monkeypatch, build, stub) == 0
    archive = next((tmp_path / "archive").iterdir())
    verify_cmd = [c for c in stub.calls if c[0] == "eim"][0][2]
    assert str(archive / "app.elf") in verify_cmd, "verifier must run on the ARCHIVED ELF"
    esptool = [c for c in stub.calls if c[0] != "eim"][0]
    assert "write-flash" in esptool and all(str(archive) in a for a in esptool if a.endswith(".bin"))
    assert (archive / "esptool.log").exists() and (archive / "esptool-command.txt").exists()


def test_stale_image_is_rejected(tmp_path, monkeypatch, capsys):
    build = make_build(tmp_path, b"ELF-A", embed_hash=hashlib.sha256(b"ELF-OLD").digest())
    stub = Stub()
    assert run_session(tmp_path, monkeypatch, build, stub) == 2
    assert "stale or mismatched build" in capsys.readouterr().out
    assert not [c for c in stub.calls if c[0] != "eim"], "esptool must not run"


def test_failed_verification_blocks_flash(tmp_path, monkeypatch, capsys):
    build = make_build(tmp_path, b"ELF-A")
    stub = Stub(ok=False)
    assert run_session(tmp_path, monkeypatch, build, stub) == 2
    assert not [c for c in stub.calls if c[0] != "eim"]


def test_missing_flash_file_is_rejected(tmp_path, monkeypatch, capsys):
    build = make_build(tmp_path, b"ELF-A")
    fa = json.loads((build / "flasher_args.json").read_text())
    fa["flash_files"]["0x20000"] = "extra/missing.bin"
    (build / "flasher_args.json").write_text(json.dumps(fa))
    stub = Stub()
    assert run_session(tmp_path, monkeypatch, build, stub) == 2
    assert "missing from the archive" in capsys.readouterr().out
