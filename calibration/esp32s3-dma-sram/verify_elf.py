#!/usr/bin/env python3
"""Verify the exact DMA-on-SRAM copy kernel in a built ELF."""

from __future__ import annotations

import argparse
import hashlib
import json
import re
import subprocess
import sys
from dataclasses import dataclass
from pathlib import Path


FUNCTION = re.compile(r"^([0-9a-fA-F]+) <([^>]+)>:$")
INSTRUCTION = re.compile(
    r"^\s*([0-9a-fA-F]+):\s+([0-9a-fA-F]+)\s+([a-zA-Z0-9_.]+)(?:\s+(.*))?$"
)
EXPECTED_ENCODINGS = [
    "002136",
    "01a042",
    "114430",
    "03ea60",
    "0358",
    "0259",
    "224b",
    "334b",
    "440b",
    "ff2456",
    "03ea20",
    "c02260",
    "f01d",
]
EXPECTED_MNEMONICS = [
    "entry",
    "movi",
    "slli",
    "rsr.ccount",
    "l32i.n",
    "s32i.n",
    "addi.n",
    "addi.n",
    "addi.n",
    "bnez",
    "rsr.ccount",
    "sub",
    "retw.n",
]


class VerificationError(ValueError):
    pass


@dataclass(frozen=True)
class Instruction:
    address: int
    encoding: str
    mnemonic: str
    operands: str


def parse_function(text: str, name: str) -> list[Instruction]:
    current = False
    instructions: list[Instruction] = []
    for line in text.splitlines():
        function = FUNCTION.match(line.strip())
        if function:
            if current:
                break
            current = function.group(2) == name
            continue
        instruction = INSTRUCTION.match(line)
        if current and instruction:
            instructions.append(
                Instruction(
                    address=int(instruction.group(1), 16),
                    encoding=instruction.group(2).lower(),
                    mnemonic=instruction.group(3),
                    operands=(instruction.group(4) or "").strip(),
                )
            )
    if not instructions:
        raise VerificationError(f"missing disassembly for {name}")
    return instructions


def parse_symbol_section(text: str, name: str) -> str:
    matches = []
    for line in text.splitlines():
        fields = line.split()
        if fields and fields[-1] == name:
            sections = [field for field in fields[1:-1] if field.startswith(".")]
            if len(sections) == 1:
                matches.append(sections[0])
    if len(matches) != 1:
        raise VerificationError(f"expected one ELF symbol {name}, found {len(matches)}")
    return matches[0]


def verify(disassembly: str, symbols: str) -> dict[str, object]:
    name = "dma_sram_copy_32k"
    instructions = parse_function(disassembly, name)[: len(EXPECTED_ENCODINGS)]
    encodings = [instruction.encoding for instruction in instructions]
    mnemonics = [instruction.mnemonic for instruction in instructions]
    if encodings != EXPECTED_ENCODINGS or mnemonics != EXPECTED_MNEMONICS:
        raise VerificationError(
            f"{name} encoding mismatch: got {encodings}, expected {EXPECTED_ENCODINGS}"
        )
    if instructions[0].address % 4 != 0:
        raise VerificationError(f"{name} is not four-byte aligned")
    if instructions[9].operands.split(",", 1)[0] != "a4":
        raise VerificationError(f"{name} loop branch does not use the 8192-word counter")
    target = int(instructions[9].operands.split(",", 1)[1].strip().split()[0], 16)
    if target != instructions[4].address:
        raise VerificationError(f"{name} loop branch target is not the exact load")
    section = parse_symbol_section(symbols, name)
    if section != ".iram0.text":
        raise VerificationError(f"{name} is in {section}, expected .iram0.text")
    body = b"".join(bytes.fromhex(item) for item in encodings)
    return {
        "ok": True,
        "copyKernel": {
            "symbol": name,
            "section": section,
            "address": instructions[0].address,
            "alignmentBytes": 4,
            "bytesPerIteration": 4,
            "iterations": 8192,
            "spanBytes": sum(len(item) // 2 for item in encodings),
            "encodingSha256": hashlib.sha256(body).hexdigest(),
            "encodings": encodings,
        },
    }


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("elf", type=Path)
    parser.add_argument("result", type=Path)
    parser.add_argument("--objdump", default="xtensa-esp32s3-elf-objdump")
    parser.add_argument("--disassembly", type=Path, help=argparse.SUPPRESS)
    parser.add_argument("--symbols", type=Path, help=argparse.SUPPRESS)
    args = parser.parse_args()
    if args.result.exists():
        print(f"refusing to overwrite result: {args.result}", file=sys.stderr)
        return 2
    try:
        if args.disassembly is not None and args.symbols is not None:
            disassembly = args.disassembly.read_text()
            symbols = args.symbols.read_text()
        elif args.disassembly is not None or args.symbols is not None:
            raise VerificationError("fixture disassembly and symbols must appear together")
        else:
            disassembly = subprocess.run(
                [args.objdump, "-d", str(args.elf)],
                check=True,
                text=True,
                capture_output=True,
            ).stdout
            symbols = subprocess.run(
                [args.objdump, "-t", str(args.elf)],
                check=True,
                text=True,
                capture_output=True,
            ).stdout
        result = verify(disassembly, symbols)
        result["elf"] = str(args.elf)
        result["elfSha256"] = hashlib.sha256(args.elf.read_bytes()).hexdigest()
    except (OSError, subprocess.CalledProcessError, VerificationError) as error:
        print(f"ELF verification failed: {error}", file=sys.stderr)
        return 2
    args.result.parent.mkdir(parents=True, exist_ok=True)
    args.result.write_text(json.dumps(result, indent=2, sort_keys=True) + "\n")
    print(f"ELF verification passed: {args.result}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
