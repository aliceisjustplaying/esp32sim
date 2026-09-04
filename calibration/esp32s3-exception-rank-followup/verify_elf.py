#!/usr/bin/env python3
"""Fail-closed verification for the IDF 6.1 H2 timing image."""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import re
import subprocess
import sys
from pathlib import Path

from prove_design import ROWS, proof


CELL_IDS = (
    "rfe_alone",
    "rfi3_alone",
    "syscall_rfe_pair",
    "window_overflow8_entry",
    "window_overflow8_control",
    "window_underflow8_entry",
    "window_underflow8_control",
    "rfwo_alone",
    "rfwu_alone",
    "mask_rom_fetch_straight_line",
    "iram_fetch_matched_control",
)
ROM_SHA256 = "c0ce0f338d1de1bdc6efbef1591779a2a42c1ab7d759d3c6ae8ae63a7dd34cfd"
ROM_TARGET = 0x400559A4
H1_RECEIPT_COMMIT = "c6c0d5af528f0988004b7f77427a9259d9d2db3a"
H1_SOURCE_COMMIT = "75778a4cfef4332b09b7e0595d36fde188d0c118"
H1_SUMMARY_PATH = "docs/evidence/timing/h1-exception-ladders-2026-09-04/summary.json"
H1_SUMMARY_SHA256 = "511dd814024a7385dc2185f9f155819802c8e81e913568307c311262b541a613"
HEADER = re.compile(r"^([0-9a-f]+) <([^>]+)>:$")
INSN = re.compile(r"^([0-9a-f]+):\s+([0-9a-f]+)\s+([a-zA-Z0-9_.]+)(?:\s+(.*))?$")
SHA256 = re.compile(r"^[0-9a-f]{64}$")
GIT_COMMIT = re.compile(r"^[0-9a-f]{40}$")


class VerificationError(ValueError):
    pass


