#!/usr/bin/env python3
"""Verify the H1 exception ladder manifest and ELF instruction contract."""

from __future__ import annotations

import argparse
import hashlib
import json
import re
import subprocess
from pathlib import Path


CELL_IDS = (
    "call4_window_pair",
    "call8_window_pair",
    "call12_window_pair",
    "syscall_rfe_pair",
    "rfe_alone",
    "rfi3_alone",
    "mask_rom_fetch_straight_line",
)
ROM_SHA256 = "c0ce0f338d1de1bdc6efbef1591779a2a42c1ab7d759d3c6ae8ae63a7dd34cfd"
ROM_TARGET = 0x400559A4
HEADER = re.compile(r"^([0-9a-f]+) <([^>]+)>:$")
INSN = re.compile(r"^([0-9a-f]+):\s+([0-9a-f]+)\s+([a-zA-Z0-9_.]+)(?:\s+(.*))?$")


class VerificationError(ValueError):
    pass


def verify_manifest(path: Path) -> dict[str, object]:
    payload = json.loads(path.read_text(encoding="utf-8"))
    cells = payload.get("cells")
    if not isinstance(cells, list):
        raise VerificationError("manifest cells must be an array")
    ids = tuple(cell.get("id") for cell in cells)
    if ids != CELL_IDS:
        raise VerificationError("manifest must contain the exact ordered H1 cells")
    if any(cell.get("samples") != 100 for cell in cells):
        raise VerificationError("every H1 cell must request 100 samples")
    syscall = cells[3]
    expected_terms = ["rsr.epc1", "addi", "wsr.epc1", "rsync", "rfe"]
    if syscall.get("knownTerms") != expected_terms:
        raise VerificationError("syscall cell must record the five known handler terms")
    rom_terms = ["entry", "retw.n"]
    if cells[6].get("family") != "instruction-fetch":
        raise VerificationError("mask-ROM cell must be an instruction-fetch cell")
    if cells[6].get("knownTerms") != rom_terms:
        raise VerificationError("mask-ROM cell must record its two straight-line terms")
    return {
        "cells": list(ids),
        "samplesPerCell": 100,
        "knownTerms": expected_terms,
        "maskRomKnownTerms": rom_terms,
    }


def _functions(disassembly: str) -> dict[str, list[tuple[int, str, str, str]]]:
    result: dict[str, list[tuple[int, str, str, str]]] = {}
    current: list[tuple[int, str, str, str]] | None = None
    for raw in disassembly.splitlines():
        line = raw.strip()
        header = HEADER.match(line)
        if header:
            current = result.setdefault(header.group(2), [])
            continue
        if current is None:
            continue
        instruction = INSN.match(line)
        if instruction:
            current.append(
                (
                    int(instruction.group(1), 16),
                    instruction.group(2),
                    instruction.group(3),
                    instruction.group(4) or "",
                )
            )
    return result


def _require(functions: dict[str, list[tuple[int, str, str, str]]], name: str):
    instructions = functions.get(name)
    if not instructions:
        raise VerificationError(f"missing {name} disassembly")
    return instructions


def _symbol(symbols: str, name: str) -> tuple[int, str, int]:
    for raw in symbols.splitlines():
        fields = raw.split()
        if len(fields) >= 5 and fields[-1] == name:
            try:
                return int(fields[0], 16), fields[-3], int(fields[-2], 16)
            except ValueError:
                continue
    raise VerificationError(f"missing {name} symbol")


def verify_rom_contract(
    app_symbols: str, rom_symbols: str, rom_disassembly: str
) -> dict[str, object]:
    alias_address, alias_section, _ = _symbol(
        app_symbols, "mask_rom_fetch_straight_line"
    )
    if alias_address != ROM_TARGET or alias_section != "*ABS*":
        raise VerificationError("mask-ROM probe alias is not the exact absolute ROM target")

    target_address, target_section, target_size = _symbol(rom_symbols, "xtos_p_none")
    if (
        target_address != ROM_TARGET
        or target_section != ".text"
        or target_size != 5
    ):
        raise VerificationError("xtos_p_none is not the expected five-byte ROM text symbol")

    target = [
        instruction
        for instruction in _require(_functions(rom_disassembly), "xtos_p_none")
        if instruction[0] < target_address + target_size
    ]
    expected = (
        (ROM_TARGET, "002136", "entry", "a1, 16"),
        (ROM_TARGET + 3, "f01d", "retw.n", ""),
    )
    if tuple(target) != expected:
        raise VerificationError("xtos_p_none is not the exact straight-line instruction pair")
    return {
        "name": "xtos_p_none",
        "address": f"0x{target_address:08x}",
        "section": target_section,
        "sizeBytes": target_size,
        "instructions": [
            {
                "address": f"0x{address:08x}",
                "encoding": encoding,
                "mnemonic": mnemonic,
                "operands": operands,
            }
            for address, encoding, mnemonic, operands in target
        ],
        "instructionFetchesPerTrial": len(target),
        "cacheCountersRequiredZero": True,
    }


