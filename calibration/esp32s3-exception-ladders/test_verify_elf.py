from __future__ import annotations

import hashlib
import importlib.util
import json
import subprocess
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
    "mask_rom_fetch_straight_line",
)
EXCEPTION_CELL_IDS = CELL_IDS[:-1]


def manifest(tmp_path: Path, *, samples: int = 100) -> Path:
    path = tmp_path / "probe-cells.json"
    path.write_text(
        json.dumps(
            {
                "protocolVersion": 2,
                "harnessVersion": "1.2.0",
                "chipModel": "ESP32-S3",
                "chipRevision": 2,
                "runtimeConfiguration": dict(MODULE.EXPECTED_RUNTIME_CONFIGURATION),
                "cells": [
                    {
                        "id": cell,
                        "family": (
                            "instruction-fetch"
                            if cell == "mask_rom_fetch_straight_line"
                            else "exception"
                        ),
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
                            else (
                                {"knownTerms": ["entry", "retw.n"]}
                                if cell == "mask_rom_fetch_straight_line"
                                else {}
                            )
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
40370203: c22203 addi a2, a2, 3
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
40370500: 0139 s32i.n a3, a1, 0
40370502: 1149 s32i.n a4, a1, 4
40370504: 0002e0 callx8 a2
40370507: 000081 l32r a8, 403704f0 (600c40c4)
4037050a: 390c movi.n a9, 3
4037050c: 0020c0 memw
4037050f: 006892 s32i a9, a8, 0
40370512: 006f50 rsil a5, 15
40370515: 03ea40 rsr.ccount a4
40370518: 0021a2 l32i a10, a1, 0
4037051b: 0003e0 callx8 a3
4037051e: 0000a1 l32r a10, 403704f4 (600c40c8)
40370521: 000081 l32r a8, 403704f8 (600c40cc)
40370524: 0888 l32i.n a8, a8, 0
40370526: 0aa8 l32i.n a10, a10, 0
40370528: 80aa or a8, a8, a10
4037052b: 0000a1 l32r a10, 403704fc (600c40d8)
4037052e: 0aa8 l32i.n a10, a10, 0
40370530: 80aa or a8, a8, a10
40370533: 0000b1 l32r a11, 403704f0 (600c40d0)
40370536: 0bb8 l32i.n a11, a11, 0
40370538: 80ba or a8, a8, a11
4037053b: 0000a1 l32r a10, 403704ec (600c40d4)
4037053e: 0aa8 l32i.n a10, a10, 0
40370540: 80aa or a8, a8, a10
40370543: c09940 sub a9, a9, a4
40370546: ee18 bnez.n a8, 40370556
40370548: ec09 beqz.n a9, 40370556
4037054a: 0199 s32i.n a9, a1, 0
4037054c: 1b0c movi.n a11, 1
40370556: f01d retw.n
"""


def test_manifest_requires_exact_h1_cells_and_100_samples(tmp_path: Path) -> None:
    result = MODULE.verify_manifest(manifest(tmp_path))
    assert tuple(result["cells"]) == CELL_IDS
    assert result["samplesPerCell"] == 100


def test_manifest_rejects_a_non_100_sample_cell(tmp_path: Path) -> None:
    with pytest.raises(MODULE.VerificationError, match="100 samples"):
        MODULE.verify_manifest(manifest(tmp_path, samples=99))


@pytest.mark.parametrize(
    ("field", "value", "message"),
    [
        ("protocolVersion", 1, "protocolVersion"),
        ("harnessVersion", "1.1.0", "harnessVersion"),
        ("chipModel", "ESP32-C3", "chipModel"),
        ("chipRevision", 1, "chipRevision"),
    ],
)
def test_manifest_rejects_changed_configuration_identity(
    tmp_path: Path, field: str, value: object, message: str
) -> None:
    path = manifest(tmp_path)
    payload = json.loads(path.read_text())
    payload[field] = value
    path.write_text(json.dumps(payload))
    with pytest.raises(MODULE.VerificationError, match=message):
        MODULE.verify_manifest(path)


def test_manifest_rejects_changed_variants(tmp_path: Path) -> None:
    path = manifest(tmp_path)
    payload = json.loads(path.read_text())
    payload["cells"][0]["variants"] = ["normal", "xip-psram"]
    path.write_text(json.dumps(payload))
    with pytest.raises(MODULE.VerificationError, match="normal variant"):
        MODULE.verify_manifest(path)


def test_manifest_rejects_changed_cell_family(tmp_path: Path) -> None:
    path = manifest(tmp_path)
    payload = json.loads(path.read_text())
    payload["cells"][0]["family"] = "instruction-fetch"
    path.write_text(json.dumps(payload))
    with pytest.raises(MODULE.VerificationError, match="families"):
        MODULE.verify_manifest(path)


def test_manifest_rejects_changed_runtime_configuration(tmp_path: Path) -> None:
    path = manifest(tmp_path)
    payload = json.loads(path.read_text())
    payload["runtimeConfiguration"]["cpuHz"] = 160_000_000
    path.write_text(json.dumps(payload))
    with pytest.raises(MODULE.VerificationError, match="runtimeConfiguration"):
        MODULE.verify_manifest(path)


def test_verified_encodings_cover_all_six_exception_cells() -> None:
    result = MODULE.verify_disassembly(disassembly())
    assert set(result["cells"]) == set(EXCEPTION_CELL_IDS)
    assert result["cells"]["call4_window_pair"]["callMnemonic"] == "call4"
    assert result["cells"]["call8_window_pair"]["callMnemonic"] == "call8"
    assert result["cells"]["call12_window_pair"]["callMnemonic"] == "call12"
    assert result["cells"]["syscall_rfe_pair"]["knownTerms"] == [
        "rsr.epc1",
        "addi",
        "wsr.epc1",
        "rsync",
        "rfe",
    ]
    assert result["cells"]["syscall_rfe_pair"]["handlerEncodings"][-1] == "003000"
    assert result["cells"]["rfe_alone"]["returnEncoding"] == "003000"
    assert result["cells"]["rfi3_alone"]["returnEncoding"] == "003310"
    assert result["measurementBoundary"]["cacheCountersRequiredZero"] is True
    assert len(result["measurementBoundary"]["cacheCounterReadAddresses"]) == 5


def test_changed_return_encoding_fails_closed() -> None:
    broken = disassembly().replace("40370406: 003310 rfi 3", "40370406: f03d nop.n")
    with pytest.raises(MODULE.VerificationError, match="rfi 3"):
        MODULE.verify_disassembly(broken)


def test_measurement_boundary_requires_counter_clear_before_dispatch() -> None:
    broken = disassembly().replace(
        "4037050f: 006892 s32i a9, a8, 0\n", ""
    )
    with pytest.raises(MODULE.VerificationError, match="exact cache-counter clear"):
        MODULE.verify_disassembly(broken)


def test_measurement_boundary_rejects_counter_clear_after_dispatch() -> None:
    clear = "4037050f: 006892 s32i a9, a8, 0\n"
    broken = disassembly().replace(clear, "").replace(
        "4037051b: 0003e0 callx8 a3\n",
        "4037051b: 0003e0 callx8 a3\n4037051e: 006892 s32i a9, a8, 0\n",
    )
    with pytest.raises(MODULE.VerificationError, match="exact cache-counter clear"):
        MODULE.verify_disassembly(broken)


def test_measurement_boundary_requires_every_post_dispatch_counter_read() -> None:
    broken = disassembly().replace("4037053e: 0aa8 l32i.n a10, a10, 0\n", "")
    with pytest.raises(MODULE.VerificationError, match="all post-dispatch"):
        MODULE.verify_disassembly(broken)


def test_measurement_boundary_rejects_counter_read_after_zero_gate() -> None:
    read = "4037053e: 0aa8 l32i.n a10, a10, 0\n"
    broken = disassembly().replace(read, "").replace(
        "40370546: ee18 bnez.n a8, 40370556\n",
        "40370546: ee18 bnez.n a8, 40370556\n" + read,
    )
    with pytest.raises(MODULE.VerificationError, match="all post-dispatch"):
        MODULE.verify_disassembly(broken)


def test_measurement_boundary_requires_cache_zero_gate() -> None:
    broken = disassembly().replace("40370546: ee18 bnez.n a8, 40370556\n", "")
    with pytest.raises(MODULE.VerificationError, match="zero gate"):
        MODULE.verify_disassembly(broken)


def test_measurement_boundary_rejects_cache_gate_after_acceptance_store() -> None:
    gate = "40370546: ee18 bnez.n a8, 40370556\n"
    broken = disassembly().replace(gate, "").replace(
        "4037054a: 0199 s32i.n a9, a1, 0\n",
        "4037054a: 0199 s32i.n a9, a1, 0\n" + gate,
    )
    with pytest.raises(MODULE.VerificationError, match="gate"):
        MODULE.verify_disassembly(broken)


def test_measurement_boundary_requires_conditional_accepted_store() -> None:
    broken = disassembly().replace("4037054a: 0199 s32i.n a9, a1, 0\n", "")
    with pytest.raises(MODULE.VerificationError, match="accepted-sample store"):
        MODULE.verify_disassembly(broken)


def test_measurement_boundary_requires_zero_elapsed_gate() -> None:
    broken = disassembly().replace("40370548: ec09 beqz.n a9, 40370556\n", "")
    with pytest.raises(MODULE.VerificationError, match="zero-elapsed gate"):
        MODULE.verify_disassembly(broken)


def app_symbols() -> str:
    return "400559a4 g       *ABS* 00000000 mask_rom_fetch_straight_line\n"


def rom_symbols() -> str:
    return "400559a4 g     F .text 00000005 xtos_p_none\n"


def rom_disassembly() -> str:
    return """
400559a4 <xtos_p_none>:
400559a4: 002136 entry a1, 16
400559a7: f01d retw.n
400559a9: 000000 ill
400559ac <xtos_unhandled_interrupt>:
400559ac: 002136 entry a1, 16
"""


def test_mask_rom_contract_proves_placement_and_instruction_encodings() -> None:
    result = MODULE.verify_rom_contract(
        app_symbols(), rom_symbols(), rom_disassembly()
    )
    assert result["address"] == "0x400559a4"
    assert result["section"] == ".text"
    assert result["instructionFetchesPerTrial"] == 2
    assert "cacheCountersRequiredZero" not in result
    assert [instruction["encoding"] for instruction in result["instructions"]] == [
        "002136",
        "f01d",
    ]


def test_mask_rom_contract_rejects_a_non_rom_alias() -> None:
    broken = app_symbols().replace("400559a4", "403559a4")
    with pytest.raises(MODULE.VerificationError, match="absolute ROM target"):
        MODULE.verify_rom_contract(broken, rom_symbols(), rom_disassembly())


def test_mask_rom_contract_rejects_changed_rom_encoding() -> None:
    broken = rom_disassembly().replace("400559a7: f01d retw.n", "400559a7: f03d nop.n")
    with pytest.raises(MODULE.VerificationError, match="instruction pair"):
        MODULE.verify_rom_contract(app_symbols(), rom_symbols(), broken)


def test_rom_elf_resolves_from_capture_environment(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    monkeypatch.setenv("ESP_ROM_ELF_DIR", str(tmp_path))
    assert MODULE.resolve_rom_elf(None) == tmp_path / "esp32s3_rev0_rom.elf"


def test_artifact_manifest_pins_every_executable_input(tmp_path: Path) -> None:
    build = tmp_path / "build"
    (build / "bootloader").mkdir(parents=True)
    (build / "partition_table").mkdir()
    paths = {
        build / "probe.elf": b"elf",
        build / "probe.bin": b"app",
        build / "bootloader" / "bootloader.bin": b"boot",
        build / "partition_table" / "partition-table.bin": b"partitions",
        build / "sdkconfig": b"config",
        build / "flash_args": b"generated arguments",
        tmp_path / "probe-cells.json": b"manifest",
        tmp_path / "esp32s3_rev0_rom.elf": b"rom",
    }
    for path, contents in paths.items():
        path.write_bytes(contents)
    (build / "flasher_args.json").write_text(
        json.dumps(
            {
                "flash_files": {
                    "0x10000": "probe.bin",
                    "0x0": "bootloader/bootloader.bin",
                    "0x8000": "partition_table/partition-table.bin",
                },
                "flash_settings": {"flash_mode": "dio"},
                "extra_esptool_args": {"chip": "esp32s3"},
            }
        )
    )

    result = MODULE.verify_artifacts(
        build / "probe.elf",
        tmp_path / "probe-cells.json",
        tmp_path / "esp32s3_rev0_rom.elf",
    )

    assert set(result["artifacts"]) == {
        "applicationElf",
        "applicationBinary",
        "bootloaderBinary",
        "partitionTableBinary",
        "sdkconfig",
        "flasherArguments",
        "flashArguments",
        "probeManifest",
        "maskRomElf",
    }
    assert result["artifacts"]["applicationBinary"]["sha256"] == hashlib.sha256(
        b"app"
    ).hexdigest()
    assert result["flashLayout"] == [
        {
            "offset": "0x0",
            "artifact": "bootloaderBinary",
            "path": "bootloader/bootloader.bin",
        },
        {
            "offset": "0x8000",
            "artifact": "partitionTableBinary",
            "path": "partition_table/partition-table.bin",
        },
        {"offset": "0x10000", "artifact": "applicationBinary", "path": "probe.bin"},
    ]
    assert result["flashSettings"] == {"flash_mode": "dio"}
    assert result["esptoolArguments"] == {"chip": "esp32s3"}


def test_capture_objdump_argument_is_accepted(tmp_path: Path) -> None:
    result = tmp_path / "verification.json"
    process = subprocess.run(
        [
            sys.executable,
            str(SCRIPT),
            str(tmp_path / "missing.elf"),
            str(result),
            "--objdump",
            "/capture/toolchain/xtensa-esp32s3-elf-objdump",
        ],
        text=True,
        capture_output=True,
        check=False,
    )
    assert process.returncode == 2
    assert "ELF verification failed:" in process.stderr
    assert "unrecognized arguments" not in process.stderr
    assert not result.exists()
