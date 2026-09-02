from __future__ import annotations

import importlib.util
import json
import sys
from pathlib import Path

import pytest


SCRIPT = Path(__file__).with_name("verify_elf.py")
SPEC = importlib.util.spec_from_file_location("exception_ladders_verify_elf", SCRIPT)
assert SPEC is not None and SPEC.loader is not None
MODULE = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = MODULE
SPEC.loader.exec_module(MODULE)


CELL_IDS = (
    "call4_window_pair",
    "call8_window_pair",
    "call12_window_pair",
    "syscall_rfe_pair",
    "rfe_alone",
    "rfi3_alone",
)


def manifest(tmp_path: Path, *, samples: int = 100) -> Path:
    path = tmp_path / "probe-cells.json"
    path.write_text(
        json.dumps(
            {
                "protocolVersion": 2,
                "harnessVersion": "1.0.0",
                "chipModel": "ESP32-S3",
                "chipRevision": 2,
                "cells": [
                    {
                        "id": cell,
                        "family": "exception",
                        "samples": samples,
                        "variants": ["normal"],
                        **(
                            {
                                "knownTerms": [
                                    "rsr.epc1",
                                    "addi",
                                    "wsr.epc1",
                                    "rsync",
                                    "rfe",
                                ]
                            }
                            if cell == "syscall_rfe_pair"
                            else {}
                        ),
                    }
                    for cell in CELL_IDS
                ],
            }
        )
    )
    return path


def disassembly() -> str:
    return """
40370000 <call4_window_pair>:
40370000: 004136 entry a1, 32
40370003: ffffc5 call4 40370000 <call4_window_pair>
40370006: 1df0 retw.n
40370010 <call8_window_pair>:
40370010: 004136 entry a1, 32
40370013: ffffc5 call8 40370010 <call8_window_pair>
40370016: 1df0 retw.n
40370020 <call12_window_pair>:
40370020: 004136 entry a1, 32
40370023: ffffc5 call12 40370020 <call12_window_pair>
40370026: 1df0 retw.n
40370100 <syscall_rfe_pair>:
40370100: 0036f0 entry a1, 32
40370103: 005000 syscall
40370106: 1df0 retw.n
40370200 <exception_level1_handler>:
40370200: 03e620 rsr.epc1 a2
40370203: 223b addi.n a2, a2, 3
40370205: 13e620 wsr.epc1 a2
40370208: 002000 rsync
4037020b: 003000 rfe
40370300 <rfe_alone>:
40370300: 001761 wsr.epc1 a7
40370303: 03ea20 rsr.ccount a2
40370306: 003000 rfe
40370309: 03ea30 rsr.ccount a3
4037030c: 1df0 retw.n
40370400 <rfi3_alone>:
40370400: 003763 wsr.epc3 a7
40370403: 03ea20 rsr.ccount a2
40370406: 003310 rfi 3
40370409: 03ea30 rsr.ccount a3
4037040c: 1df0 retw.n
40370500 <measure_exception_sample>:
40370500: 000081 l32r a8, 403704f0 (600c40c4)
40370503: 08c9 s32i.n a12, a8, 0
40370505: 0008e0 callx8 a8
40370508: 000091 l32r a9, 403704f4 (600c40cc)
4037050b: 0998 l32i.n a9, a9, 0
4037050d: f01d retw.n
"""


def test_manifest_requires_exact_h1_cells_and_100_samples(tmp_path: Path) -> None:
    result = MODULE.verify_manifest(manifest(tmp_path))
    assert tuple(result["cells"]) == CELL_IDS
    assert result["samplesPerCell"] == 100


def test_manifest_rejects_a_non_100_sample_cell(tmp_path: Path) -> None:
    with pytest.raises(MODULE.VerificationError, match="100 samples"):
        MODULE.verify_manifest(manifest(tmp_path, samples=99))


def test_verified_encodings_cover_all_six_h1_cells() -> None:
    result = MODULE.verify_disassembly(disassembly())
    assert set(result["cells"]) == set(CELL_IDS)
    assert result["cells"]["call4_window_pair"]["callMnemonic"] == "call4"
    assert result["cells"]["call8_window_pair"]["callMnemonic"] == "call8"
    assert result["cells"]["call12_window_pair"]["callMnemonic"] == "call12"
    assert result["cells"]["syscall_rfe_pair"]["knownTerms"] == [
        "rsr.epc1",
        "addi.n",
        "wsr.epc1",
        "rsync",
        "rfe",
    ]
    assert result["cells"]["syscall_rfe_pair"]["handlerEncodings"][-1] == "003000"
    assert result["cells"]["rfe_alone"]["returnEncoding"] == "003000"
    assert result["cells"]["rfi3_alone"]["returnEncoding"] == "003310"


def test_changed_return_encoding_fails_closed() -> None:
    broken = disassembly().replace("40370406: 003310 rfi 3", "40370406: f03d nop.n")
    with pytest.raises(MODULE.VerificationError, match="rfi 3"):
        MODULE.verify_disassembly(broken)


def test_measurement_boundary_requires_counter_clear_before_dispatch() -> None:
    broken = disassembly().replace(
        "40370503: 08c9 s32i.n a12, a8, 0\n", ""
    )
    with pytest.raises(MODULE.VerificationError, match="cache-counter clear"):
        MODULE.verify_disassembly(broken)
