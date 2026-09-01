from __future__ import annotations

import importlib.util
import sys
from pathlib import Path

import pytest


MODULE_PATH = Path(__file__).with_name("verify_elf.py")
SPEC = importlib.util.spec_from_file_location("register_blocks_verify_elf", MODULE_PATH)
assert SPEC is not None and SPEC.loader is not None
MODULE = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = MODULE
SPEC.loader.exec_module(MODULE)


def fixture() -> str:
    lines: list[str] = []
    address = 0x40370000
    for kind in ("read", "write"):
        access = "002282" if kind == "read" else "006232"
        mnemonic = "l32i" if kind == "read" else "s32i"
        operands = "a8, a2, 0" if kind == "read" else "a3, a2, 0"
        for operations in MODULE.RUN_LENGTHS:
            address = (address + 3) & ~3
            lines.append(f"{address:08x} <register_{kind}_{operations}>:")
            encodings = [
                ("002136", "entry", "a1, 16"),
                ("03ea90", "rsr.ccount", "a9"),
                *([(access, mnemonic, operands)] * operations),
                ("03ea20", "rsr.ccount", "a2"),
                ("c02290", "sub", "a2, a2, a9"),
                ("f01d", "retw.n", ""),
            ]
            for encoding, name, args in encodings:
                lines.append(f"{address:08x}: {encoding} {name} {args}")
                address += len(encoding) // 2
    lines.extend(
        [
            "4037f000 <measure_once>:",
            "4037f000: 002136 entry a1, 16",
            "4037f003: 000081 l32r a8, 4037eff0 (600c40c4)",
            "4037f006: 08c9 s32i.n a12, a8, 0",
            "4037f008: 0008e0 callx8 a8",
            "4037f00b: f01d retw.n",
        ]
    )
    return "\n".join(lines)


def test_all_ladders_have_exact_access_encodings_and_alignment() -> None:
    result = MODULE.verify(fixture())
    assert result["ok"] is True
    assert len(result["accessBlocks"]) == 12
    assert {block["operations"] for block in result["accessBlocks"]} == {
        1,
        2,
        4,
        8,
        16,
        256,
    }


def test_changed_access_encoding_fails_closed() -> None:
    broken = fixture().replace("002282 l32i", "f03d nop.n", 1)
    with pytest.raises(MODULE.VerificationError, match="encoding mismatch"):
        MODULE.verify(broken)


def test_misaligned_block_fails_closed() -> None:
    broken = fixture().replace(
        "40370000 <register_read_1>:", "40370001 <register_read_1>:", 1
    )
    broken = broken.replace("40370000: 002136", "40370001: 002136", 1)
    with pytest.raises(MODULE.VerificationError, match="not 4-byte aligned"):
        MODULE.verify(broken)


def test_measurement_boundary_must_remain_in_iram() -> None:
    broken = fixture().replace("4037f000 <measure_once>:", "4200f000 <measure_once>:")
    broken = broken.replace("4037f000: 002136", "4200f000: 002136")
    with pytest.raises(MODULE.VerificationError, match="not in IRAM"):
        MODULE.verify(broken)


def test_measurement_boundary_rejects_direct_flash_calls() -> None:
    broken = fixture().replace(
        "4037f008: 0008e0 callx8 a8",
        "4037f008: 000005 call8 42001000 <flash_helper>",
    )
    with pytest.raises(MODULE.VerificationError, match="calls outside IRAM"):
        MODULE.verify(broken)


def test_measurement_boundary_rejects_dispatch_reload_after_counter_clear() -> None:
    broken = fixture().replace(
        "4037f008: 0008e0 callx8 a8",
        "4037f008: 2288 l32i.n a8, a2, 8\n4037f00a: 0008e0 callx8 a8",
    )
    with pytest.raises(MODULE.VerificationError, match="reloads access dispatch"):
        MODULE.verify(broken)
