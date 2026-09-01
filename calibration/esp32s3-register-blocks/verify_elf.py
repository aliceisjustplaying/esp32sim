#!/usr/bin/env python3
"""Verify register-block access ladders from an ESP32-S3 ELF."""

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
RUN_LENGTHS = (1, 2, 4, 8, 16, 256)


class VerificationError(ValueError):
    pass


@dataclass(frozen=True)
class Instruction:
    address: int
    encoding: str
    mnemonic: str
    operands: str


def parse_disassembly(text: str) -> dict[str, list[Instruction]]:
    functions: dict[str, list[Instruction]] = {}
    current: list[Instruction] | None = None
    for line in text.splitlines():
        function = FUNCTION.match(line.strip())
        if function:
            current = []
            functions[function.group(2)] = current
            continue
        instruction = INSTRUCTION.match(line)
        if instruction and current is not None:
            current.append(
                Instruction(
                    address=int(instruction.group(1), 16),
                    encoding=instruction.group(2).lower(),
                    mnemonic=instruction.group(3),
                    operands=(instruction.group(4) or "").strip(),
                )
            )
    return functions


def verify_block(
    functions: dict[str, list[Instruction]], kind: str, operations: int
) -> dict[str, int | str]:
    symbol = f"register_{kind}_{operations}"
    instructions = functions.get(symbol)
    if not instructions:
        raise VerificationError(f"missing disassembly for {symbol}")
    if instructions[0].address % 4 != 0:
        raise VerificationError(f"{symbol} is not 4-byte aligned")
    access = "002282" if kind == "read" else "006232"
    expected = [
        "002136",
        "03ea90",
        *([access] * operations),
        "03ea20",
        "c02290",
        "f01d",
    ]
    encodings = [instruction.encoding for instruction in instructions[: len(expected)]]
    if encodings != expected:
        mismatch = next(
            (
                index
                for index, pair in enumerate(zip(encodings, expected, strict=False))
                if pair[0] != pair[1]
            ),
            min(len(encodings), len(expected)),
        )
        raise VerificationError(
            f"{symbol} encoding mismatch at instruction {mismatch}: "
            f"got {encodings[mismatch:mismatch + 1]}, "
            f"expected {expected[mismatch:mismatch + 1]}"
        )
    body = instructions[2 : 2 + operations]
    return {
        "symbol": symbol,
        "address": instructions[0].address,
        "alignment": 4,
        "operations": operations,
        "accessEncoding": access,
        "bodySha256": hashlib.sha256(
            bytes.fromhex("".join(instruction.encoding for instruction in body))
        ).hexdigest(),
    }


def verify(disassembly: str) -> dict[str, object]:
    functions = parse_disassembly(disassembly)
    blocks = [
        verify_block(functions, kind, operations)
        for kind in ("read", "write")
        for operations in RUN_LENGTHS
    ]
    return {"ok": True, "accessBlocks": blocks}


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("elf", type=Path)
    parser.add_argument("result", type=Path)
    parser.add_argument("--objdump", default="xtensa-esp32s3-elf-objdump")
    parser.add_argument("--disassembly", type=Path, help=argparse.SUPPRESS)
    args = parser.parse_args()
    if args.result.exists():
        print(f"refusing to overwrite result: {args.result}", file=sys.stderr)
        return 2
    try:
        if args.disassembly is not None:
            disassembly = args.disassembly.read_text()
        else:
            disassembly = subprocess.run(
                [args.objdump, "-d", str(args.elf)],
                check=True,
                text=True,
                capture_output=True,
            ).stdout
        result = verify(disassembly)
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