def verify_disassembly(disassembly: str) -> dict[str, object]:
    functions = _functions(disassembly)
    cells: dict[str, object] = {}
    for cell, call in (
        ("call4_window_pair", "call4"),
        ("call8_window_pair", "call8"),
        ("call12_window_pair", "call12"),
    ):
        instructions = _require(functions, cell)
        if not any(mnemonic == call for _, _, mnemonic, _ in instructions):
            raise VerificationError(f"{cell} does not contain {call}")
        if not any(mnemonic.startswith("retw") for _, _, mnemonic, _ in instructions):
            raise VerificationError(f"{cell} is not non-tail recursion")
        cells[cell] = {"callMnemonic": call}

    syscall = _require(functions, "syscall_rfe_pair")
    if not any(mnemonic == "syscall" for _, _, mnemonic, _ in syscall):
        raise VerificationError("syscall_rfe_pair does not contain syscall")
    handler = _require(functions, "exception_level1_handler")
    known = handler[:5]
    expected = ("rsr.epc1", "addi", "wsr.epc1", "rsync", "rfe")
    actual = tuple(mnemonic for _, _, mnemonic, _ in known)
    if actual != expected or len(handler) != 5:
        raise VerificationError("exception handler is not the exact five-term EPC1 adjustment")
    cells["syscall_rfe_pair"] = {
        "knownTerms": list(actual),
        "handlerEncodings": [encoding for _, encoding, _, _ in known],
    }

    rfe = _require(functions, "rfe_alone")
    rfe_instruction = next((item for item in rfe if item[2] == "rfe"), None)
    if rfe_instruction is None:
        raise VerificationError("rfe_alone does not contain rfe")
    cells["rfe_alone"] = {"returnEncoding": rfe_instruction[1]}
    rfi = _require(functions, "rfi3_alone")
    rfi_instruction = next(
        (item for item in rfi if item[2] == "rfi" and item[3].strip() == "3"), None
    )
    if rfi_instruction is None:
        raise VerificationError("rfi3_alone does not contain rfi 3")
    cells["rfi3_alone"] = {"returnEncoding": rfi_instruction[1]}

    measure = next(
        (
            instructions
            for name, instructions in functions.items()
            if name.startswith("measure_probe_samples")
        ),
        None,
    ) or _require(functions, "measure_exception_sample")
    clear = next(
        (
            index
            for index, item in enumerate(measure)
            if "clear_cache_counters" in item[3] or item[2].startswith("s32i")
        ),
        None,
    )
    dispatch = next(
        (index for index, item in enumerate(measure) if item[2].startswith("callx")),
        None,
    )
    if clear is None or dispatch is None or clear >= dispatch:
        raise VerificationError("measurement lacks cache-counter clear before dispatch")
    return {"cells": cells}


def verify_elf(
    elf_path: Path, manifest_path: Path, rom_elf_path: Path
) -> dict[str, object]:
    manifest = verify_manifest(manifest_path)
    rom_bytes = rom_elf_path.read_bytes()
    rom_sha256 = hashlib.sha256(rom_bytes).hexdigest()
    if rom_sha256 != ROM_SHA256:
        raise VerificationError(
            f"mask ROM SHA-256 {rom_sha256} does not match the pinned ESP32-S3 ROM"
        )
    objdump = subprocess.run(
        ["xtensa-esp32s3-elf-objdump", "-d", str(elf_path)],
        check=True,
        capture_output=True,
        text=True,
    ).stdout
    disassembly = verify_disassembly(objdump)
    app_symbols = subprocess.run(
        ["xtensa-esp32s3-elf-objdump", "-t", str(elf_path)],
        check=True,
        capture_output=True,
        text=True,
    ).stdout
    rom_symbols = subprocess.run(
        ["xtensa-esp32s3-elf-objdump", "-t", str(rom_elf_path)],
        check=True,
        capture_output=True,
        text=True,
    ).stdout
    rom_disassembly = subprocess.run(
        ["xtensa-esp32s3-elf-objdump", "-d", str(rom_elf_path)],
        check=True,
        capture_output=True,
        text=True,
    ).stdout
    rom_contract = verify_rom_contract(app_symbols, rom_symbols, rom_disassembly)
    verified_cells = dict(disassembly["cells"])
    verified_cells["mask_rom_fetch_straight_line"] = rom_contract
    return {
        "elfSha256": hashlib.sha256(elf_path.read_bytes()).hexdigest(),
        "romElfSha256": rom_sha256,
        "manifest": manifest,
        **disassembly,
        "cells": verified_cells,
    }


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("elf", type=Path)
    parser.add_argument("output", type=Path)
    parser.add_argument("--rom-elf", required=True, type=Path)
    args = parser.parse_args()
    result = verify_elf(
        args.elf, Path(__file__).with_name("probe-cells.json"), args.rom_elf
    )
    args.output.write_text(json.dumps(result, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
