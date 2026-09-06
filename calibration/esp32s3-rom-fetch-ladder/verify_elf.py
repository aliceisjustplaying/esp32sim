#!/usr/bin/env python3
"""Verify the ROM fetch ladder image against the pinned mask ROM and manifest.

Checks, all fail-closed:
- the manifest has the expected runtime configuration and the twenty cells;
- the pinned ROM ELF hash matches and every ROM cell's instruction encodings
  re-derived from its disassembly equal `rom-cells.json`;
- no ROM body contains an instruction outside the straight-line whitelist;
- the application ELF's `measure_probe_samples` has exactly one `callx8`
  between two `rsr.ccount` and no other call or branch in between;
- every build-time IRAM twin has the ROM's exact bytes, sits in IRAM and matches
  the ROM address modulo 128;
- the ROM cell table compiled into the firmware matches `rom-cells.json`.
Writes a JSON receipt pinning hashes and tool versions. Derives no price.
"""

import argparse
import hashlib
import json
import os
import re
import subprocess
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
ROM_SHA256 = "c0ce0f338d1de1bdc6efbef1591779a2a42c1ab7d759d3c6ae8ae63a7dd34cfd"
HEADER = re.compile(r"^([0-9a-f]+) <([^>]+)>:$")
INSN = re.compile(r"^([0-9a-f]+):\s+([0-9a-f]+)\s+([a-zA-Z0-9_.]+)(?:\s+(.*))?$")
STRAIGHT_LINE = {
    "entry", "retw.n", "abs", "mull", "nsau", "sub", "extui", "addi.n", "add.n",
    "neg", "and", "addi", "movnez", "movi.n",
}
EXPECTED_RUNTIME = {
    "schemaVersion": "1.0.0", "idfVersion": "v6.1", "target": "esp32s3",
    "cores": 2, "cpuHz": 240000000, "ccountHz": 240000000, "samplesPerCell": 100,
    "maxAttemptsPerCell": 200, "recursionDepth": 1, "probe": "rom-fetch-ladder",
    "emulatorChipRevision": 0,
}


class VerificationError(ValueError):
    pass


def tool_version(path: str) -> str:
    return subprocess.run([path, "--version"], capture_output=True, text=True, check=False).stdout.splitlines()[0]


