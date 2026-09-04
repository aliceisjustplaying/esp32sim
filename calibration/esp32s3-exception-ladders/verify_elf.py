#!/usr/bin/env python3
"""Verify the H1 exception ladder manifest and ELF instruction contract."""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import re
import subprocess
import sys
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
CACHE_COUNTER_CTRL = "600c40c4"
CACHE_COUNTERS = (
    "600c40c8",
    "600c40cc",
    "600c40d0",
    "600c40d4",
    "600c40d8",
)
EXPECTED_CONFIGURATION = {
    "protocolVersion": 2,
    "harnessVersion": "1.2.0",
    "chipModel": "ESP32-S3",
    "chipRevision": 2,
}
EXPECTED_RUNTIME_CONFIGURATION = {
    "schemaVersion": "1.0.0",
    "idfVersion": "v6.1",
    "target": "esp32s3",
    "cores": 2,
    "cpuHz": 240000000,
    "ccountHz": 240000000,
    "samplesPerCell": 100,
    "maxAttemptsPerCell": 200,
    "recursionDepth": 20,
    "probe": "exception-ladders",
    "emulatorChipRevision": 0,
}
HEADER = re.compile(r"^([0-9a-f]+) <([^>]+)>:$")
INSN = re.compile(r"^([0-9a-f]+):\s+([0-9a-f]+)\s+([a-zA-Z0-9_.]+)(?:\s+(.*))?$")


class VerificationError(ValueError):
    pass


