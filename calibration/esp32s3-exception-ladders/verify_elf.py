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
)
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
    return {"cells": list(ids), "samplesPerCell": 100, "knownTerms": expected_terms}


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


def verify_elf(elf_path: Path, manifest_path: Path) -> dict[str, object]:
    manifest = verify_manifest(manifest_path)
    objdump = subprocess.run(
        ["xtensa-esp32s3-elf-objdump", "-d", str(elf_path)],
        check=True,
        capture_output=True,
        text=True,
    ).stdout
    disassembly = verify_disassembly(objdump)
    return {
        "elfSha256": hashlib.sha256(elf_path.read_bytes()).hexdigest(),
        "manifest": manifest,
        **disassembly,
    }


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("elf", type=Path)
    parser.add_argument("output", type=Path)
    args = parser.parse_args()
    result = verify_elf(args.elf, Path(__file__).with_name("probe-cells.json"))
    args.output.write_text(json.dumps(result, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