def sha256(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def run(command: list[str], cwd: Path | None = None) -> str:
    return subprocess.run(
        command, cwd=cwd, check=True, capture_output=True, text=True
    ).stdout


def functions(disassembly: str) -> dict[str, list[tuple[int, str, str, str]]]:
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


def require_function(
    parsed: dict[str, list[tuple[int, str, str, str]]], name: str
) -> list[tuple[int, str, str, str]]:
    instructions = parsed.get(name)
    if not instructions:
        raise VerificationError(f"missing {name} disassembly")
    return instructions


def symbol(symbols: str, name: str) -> tuple[int, str, int]:
    for raw in symbols.splitlines():
        fields = raw.split()
        if len(fields) >= 5 and fields[-1] == name:
            try:
                return int(fields[0], 16), fields[-3], int(fields[-2], 16)
            except ValueError:
                continue
    raise VerificationError(f"missing {name} symbol")


def mnemonics(instructions: list[tuple[int, str, str, str]]) -> tuple[str, ...]:
    return tuple(instruction[2] for instruction in instructions)


def contiguous(
    instructions: list[tuple[int, str, str, str]], expected: tuple[str, ...]
) -> bool:
    actual = mnemonics(instructions)
    return any(actual[index : index + len(expected)] == expected for index in range(len(actual)))


def require_ordered(
    instructions: list[tuple[int, str, str, str]],
    expected: tuple[tuple[str, str], ...],
    name: str,
) -> list[int]:
    cursor = 0
    addresses: list[int] = []
    for mnemonic, operands in expected:
        match = next(
            (
                index
                for index in range(cursor, len(instructions))
                if instructions[index][2] == mnemonic
                and instructions[index][3].strip() == operands
            ),
            None,
        )
        if match is None:
            raise VerificationError(f"{name} lacks ordered {mnemonic} {operands}")
        addresses.append(instructions[match][0])
        cursor = match + 1
    return addresses


def require_restore(
    parsed: dict[str, list[tuple[int, str, str, str]]],
    name: str,
    registers: tuple[str, ...],
) -> None:
    instructions = require_function(parsed, name)
    expected_tail = (
        "l32i.n", "wsr.epc1",
        "l32i.n", "wsr.excsave1",
        "l32i.n", "wsr.exccause",
        "l32i.n", "wsr.vecbase",
        "l32i.n", "wsr.sar",
        "l32i.n", "wsr.excsave2",
        "l32i.n", "wsr.ps", "rsync",
        "l32i.n", "wsr.windowstart", "rsync",
        "l32i.n", "retw.n",
    )
    returns = [index for index, item in enumerate(instructions) if item[2] == "retw.n"]
    if not returns:
        raise VerificationError(f"{name} has no return")
    for index in returns:
        actual = mnemonics(instructions[max(0, index - len(expected_tail) + 1) : index + 1])
        if actual != expected_tail:
            raise VerificationError(f"{name} has a return without the exact full-state restore tail")
    writes = {item[2][4:] for item in instructions if item[2].startswith("wsr.")}
    missing = [register for register in registers if register not in writes]
    if missing:
        raise VerificationError(f"{name} never restores {missing}")
    if any(item[2] == "wsr.windowbase" for item in instructions):
        raise VerificationError(f"{name} must return WINDOWBASE naturally")


def verify_manifest(path: Path) -> dict[str, object]:
    payload = json.loads(path.read_text(encoding="utf-8"))
    if set(payload) != {
        "protocolVersion",
        "harnessVersion",
        "chipModel",
        "chipRevision",
        "runtimeConfiguration",
        "cells",
    }:
        raise VerificationError("manifest has unexpected fields")
    if (
        payload["protocolVersion"],
        payload["harnessVersion"],
        payload["chipModel"],
        payload["chipRevision"],
    ) != (2, "2.0.0", "ESP32-S3", 2):
        raise VerificationError("manifest does not name the exact H2 board contract")
    expected_runtime = {
        "schemaVersion": "1.0.0",
        "idfVersion": "v6.1",
        "target": "esp32s3",
        "cores": 2,
        "cpuHz": 240000000,
        "ccountHz": 240000000,
        "samplesPerCell": 100,
        "maxAttemptsPerCell": 200,
        "probe": "exception-rank-followup",
        "emulatorChipRevision": 0,
    }
    if payload["runtimeConfiguration"] != expected_runtime:
        raise VerificationError("manifest runtime configuration is not exact")
    cells = payload["cells"]
    if not isinstance(cells, list) or tuple(cell.get("id") for cell in cells) != CELL_IDS:
        raise VerificationError("manifest does not contain the exact ordered H2 cells")
    if any(cell.get("samples") != 100 or cell.get("variants") != ["normal"] for cell in cells):
        raise VerificationError("every H2 cell must request 100 normal samples")
    return payload


def verify_h1_receipt(repo: Path) -> dict[str, str]:
    summary = subprocess.run(
        ["git", "show", f"{H1_RECEIPT_COMMIT}:{H1_SUMMARY_PATH}"],
        cwd=repo,
        check=True,
        capture_output=True,
    ).stdout
    digest = hashlib.sha256(summary).hexdigest()
    if digest != H1_SUMMARY_SHA256:
        raise VerificationError(f"H1 summary SHA-256 changed to {digest}")
    payload = json.loads(summary)
    if payload.get("inputs", {}).get("source_commit") != H1_SOURCE_COMMIT:
        raise VerificationError("H1 summary does not pin the expected source commit")
    equation_text = json.dumps(payload, sort_keys=True)
    if (
        "35 window pair - 18 handler prefixes = E_window_overflow8 + E_window_underflow8 + rfwo + rfwu"
        not in equation_text
        or '"value": 17' not in equation_text
    ):
        raise VerificationError("H1 summary lacks class-specific window entry columns")
    return {
        "commit": H1_RECEIPT_COMMIT,
        "sourceCommit": H1_SOURCE_COMMIT,
        "path": H1_SUMMARY_PATH,
        "sha256": digest,
    }


def verify_disassembly(disassembly: str, symbols: str) -> dict[str, object]:
    parsed = functions(disassembly)
    all_instructions = [item for body in parsed.values() for item in body]
    derived_rows: dict[str, tuple[int, ...]] = {}
    equation_evidence: dict[str, object] = {}

    iram = require_function(parsed, "iram_fetch_matched_control")
    if tuple((item[1], item[2], item[3]) for item in iram[:2]) != (
        ("002136", "entry", "a1, 16"),
        ("f01d", "retw.n", ""),
    ):
        raise VerificationError("IRAM matched control is not exact entry; retw.n")
    iram_address, iram_section, iram_size = symbol(symbols, "iram_fetch_matched_control")
    if iram_address % 4 or iram_section != ".iram0.text" or iram_size != 5:
        raise VerificationError("IRAM matched control placement changed")

    boundaries = {
        "rfe_alone": (("rsr.ccount", "a2"), ("rfe", ""), ("rsr.ccount", "a3")),
        "rfi3_alone": (("rsr.ccount", "a2"), ("rfi", "3"), ("rsr.ccount", "a3")),
        "rfwo_alone": (("rsr.ccount", "a2"), ("rfwo", ""), ("rsr.ccount", "a3")),
        "rfwu_alone": (("rsr.ccount", "a2"), ("rfwu", ""), ("rsr.ccount", "a3")),
        "syscall_rfe_pair": (("rsr.ccount", "a10"), ("syscall", ""), ("rsr.ccount", "a2")),
    }
    for name, expected in boundaries.items():
        body = require_function(parsed, name)
        actual = tuple((item[2], item[3].strip()) for item in body)
        matches = [
            index for index in range(len(actual) - len(expected) + 1)
            if actual[index : index + len(expected)] == expected
        ]
        if len(matches) != 1:
            raise VerificationError(f"{name} CCOUNT boundary changed")
        index = matches[0]
        equation_evidence[name] = {
            "addresses": [f"0x{item[0]:08x}" for item in body[index : index + 3]],
            "instructions": [f"{mnemonic} {operands}".strip() for mnemonic, operands in expected],
        }
        derived_rows[name] = ROWS[name]

    if mnemonics(require_function(parsed, "h2_syscall_handler")) != (
        "rsr.epc1",
        "addi",
        "wsr.epc1",
        "rsync",
        "rfe",
    ):
        raise VerificationError("syscall handler body changed")
    overflow = require_function(parsed, "window_overflow8_entry")
    overflow_control = require_function(parsed, "window_overflow8_control")
    overflow_target = require_function(parsed, "h2_overflow_target")
    if tuple((item[2], item[3].strip()) for item in overflow_target) != (
        ("entry", "a1, 16"), ("rsr.ccount", "a2"), ("retw.n", "")
    ):
        raise VerificationError("overflow entry and control do not share the exact target")
    overflow_shape = (
        ("rsr.windowbase", "a5"),
        ("addi.n", "a11, a5, 1"), ("extui", "a11, a11, 0, 4"),
        ("addi.n", "a12, a5, 2"), ("extui", "a12, a12, 0, 4"),
        ("addi.n", "a13, a5, 3"), ("extui", "a13, a13, 0, 4"),
        ("addi.n", "a14, a5, 4"), ("extui", "a14, a14, 0, 4"),
        ("rsr.windowstart", "a10"),
        ("or", "a11, a6, a7"), ("or", "a11, a11, a8"),
        ("and", "a3, a11, a10"),
    )
    for name, body in (
        ("window_overflow8_entry", overflow),
        ("window_overflow8_control", overflow_control),
    ):
        require_ordered(body, overflow_shape, name)
        calls = [index for index, item in enumerate(body) if item[2] == "call8" and "h2_overflow_target" in item[3]]
        if len(calls) != 1 or calls[0] == 0 or body[calls[0] - 1][2:] != ("rsr.ccount", "a2"):
            raise VerificationError(f"{name} does not share the exact CCOUNT-call8 boundary")
    require_ordered(
        overflow,
        (("or", "a11, a7, a9"), ("or", "a10, a10, a11"),
         ("wsr.windowstart", "a10"), ("rsync", ""),
         ("rsr.ccount", "a2")),
        "window_overflow8_entry trigger mask",
    )
    require_ordered(
        overflow_control,
        (("rsr.ccount", "a2"), ("sub", "a2, a10, a2")),
        "window_overflow8_control endpoint",
    )
    require_ordered(
        overflow,
        (("rsr.ccount", "a2"), ("rsr.excsave2", "a3"), ("sub", "a2, a3, a2")),
        "window_overflow8_entry endpoint",
    )
    derived_rows["window_overflow8_entry"] = ROWS["window_overflow8_entry"]
    equation_evidence["window_overflow8_entry"] = {
        "equation": "trigger raw minus matched no-overflow raw",
        "precondition": "WINDOWSTART[B+1..B+3] == 0",
        "triggerMask": "WINDOWSTART |= bit(B+2) | bit(B+4)",
        "sharedTarget": "entry a1, 16; rsr.ccount a2; retw.n",
    }
    underflow_target = require_function(parsed, "h2_underflow_target")
    require_ordered(
        underflow_target,
        (("entry", "a1, 32"), ("rsr.windowbase", "a2"),
         ("addi", "a2, a2, -2"), ("extui", "a2, a2, 0, 4"),
         ("rsr.windowstart", "a2"), ("movi", "a4, -1"),
         ("xor", "a3, a3, a4"), ("and", "a2, a2, a3"),
         ("wsr.windowstart", "a2"), ("rsync", ""),
         ("rsr.ccount", "a2"), ("retw.n", "")),
        "h2_underflow_target deterministic clear",
    )
    underflow_shape = (
        ("rsr.windowbase", "a5"),
        ("addi.n", "a11, a5, -1"), ("extui", "a11, a11, 0, 4"),
        ("addi.n", "a12, a5, 1"), ("extui", "a12, a12, 0, 4"),
        ("addi.n", "a13, a5, 2"), ("extui", "a13, a13, 0, 4"),
        ("rsr.windowstart", "a10"), ("and", "a3, a6, a10"),
        ("or", "a11, a7, a8"), ("or", "a11, a11, a9"),
        ("and", "a3, a11, a10"),
    )
    for name in ("window_underflow8_entry", "window_underflow8_control"):
        body = require_function(parsed, name)
        require_ordered(body, underflow_shape, name)
        calls = [item for item in body if item[2] == "call8" and "h2_underflow_target" in item[3]]
        if len(calls) != 1:
            raise VerificationError(f"{name} does not use the shared underflow target")
        call_index = body.index(calls[0])
        flag = "1" if name.endswith("entry") else "0"
        if call_index == 0 or body[call_index - 1][2] not in {"movi", "movi.n"} or body[call_index - 1][3] != f"a10, {flag}":
            raise VerificationError(f"{name} does not select the exact shared target path")
    require_ordered(
        require_function(parsed, "window_underflow8_entry"),
        (("sub", "a2, a11, a10"),),
        "window_underflow8_entry endpoint mapping",
    )
    require_ordered(
        require_function(parsed, "window_underflow8_control"),
        (("rsr.ccount", "a3"), ("sub", "a2, a3, a10")),
        "window_underflow8_control endpoint mapping",
    )
    derived_rows["window_underflow8_entry"] = ROWS["window_underflow8_entry"]
    equation_evidence["window_underflow8_entry"] = {
        "equation": "trigger raw minus matched no-underflow raw",
        "precondition": "WINDOWSTART[B] == 1 and WINDOWSTART[B-1,B+1,B+2] == 0",
        "triggerMask": "target AND-clears bit(target WINDOWBASE-2), the caller B bit",
        "endpointMapping": "target a2 to caller a10; vector a3 to caller a11",
    }

    vector_base, vector_section, _ = symbol(symbols, "h2_window_vector_base")
    overflow_vector, _, _ = symbol(symbols, "h2_window_overflow8_vector")
    underflow_vector, _, _ = symbol(symbols, "h2_window_underflow8_vector")
    if (
        vector_base % 0x400
        or vector_section != ".iram0.text"
        or overflow_vector != vector_base + 0x80
        or underflow_vector != vector_base + 0xC0
    ):
        raise VerificationError("private window vector offsets changed")
    vector_contracts = {
        "h2_window_overflow8_vector": (
            ("rsr.ccount", "a2"), ("wsr.excsave2", "a2"),
            ("rsync", ""), ("rfwo", ""),
        ),
        "h2_window_underflow8_vector": (
            ("rsr.ccount", "a3"), ("wsr.excsave2", "a3"),
            ("rsync", ""), ("rfwu", ""),
        ),
    }
    for name, expected in vector_contracts.items():
        actual = tuple((item[2], item[3].strip()) for item in require_function(parsed, name)[:4])
        if actual != expected:
            raise VerificationError(f"{name} first-instruction endpoint contract changed")

    restore_contracts = {
        name: (
            "windowstart", "epc1", "excsave1", "exccause",
            "vecbase", "sar", "excsave2", "ps",
        )
        for name in (
            "rfe_alone", "rfwo_alone", "rfwu_alone", "syscall_rfe_pair",
            "window_overflow8_entry", "window_overflow8_control",
            "window_underflow8_entry", "window_underflow8_control",
        )
    }
    restore_contracts["rfi3_alone"] = (
        "windowstart", "epc1", "epc3", "eps3", "excsave1",
        "exccause", "vecbase", "sar", "excsave2", "ps",
    )
    for name, registers in restore_contracts.items():
        require_restore(parsed, name, registers)

    for wrapper in ("measure_elapsed_samples", "measure_target_samples"):
        body = require_function(parsed, wrapper)
        dispatches = [item for item in body if item[2] == "callx8"]
        if len(dispatches) != 2:
            raise VerificationError(f"{wrapper} lacks one warmup and one measured callx8")
        text = "\n".join(item[3].lower() for item in body)
        if "600c40c4" not in text or any(
            address not in text
            for address in ("600c40c8", "600c40cc", "600c40d0", "600c40d4", "600c40d8")
        ):
            raise VerificationError(f"{wrapper} cache-counter gate changed")
        if not any(item[2].startswith("bnez") for item in body):
            raise VerificationError(f"{wrapper} lacks the cache-counter rejection branch")
        same_state_address, _, _ = symbol(symbols, "same_state")
        state_checks = [
            item for item in body
            if item[2] == "call8" and f"{same_state_address:x}" in item[3]
        ]
        if len(state_checks) != 2:
            raise VerificationError(f"{wrapper} does not state-check warmup and every sample")

    target_measure = require_function(parsed, "measure_target_samples")
    target_mnemonics = mnemonics(target_measure)
    woe_clear_count = sum(
        target_mnemonics[index] == "rsr.ps"
        and target_mnemonics[index + 1] in {"movi", "movi.n"}
        and target_mnemonics[index + 2 : index + 7]
        == ("slli", "or", "xor", "wsr.ps", "rsync")
        for index in range(len(target_mnemonics) - 6)
    )
    if woe_clear_count != 2:
        raise VerificationError("ROM wrapper does not clear PS.WOE for warmup and every sample")
    equation_evidence["rom_minus_iram_control"] = {
        "wrapper": "one measure_target_samples function pointer path",
        "windowState": "PS.WOE clear with exact before/after state checks",
        "cacheState": "all five cache counters folded into the rejection gate",
    }

    instruction_map = {item[0]: item for item in all_instructions}
    syscall_handler, _, _ = symbol(symbols, "h2_syscall_handler")
    base, section, _ = symbol(symbols, "h2_syscall_vector_base")
    if base % 0x400 or section != ".iram0.text":
        raise VerificationError("h2_syscall_vector_base is not an aligned IRAM vector")
    for offset in (0x300, 0x340):
        jump = instruction_map.get(base + offset)
        if jump is None or jump[2] != "j" or f"{syscall_handler:x}" not in jump[3]:
            raise VerificationError(f"syscall vector +0x{offset:x} does not jump to handler")

    return {
        "iramMatchedControl": {
            "address": f"0x{iram_address:08x}",
            "section": iram_section,
            "sizeBytes": iram_size,
            "instructions": [
                {"address": f"0x{item[0]:08x}", "encoding": item[1],
                 "mnemonic": item[2], "operands": item[3]}
                for item in iram[:2]
            ],
            "windowContract": "same wrapper, PS.WOE=0, exact state equality before and after",
        },
        "windowVectors": {
            "base": f"0x{vector_base:08x}",
            "overflow8": f"0x{overflow_vector:08x}",
            "underflow8": f"0x{underflow_vector:08x}",
            "firstInstruction": "rsr.ccount",
        },
        "restoredState": restore_contracts,
        "levelOneStateModel": "EPC1 plus exact PS including EXCM/OWB; Xtensa has no EPS1",
        "levelThreeStateModel": "EPC3 plus EPS3 and exact PS",
        "repetitionContract": "warmup and 100 accepted samples per cell require exact state equality",
        "reconstructedRows": {name: list(row) for name, row in derived_rows.items()},
        "equationEvidence": equation_evidence,
    }


def verify_rom(app_symbols: str, rom_symbols: str, rom_disassembly: str) -> dict[str, object]:
    alias_address, alias_section, _ = symbol(app_symbols, "mask_rom_fetch_straight_line")
    target_address, target_section, target_size = symbol(rom_symbols, "xtos_p_none")
    target = [
        item
        for item in require_function(functions(rom_disassembly), "xtos_p_none")
        if item[0] < target_address + target_size
    ]
    if (
        alias_address != ROM_TARGET
        or alias_section != "*ABS*"
        or target_address != ROM_TARGET
        or target_section != ".text"
        or target_size != 5
        or tuple((item[1], item[2], item[3]) for item in target)
        != (("002136", "entry", "a1, 16"), ("f01d", "retw.n", ""))
    ):
        raise VerificationError("mask-ROM target is not the exact aligned xtos_p_none pair")
    return {
        "address": f"0x{target_address:08x}",
        "section": target_section,
        "sizeBytes": target_size,
        "sha256": ROM_SHA256,
        "instructions": [
            {"address": f"0x{item[0]:08x}", "encoding": item[1],
             "mnemonic": item[2], "operands": item[3]}
            for item in target
        ],
    }


def artifact_map(build: Path, elf: Path, rom_elf: Path, image: Path) -> dict[str, dict[str, str]]:
    paths = {
        "applicationElf": elf,
        "applicationBinary": elf.with_suffix(".bin"),
        "bootloaderBinary": build / "bootloader/bootloader.bin",
        "partitionTableBinary": build / "partition_table/partition-table.bin",
        "sdkconfig": build / "sdkconfig",
        "flashArguments": build / "flash_args",
        "flasherArguments": build / "flasher_args.json",
        "probeManifest": image / "probe-cells.json",
        "designProof": image / "design-proof.json",
        "maskRomElf": rom_elf,
    }
    missing = [str(path) for path in paths.values() if not path.is_file()]
    if missing:
        raise VerificationError(f"missing build artifacts: {missing}")
    return {
        name: {"path": os.path.relpath(path, build), "sha256": sha256(path)}
        for name, path in paths.items()
    }


def verify(
    elf: Path, build: Path, rom_elf: Path, objdump: str, compiler: str, repo: Path
) -> dict[str, object]:
    image = Path(__file__).resolve().parent
    source_commit = run(["git", "rev-parse", "HEAD"], repo).strip()
    if GIT_COMMIT.fullmatch(source_commit) is None:
        raise VerificationError("source commit is not a full Git object ID")
    if run(["git", "status", "--porcelain"], repo):
        raise VerificationError("source tree must be clean before exact ELF verification")
    manifest = verify_manifest(image / "probe-cells.json")
    subprocess.run(
        [sys.executable, str(image / "prove_design.py"), "--check", str(image / "design-proof.json")],
        check=True,
    )
    if sha256(rom_elf) != ROM_SHA256:
        raise VerificationError("mask-ROM ELF hash changed")
    app_disassembly = run([objdump, "-d", str(elf)])
    app_symbols = run([objdump, "-t", str(elf)])
    rom_symbols = run([objdump, "-t", str(rom_elf)])
    rom_disassembly = run([objdump, "-d", str(rom_elf)])
    elf_contract = verify_disassembly(app_disassembly, app_symbols)
    rom_contract = verify_rom(app_symbols, rom_symbols, rom_disassembly)
    reconstructed_rows = {
        name: tuple(row) for name, row in elf_contract.pop("reconstructedRows").items()
    }
    reconstructed_rows["rom_minus_iram_control"] = ROWS["rom_minus_iram_control"]
    if set(reconstructed_rows) != set(ROWS):
        raise VerificationError("executable row names do not match the paper design")
    ordered_rows = {name: reconstructed_rows[name] for name in ROWS}
    elf_contract["executableRankProof"] = proof(ordered_rows)
    return {
        "schemaVersion": 1,
        "sourceCommit": source_commit,
        "sourceDirty": False,
        "idfVersion": "v6.1",
        "toolchain": {
            "compiler": compiler,
            "compilerVersion": run([compiler, "--version"]).splitlines()[0],
            "objdump": objdump,
            "objdumpVersion": run([objdump, "--version"]).splitlines()[0],
        },
        "manifest": manifest,
        "h1Receipt": verify_h1_receipt(repo),
        "artifacts": artifact_map(build, elf, rom_elf, image),
        "elfContract": elf_contract,
        "romContract": rom_contract,
    }


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("elf", type=Path)
    parser.add_argument("output", type=Path)
    parser.add_argument("--build", type=Path)
    parser.add_argument("--rom-elf", type=Path)
    parser.add_argument("--objdump", default="xtensa-esp32s3-elf-objdump")
    parser.add_argument("--compiler", default="xtensa-esp32s3-elf-gcc")
    args = parser.parse_args()
    if args.output.exists():
        print(f"refusing to overwrite {args.output}", file=sys.stderr)
        return 2
    rom_elf = args.rom_elf
    if rom_elf is None:
        directory = os.environ.get("ESP_ROM_ELF_DIR")
        if not directory:
            print("ESP_ROM_ELF_DIR is required", file=sys.stderr)
            return 2
        rom_elf = Path(directory) / "esp32s3_rev0_rom.elf"
    try:
        repo = Path(run(["git", "rev-parse", "--show-toplevel"]).strip())
        build = (args.build or args.elf.parent).resolve()
        result = verify(args.elf.resolve(), build, rom_elf.resolve(), args.objdump, args.compiler, repo)
    except (OSError, TypeError, json.JSONDecodeError, subprocess.CalledProcessError, VerificationError) as error:
        print(f"ELF verification failed: {error}", file=sys.stderr)
        return 2
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(result, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    print(f"ELF verification passed: {args.output}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