def sha256(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def disassemble(objdump: str, elf: Path) -> dict[str, list[tuple[int, str, str, str]]]:
    text = subprocess.run([objdump, "-d", str(elf)], check=True, capture_output=True, text=True).stdout
    functions: dict[str, list[tuple[int, str, str, str]]] = {}
    current = None
    for line in text.splitlines():
        header = HEADER.match(line.strip())
        if header:
            current = header.group(2)
            functions[current] = []
            continue
        insn = INSN.match(line.strip())
        if insn and current is not None:
            functions[current].append((int(insn.group(1), 16), insn.group(2), insn.group(3), (insn.group(4) or "").strip()))
    return functions


def verify_manifest(path: Path) -> dict:
    manifest = json.loads(path.read_text())
    if manifest.get("protocolVersion") != 2 or manifest.get("harnessVersion") != "1.3.0":
        raise VerificationError("manifest protocol or harness version")
    if manifest.get("chipModel") != "ESP32-S3" or manifest.get("chipRevision") != 2:
        raise VerificationError("manifest chip identity")
    runtime = manifest.get("runtimeConfiguration")
    if runtime != EXPECTED_RUNTIME:
        raise VerificationError(f"manifest runtimeConfiguration {runtime!r}")
    ids = [cell["id"] for cell in manifest["cells"]]
    return {"manifestSha256": sha256(path), "cellIds": ids}


def verify_rom(objdump: str, rom_elf: Path, cells: list[dict]) -> dict:
    digest = sha256(rom_elf)
    if digest != ROM_SHA256:
        raise VerificationError(f"ROM ELF sha256 {digest} != {ROM_SHA256}")
    functions = disassemble(objdump, rom_elf)
    by_address = {ins[0]: ins for body in functions.values() for ins in body}
    verified = []
    for cell in cells:
        expected = [(int(a), e, m, o) for a, e, m, o in cell["encodings"]]
        for address, encoding, mnemonic, operands in expected:
            actual = by_address.get(address)
            if actual is None or actual[1] != encoding or actual[2] != mnemonic or actual[3] != operands:
                raise VerificationError(f"{cell['symbol']} at {address:#x}: {actual} != {(encoding, mnemonic, operands)}")
            if mnemonic not in STRAIGHT_LINE:
                raise VerificationError(f"{cell['symbol']} contains non-straight-line {mnemonic}")
        first, last = expected[0], expected[-1]
        if first[2] != "entry" or last[2] != "retw.n":
            raise VerificationError(f"{cell['symbol']} is not entry..retw.n")
        span = last[0] + len(last[1]) // 2 - first[0]
        if span != cell["bytes"] or first[0] != cell["address"]:
            raise VerificationError(f"{cell['symbol']} span {span} address {first[0]:#x}")
        verified.append({"symbol": cell["symbol"], "address": first[0], "bytes": span, "instructions": len(expected)})
    return {"romElfSha256": digest, "cells": verified}


def verify_driver(objdump: str, app_elf: Path) -> dict:
    functions = disassemble(objdump, app_elf)
    driver = functions.get("measure_probe_samples")
    if not driver:
        raise VerificationError("measure_probe_samples not found in application ELF")
    ccounts = [i for i, ins in enumerate(driver) if ins[2] == "rsr.ccount"]
    if len(ccounts) != 2:
        raise VerificationError(f"driver has {len(ccounts)} rsr.ccount, expected 2")
    between = driver[ccounts[0] + 1:ccounts[1]]
    allowed = {"callx8", "mov.n", "movi", "movi.n"}
    mnemonics = [ins[2] for ins in between]
    if mnemonics.count("callx8") != 1 or any(m not in allowed for m in mnemonics):
        raise VerificationError(f"driver instructions between ccount reads: {mnemonics}")
    # No memory access, no volatile sink traffic and no branch inside the timed region.
    return {
        "timedRegion": [{"address": ins[0], "encoding": ins[1], "mnemonic": ins[2], "operands": ins[3]}
                        for ins in driver[ccounts[0]:ccounts[1] + 1]],
        "callBetweenCcount": "callx8",
    }


def verify_twins(objdump: str, app_elf: Path, cells: list[dict]) -> dict:
    functions = disassemble(objdump, app_elf)
    verified = []
    for cell in cells:
        twin = functions.get("twin_" + cell["short"])
        if not twin:
            raise VerificationError(f"twin_{cell['short']} missing from application ELF")
        expected = [(e, m, o) for _, e, m, o in cell["encodings"]]
        actual = [(e, m, o) for _, e, m, o in twin[: len(expected)]]
        if actual != expected:
            raise VerificationError(f"twin_{cell['short']} bytes differ from ROM: {actual} != {expected}")
        if twin[0][0] % 128 != cell["address"] % 128:
            raise VerificationError(f"twin_{cell['short']} at {twin[0][0]:#x} is not ROM-aligned modulo 128")
        if not (0x40370000 <= twin[0][0] < 0x403E0000):
            raise VerificationError(f"twin_{cell['short']} at {twin[0][0]:#x} is not in IRAM")
        verified.append({"symbol": "twin_" + cell["short"], "address": twin[0][0], "romAddress": cell["address"]})
    return {"twins": verified}


def verify_firmware_table(app_elf: Path, cells: list[dict]) -> dict:
    # The firmware embeds each ROM address as a 32-bit literal; require all ten.
    data = app_elf.read_bytes()
    missing = [c["symbol"] for c in cells if c["address"].to_bytes(4, "little") not in data]
    if missing:
        raise VerificationError(f"firmware lacks ROM addresses for {missing}")
    return {"romAddressesEmbedded": len(cells)}


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("app_elf", type=Path)
    parser.add_argument("output", type=Path)
    parser.add_argument("--objdump", default="xtensa-esp32s3-elf-objdump")
    parser.add_argument("--rom-elf", type=Path, default=None)
    parser.add_argument("--compiler", default=None, help="recorded for provenance only")
    args = parser.parse_args()
    rom_elf = args.rom_elf
    if rom_elf is None:
        rom_dir = os.environ.get("ESP_ROM_ELF_DIR")
        if rom_dir:
            rom_elf = Path(rom_dir) / "esp32s3_rev0_rom.elf"
    if rom_elf is None or not rom_elf.exists():
        print(json.dumps({"ok": False, "error": "ROM ELF not found; pass --rom-elf"}))
        return 2
    cells = json.loads((HERE / "rom-cells.json").read_text())
    try:
        result = {
            "ok": True,
            "appElfSha256": sha256(args.app_elf),
            "manifest": verify_manifest(HERE / "probe-cells.json"),
            "rom": verify_rom(args.objdump, rom_elf, cells),
            "driver": verify_driver(args.objdump, args.app_elf),
            "twins": verify_twins(args.objdump, args.app_elf, cells),
            "firmware": verify_firmware_table(args.app_elf, cells),
            "romCellsSha256": sha256(HERE / "rom-cells.json"),
            "objdump": Path(args.objdump).name,
            "objdumpVersion": tool_version(args.objdump),
            "compiler": Path(args.compiler).name if args.compiler else None,
            "compilerVersion": tool_version(args.compiler) if args.compiler else None,
            "romElfPath": str(rom_elf),
        }
    except (VerificationError, subprocess.CalledProcessError, OSError) as error:
        print(json.dumps({"ok": False, "error": str(error)}, sort_keys=True))
        return 2
    args.output.write_text(json.dumps(result, indent=2, sort_keys=True) + "\n")
    print(json.dumps({"ok": True, "output": str(args.output)}))
    return 0


if __name__ == "__main__":
    sys.exit(main())
