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
IRAM_START = 0x40370000
IRAM_END = 0x403E0000
CACHE_COUNTER_CTRL = "600c40c4"


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


def verify_measurement_boundary(
    functions: dict[str, list[Instruction]],
) -> dict[str, int | str]:
    symbol = "measure_once"
    instructions = functions.get(symbol)
    if not instructions:
        raise VerificationError(f"missing disassembly for {symbol}")
    address = instructions[0].address
    if not IRAM_START <= address < IRAM_END:
        raise VerificationError(f"{symbol} is not in IRAM")
    for instruction in instructions:
        if not instruction.mnemonic.startswith("call") or instruction.mnemonic == "callx8":
            continue
        target = re.search(r"\b([0-9a-fA-F]{8})\s+<", instruction.operands)
        if target is not None and not IRAM_START <= int(target.group(1), 16) < IRAM_END:
            raise VerificationError(
                f"{symbol} calls outside IRAM at {instruction.address:#010x}"
            )
    access_calls = [
        index
        for index, instruction in enumerate(instructions)
        if instruction.mnemonic == "callx8"
    ]
    if len(access_calls) != 1:
        raise VerificationError(f"{symbol} does not call an access ladder")
    access_call_index = access_calls[0]
    counter_control_loads = [
        index
        for index, instruction in enumerate(instructions[:access_call_index])
        if instruction.mnemonic == "l32r"
        and CACHE_COUNTER_CTRL in instruction.operands.lower()
    ]
    if len(counter_control_loads) != 1:
        raise VerificationError(f"{symbol} does not load the cache-counter control")
    counter_control_load_index = counter_control_loads[0]
    counter_clear_stores = [
        index
        for index, instruction in enumerate(
            instructions[counter_control_load_index + 1 : access_call_index],
            counter_control_load_index + 1,
        )
        if instruction.mnemonic.startswith("s32i")
    ]
    if len(counter_clear_stores) != 1:
        raise VerificationError(f"{symbol} does not have one cache-counter clear")
    counter_clear_index = counter_clear_stores[0]
    dispatch_loads = [
        instruction
        for instruction in instructions[counter_clear_index + 1 : access_call_index]
        if instruction.mnemonic.startswith("l")
    ]
    if dispatch_loads:
        raise VerificationError(
            f"{symbol} reloads access dispatch after counter clear at "
            f"{dispatch_loads[0].address:#010x}"
        )
    return {
        "symbol": symbol,
        "address": address,
        "memory": "iram",
        "endAddress": instructions[-1].address,
        "counterClearAddress": instructions[counter_clear_index].address,
        "accessCallAddress": instructions[access_call_index].address,
        "dispatchLoadsAfterClear": 0,
    }


def verify(disassembly: str) -> dict[str, object]:
    functions = parse_disassembly(disassembly)
    blocks = [
        verify_block(functions, kind, operations)
        for kind in ("read", "write")
        for operations in RUN_LENGTHS
    ]
    return {
        "ok": True,
        "accessBlocks": blocks,
        "measurementBoundary": verify_measurement_boundary(functions),
    }


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