def _sha256(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def verify_artifacts(
    elf_path: Path, manifest_path: Path, rom_elf_path: Path
) -> dict[str, object]:
    build = elf_path.parent
    app_bin = elf_path.with_suffix(".bin")
    bootloader = build / "bootloader" / "bootloader.bin"
    partition_table = build / "partition_table" / "partition-table.bin"
    sdkconfig = build / "sdkconfig"
    flasher_args = build / "flasher_args.json"
    flash_args = build / "flash_args"
    flasher = json.loads(flasher_args.read_text(encoding="utf-8"))
    if not isinstance(flasher, dict) or not isinstance(
        flasher.get("flash_files"), dict
    ):
        raise VerificationError("flasher_args.json does not define flash_files")
    roles_by_path = {
        "bootloader/bootloader.bin": "bootloaderBinary",
        "partition_table/partition-table.bin": "partitionTableBinary",
        app_bin.name: "applicationBinary",
    }
    try:
        flash_layout = sorted(
            (
                {
                    "offset": offset,
                    "artifact": roles_by_path[path],
                    "path": path,
                }
                for offset, path in flasher["flash_files"].items()
            ),
            key=lambda item: int(item["offset"], 16),
        )
    except (KeyError, TypeError, ValueError) as error:
        raise VerificationError("flasher_args.json has an invalid flash layout") from error
    if {item["artifact"] for item in flash_layout} != set(roles_by_path.values()):
        raise VerificationError("flasher_args.json does not flash every executable binary once")
    return {
        "artifacts": {
            "applicationElf": {"path": elf_path.name, "sha256": _sha256(elf_path)},
            "applicationBinary": {"path": app_bin.name, "sha256": _sha256(app_bin)},
            "bootloaderBinary": {
                "path": "bootloader/bootloader.bin",
                "sha256": _sha256(bootloader),
            },
            "partitionTableBinary": {
                "path": "partition_table/partition-table.bin",
                "sha256": _sha256(partition_table),
            },
            "sdkconfig": {"path": "sdkconfig", "sha256": _sha256(sdkconfig)},
            "flasherArguments": {
                "path": "flasher_args.json",
                "sha256": _sha256(flasher_args),
            },
            "flashArguments": {"path": "flash_args", "sha256": _sha256(flash_args)},
            "probeManifest": {
                "path": manifest_path.name,
                "sha256": _sha256(manifest_path),
            },
            "maskRomElf": {
                "path": rom_elf_path.name,
                "sha256": _sha256(rom_elf_path),
            },
        },
        "flashLayout": flash_layout,
        "flashSettings": flasher.get("flash_settings"),
        "esptoolArguments": flasher.get("extra_esptool_args"),
    }


def verify_manifest(path: Path) -> dict[str, object]:
    payload = json.loads(path.read_text(encoding="utf-8"))
    if not isinstance(payload, dict):
        raise VerificationError("manifest must be an object")
    expected_top_level = {*EXPECTED_CONFIGURATION, "runtimeConfiguration", "cells"}
    if set(payload) != expected_top_level:
        raise VerificationError("manifest does not have the exact H1 configuration contract")
    for field, expected in EXPECTED_CONFIGURATION.items():
        if payload.get(field) != expected or type(payload.get(field)) is not type(
            expected
        ):
            raise VerificationError(f"manifest {field} must be {expected!r}")
    runtime_configuration = payload.get("runtimeConfiguration")
    if (
        runtime_configuration != EXPECTED_RUNTIME_CONFIGURATION
        or not isinstance(runtime_configuration, dict)
        or any(
            type(runtime_configuration.get(field)) is not type(expected)
            for field, expected in EXPECTED_RUNTIME_CONFIGURATION.items()
        )
    ):
        raise VerificationError(
            "manifest runtimeConfiguration must match the exact H1 contract"
        )
    cells = payload.get("cells")
    if not isinstance(cells, list) or not all(isinstance(cell, dict) for cell in cells):
        raise VerificationError("manifest cells must be an array")
    ids = tuple(cell.get("id") for cell in cells)
    if ids != CELL_IDS:
        raise VerificationError("manifest must contain the exact ordered H1 cells")
    if any(cell.get("samples") != 100 for cell in cells):
        raise VerificationError("every H1 cell must request 100 samples")
    if any(cell.get("variants") != ["normal"] for cell in cells):
        raise VerificationError("every H1 cell must have exactly the normal variant")
    expected_families = ["exception"] * 6 + ["instruction-fetch"]
    if [cell.get("family") for cell in cells] != expected_families:
        raise VerificationError("H1 cell families do not match the exact contract")
    basic_keys = {"id", "family", "samples", "variants"}
    for index, cell in enumerate(cells):
        expected_keys = basic_keys | ({"knownTerms"} if index in (3, 6) else set())
        if set(cell) != expected_keys:
            raise VerificationError(f"manifest cell {CELL_IDS[index]} has unexpected fields")
    syscall = cells[3]
    expected_terms = ["rsr.epc1", "addi", "wsr.epc1", "rsync", "rfe"]
    if syscall.get("knownTerms") != expected_terms:
        raise VerificationError("syscall cell must record the five known handler terms")
    rom_terms = ["entry", "retw.n"]
    if cells[6].get("knownTerms") != rom_terms:
        raise VerificationError("mask-ROM cell must record its two straight-line terms")
    return {
        "cells": list(ids),
        "samplesPerCell": 100,
        "knownTerms": expected_terms,
        "maskRomKnownTerms": rom_terms,
        "configuration": dict(EXPECTED_CONFIGURATION),
        "runtimeConfiguration": dict(EXPECTED_RUNTIME_CONFIGURATION),
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
    }


def _operands(instruction: tuple[int, str, str, str]) -> list[str]:
    return [operand.strip() for operand in instruction[3].split(",")]


def _branch_target(instruction: tuple[int, str, str, str]) -> int | None:
    operands = _operands(instruction)
    if len(operands) != 2:
        return None
    match = re.match(r"^([0-9a-fA-F]+)", operands[1])
    return int(match.group(1), 16) if match is not None else None


def _verify_post_dispatch_cache_gate(
    measure: list[tuple[int, str, str, str]], dispatch: int
) -> dict[str, object]:
    tail = measure[dispatch + 1 :]
    loads: dict[str, tuple[int, str]] = {}
    for counter in CACHE_COUNTERS:
        matches = [
            (index, _operands(item)[0])
            for index, item in enumerate(tail, dispatch + 1)
            if item[2] == "l32r"
            and counter in item[3].lower()
            and len(_operands(item)) == 2
        ]
        if len(matches) != 1:
            raise VerificationError(
                f"measurement lacks one exact post-dispatch read base for 0x{counter}"
            )
        loads[counter] = matches[0]

    base_counters: dict[str, str] = {}
    taint: dict[str, set[str]] = {}
    reads: dict[str, int] = {}
    dirty_gate: tuple[int, tuple[int, str, str, str]] | None = None
    for index, item in enumerate(tail, dispatch + 1):
        operands = _operands(item)
        loaded_counter = next(
            (counter for counter, (load, _) in loads.items() if load == index), None
        )
        if loaded_counter is not None:
            register = operands[0]
            base_counters[register] = loaded_counter
            taint[register] = set()
            continue
        if item[2].startswith("l32i") and len(operands) == 3 and operands[2] == "0":
            destination, base = operands[:2]
            counter = base_counters.get(base)
            if counter is not None:
                if counter in reads:
                    raise VerificationError(
                        f"measurement reads cache counter 0x{counter} more than once"
                    )
                reads[counter] = index
                taint[destination] = {counter}
            continue
        if item[2] == "or" and len(operands) == 3:
            destination, left, right = operands
            taint[destination] = taint.get(left, set()) | taint.get(right, set())
            continue
        if item[2].startswith("bnez") and len(operands) == 2:
            if taint.get(operands[0], set()) == set(CACHE_COUNTERS):
                dirty_gate = (index, item)
                break
    if set(reads) != set(CACHE_COUNTERS) or dirty_gate is None:
        raise VerificationError(
            "measurement does not fold all post-dispatch cache-counter reads into the zero gate"
        )

    gate_index, gate = dirty_gate
    rejection_target = _branch_target(gate)
    if rejection_target is None:
        raise VerificationError("cache-counter zero gate has no exact rejection target")
    elapsed_definitions = [
        (index, _operands(item)[0])
        for index, item in enumerate(measure[dispatch + 1 : gate_index], dispatch + 1)
        if item[2] == "sub" and len(_operands(item)) == 3
    ]
    if len(elapsed_definitions) != 1:
        raise VerificationError("measurement lacks one elapsed-cycle value before acceptance")
    elapsed_register = elapsed_definitions[0][1]
    elapsed_gates = [
        (index, item)
        for index, item in enumerate(measure[gate_index + 1 :], gate_index + 1)
        if item[2].startswith("beqz")
        and len(_operands(item)) == 2
        and _operands(item)[0] == elapsed_register
        and _branch_target(item) == rejection_target
    ]
    if len(elapsed_gates) != 1:
        raise VerificationError(
            "measurement lacks the zero-elapsed gate to the cache rejection path"
        )
    elapsed_gate_index = elapsed_gates[0][0]
    accepted_stores = [
        (index, item)
        for index, item in enumerate(
            measure[elapsed_gate_index + 1 :], elapsed_gate_index + 1
        )
        if item[0] < rejection_target
        and item[2].startswith("s32i")
        and len(_operands(item)) == 3
        and _operands(item)[0] == elapsed_register
    ]
    if len(accepted_stores) != 1:
        raise VerificationError(
            "measurement lacks one accepted-sample store behind the cache zero gate"
        )
    return {
        "cacheCounterRegisters": [f"0x{counter}" for counter in CACHE_COUNTERS],
        "cacheCounterReadAddresses": [
            f"0x{measure[reads[counter]][0]:08x}" for counter in CACHE_COUNTERS
        ],
        "cacheZeroGateAddress": f"0x{gate[0]:08x}",
        "rejectionTargetAddress": f"0x{rejection_target:08x}",
        "acceptedSampleStoreAddress": f"0x{accepted_stores[0][1][0]:08x}",
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
    dispatches = [
        index for index, item in enumerate(measure) if item[2] == "callx8"
    ]
    if len(dispatches) != 2:
        raise VerificationError("measurement must have one warmup and one measured dispatch")
    warmup, dispatch = dispatches
    control_loads = [
        index
        for index, item in enumerate(measure[warmup + 1 : dispatch], warmup + 1)
        if item[2] == "l32r" and CACHE_COUNTER_CTRL in item[3].lower()
    ]
    if len(control_loads) != 1:
        raise VerificationError(
            "measurement lacks the exact cache-counter control load before dispatch"
        )
    control_load = control_loads[0]
    control_register = measure[control_load][3].split(",", 1)[0].strip()
    counter_clear_stores = []
    for index, item in enumerate(
        measure[control_load + 1 : dispatch], control_load + 1
    ):
        if not item[2].startswith("s32i"):
            continue
        operands = [operand.strip() for operand in item[3].split(",")]
        if len(operands) == 3 and operands[1:] == [control_register, "0"]:
            counter_clear_stores.append(index)
    if len(counter_clear_stores) != 1:
        raise VerificationError(
            "measurement lacks one exact cache-counter clear before dispatch"
        )
    clear = counter_clear_stores[0]
    clear_source = measure[clear][3].split(",", 1)[0].strip()
    clear_values = [
        item
        for item in measure[control_load + 1 : clear]
        if item[2] in {"movi", "movi.n"}
        and item[3].split(",", 1)[0].strip() == clear_source
        and item[3].split(",", 1)[1].strip() == "3"
    ]
    if len(clear_values) != 1:
        raise VerificationError("cache-counter clear does not write both clear bits")
    later_stores = [
        item for item in measure[clear + 1 : dispatch] if item[2].startswith("s32i")
    ]
    if later_stores:
        raise VerificationError("cache-counter clear is not the final store before dispatch")
    measurement_boundary = {
        "counterControl": f"0x{CACHE_COUNTER_CTRL}",
        "counterClearAddress": f"0x{measure[clear][0]:08x}",
        "dispatchAddress": f"0x{measure[dispatch][0]:08x}",
        "warmupDispatches": 1,
        **_verify_post_dispatch_cache_gate(measure, dispatch),
    }
    return {"cells": cells, "measurementBoundary": measurement_boundary}


def verify_elf(
    elf_path: Path, manifest_path: Path, rom_elf_path: Path, objdump: str
) -> dict[str, object]:
    manifest = verify_manifest(manifest_path)
    rom_bytes = rom_elf_path.read_bytes()
    rom_sha256 = hashlib.sha256(rom_bytes).hexdigest()
    if rom_sha256 != ROM_SHA256:
        raise VerificationError(
            f"mask ROM SHA-256 {rom_sha256} does not match the pinned ESP32-S3 ROM"
        )
    app_disassembly = subprocess.run(
        [objdump, "-d", str(elf_path)],
        check=True,
        capture_output=True,
        text=True,
    ).stdout
    disassembly = verify_disassembly(app_disassembly)
    app_symbols = subprocess.run(
        [objdump, "-t", str(elf_path)],
        check=True,
        capture_output=True,
        text=True,
    ).stdout
    rom_symbols = subprocess.run(
        [objdump, "-t", str(rom_elf_path)],
        check=True,
        capture_output=True,
        text=True,
    ).stdout
    rom_disassembly = subprocess.run(
        [objdump, "-d", str(rom_elf_path)],
        check=True,
        capture_output=True,
        text=True,
    ).stdout
    rom_contract = verify_rom_contract(app_symbols, rom_symbols, rom_disassembly)
    rom_contract["cacheCountersRequiredZero"] = disassembly[
        "measurementBoundary"
    ]["cacheCountersRequiredZero"]
    verified_cells = dict(disassembly["cells"])
    verified_cells["mask_rom_fetch_straight_line"] = rom_contract
    return {
        "elfSha256": hashlib.sha256(elf_path.read_bytes()).hexdigest(),
        "romElfSha256": rom_sha256,
        "manifestSha256": _sha256(manifest_path),
        **verify_artifacts(elf_path, manifest_path, rom_elf_path),
        "manifest": manifest,
        **disassembly,
        "cells": verified_cells,
    }


def resolve_rom_elf(explicit: Path | None) -> Path:
    if explicit is not None:
        return explicit
    rom_directory = os.environ.get("ESP_ROM_ELF_DIR")
    if not rom_directory:
        raise VerificationError(
            "ESP_ROM_ELF_DIR must locate the pinned ESP32-S3 ROM ELF"
        )
    return Path(rom_directory) / "esp32s3_rev0_rom.elf"


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("elf", type=Path)
    parser.add_argument("output", type=Path)
    parser.add_argument("--objdump", default="xtensa-esp32s3-elf-objdump")
    parser.add_argument("--rom-elf", type=Path)
    args = parser.parse_args()
    if args.output.exists():
        print(f"refusing to overwrite result: {args.output}", file=sys.stderr)
        return 2
    try:
        result = verify_elf(
            args.elf,
            Path(__file__).with_name("probe-cells.json"),
            resolve_rom_elf(args.rom_elf),
            args.objdump,
        )
    except (
        OSError,
        TypeError,
        json.JSONDecodeError,
        subprocess.CalledProcessError,
        VerificationError,
    ) as error:
        print(f"ELF verification failed: {error}", file=sys.stderr)
        return 2
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(
        json.dumps(result, indent=2, sort_keys=True) + "\n", encoding="utf-8"
    )
    print(f"ELF verification passed: {args.output}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
