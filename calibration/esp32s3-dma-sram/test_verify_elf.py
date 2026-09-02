from __future__ import annotations

import importlib.util
import sys
from pathlib import Path

import pytest


ROOT = Path(__file__).resolve().parent
SPEC = importlib.util.spec_from_file_location("dma_verify", ROOT / "verify_elf.py")
assert SPEC is not None and SPEC.loader is not None
MODULE = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = MODULE
SPEC.loader.exec_module(MODULE)


def fixture(encodings: list[str] | None = None, address: int = 0x40378000) -> tuple[str, str]:
    encodings = encodings or MODULE.EXPECTED_ENCODINGS
    mnemonics = MODULE.EXPECTED_MNEMONICS
    operands = [
        "a1, 16",
        "a4, 1",
        "a4, a4, 13",
        "a6",
        "a5, a3, 0",
        "a5, a2, 0",
        "a2, a2, 4",
        "a3, a3, 4",
        "a4, a4, -1",
        f"a4, {address + 12:08x} <dma_sram_copy_32k+0xc>",
        "a2",
        "a2, a2, a6",
        "",
    ]
    lines = [f"{address:08x} <dma_sram_copy_32k>:"]
    current = address
    for encoding, mnemonic, operand in zip(encodings, mnemonics, operands, strict=True):
        lines.append(f"{current:08x}: {encoding:<8} {mnemonic} {operand}")
        current += len(encoding) // 2
    symbols = f"{address:08x} g .iram0.text 00000021 dma_sram_copy_32k\n"
    return "\n".join(lines), symbols


def test_exact_kernel_passes() -> None:
    disassembly, symbols = fixture()
    result = MODULE.verify(disassembly, symbols)
    assert result["copyKernel"]["iterations"] == 8192
    assert result["copyKernel"]["section"] == ".iram0.text"


def test_encoding_change_fails() -> None:
    encodings = list(MODULE.EXPECTED_ENCODINGS)
    encodings[4] = "f03d"
    disassembly, symbols = fixture(encodings)
    with pytest.raises(MODULE.VerificationError, match="encoding mismatch"):
        MODULE.verify(disassembly, symbols)


def test_alignment_change_fails() -> None:
    disassembly, symbols = fixture(address=0x40378002)
    with pytest.raises(MODULE.VerificationError, match="four-byte aligned"):
        MODULE.verify(disassembly, symbols)


def test_non_iram_symbol_fails() -> None:
    disassembly, symbols = fixture()
    with pytest.raises(MODULE.VerificationError, match="expected .iram0.text"):
        MODULE.verify(disassembly, symbols.replace(".iram0.text", ".flash.text"))
