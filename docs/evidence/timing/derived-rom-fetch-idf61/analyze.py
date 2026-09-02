#!/usr/bin/env python3
"""Recompute the mask-ROM memset fetch-cost candidates from pinned inputs."""

from __future__ import annotations

import hashlib
import json
import os
from fractions import Fraction
from pathlib import Path
import statistics
import struct
import tarfile


ROM_SHA256 = "c0ce0f338d1de1bdc6efbef1591779a2a42c1ab7d759d3c6ae8ae63a7dd34cfd"
ROOT = Path(__file__).resolve().parents[4]
RECEIPTS = ROOT / "docs/evidence/timing/idf61-rebaseline-3db3985/receipts"

INSTRUCTIONS = [
    (0x400570C8, "362100", "entry a1, 16", "issue"),
    (0x400570CB, "303074", "extui a3, a3, 0, 8", "issue"),
    (0x400570CE, "807311", "slli a7, a3, 8", "issue"),
    (0x400570D1, "703320", "or a3, a3, a7", "issue"),
    (0x400570D4, "007311", "slli a7, a3, 16", "issue"),
    (0x400570D7, "703320", "or a3, a3, a7", "issue"),
    (0x400570DA, "5d02", "mov.n a5, a2", "issue"),
    (0x400570DC, "07e2cc", "bbsi a2, 0, 0x400570ac", "branch_not_taken"),
    (0x400570DF, "17e2d6", "bbsi a2, 1, 0x400570b9", "branch_not_taken"),
    (0x400570E2, "407441", "srli a7, a4, 4", "issue"),
    (0x400570E5, "76970a", "loopnez a7, 0x400570f3", "loop_setup"),
    (0x400570E8, "3905", "s32i.n a3, a5, 0", "issue"),
    (0x400570EA, "3915", "s32i.n a3, a5, 4", "issue"),
    (0x400570EC, "3925", "s32i.n a3, a5, 8", "issue"),
    (0x400570EE, "3935", "s32i.n a3, a5, 12", "issue"),
    (0x400570F0, "52c510", "addi a5, a5, 16", "issue"),
    (0x400570F3, "376406", "bbci a4, 3, 0x400570fd", "branch_taken"),
    (0x400570F6, "3905", "s32i.n a3, a5, 0", "issue"),
    (0x400570F8, "3915", "s32i.n a3, a5, 4", "issue"),
    (0x400570FA, "52c508", "addi a5, a5, 8", "issue"),
    (0x400570FD, "276403", "bbci a4, 2, 0x40057104", "branch_taken"),
    (0x40057100, "3905", "s32i.n a3, a5, 0", "issue"),
    (0x40057102, "4b55", "addi.n a5, a5, 4", "issue"),
    (0x40057104, "176404", "bbci a4, 1, 0x4005710c", "branch_taken"),
    (0x40057107, "325500", "s16i a3, a5, 0", "issue"),
    (0x4005710A, "2b55", "addi.n a5, a5, 2", "issue"),
    (0x4005710C, "076402", "bbci a4, 0, 0x40057112", "branch_taken"),
    (0x4005710F, "324500", "s8i a3, a5, 0", "issue"),
    (0x40057112, "1df0", "retw.n", "call8_retw_pair"),
]

PREFIX = [address for address, _, _, _ in INSTRUCTIONS[:11]]
LOOP_BODY = [address for address, _, _, _ in INSTRUCTIONS[11:16]]
TAIL_ALIGNED = [0x400570F3, 0x400570FD, 0x40057104, 0x4005710C, 0x40057112]
PRICE = {
    "issue": Fraction(1),
    "branch_not_taken": Fraction(1),
    "branch_taken": Fraction(3),
    "loop_setup": Fraction(5),
    "call8_retw_pair": Fraction(15, 2),
}


def elf_symbol(data: bytes, wanted: str) -> tuple[int, bytes, str]:
    if data[:6] != b"\x7fELF\x01\x01":
        raise ValueError("ESP32S3_ROM_ELF must be a little-endian ELF32 file")
    section_offset = struct.unpack_from("<I", data, 32)[0]
    section_size, section_count, names_index = struct.unpack_from("<HHH", data, 46)
    sections = [
        struct.unpack_from("<IIIIIIIIII", data, section_offset + index * section_size)
        for index in range(section_count)
    ]
    names_section = sections[names_index]
    names = data[names_section[4] : names_section[4] + names_section[5]]

    def string(table: bytes, offset: int) -> str:
        end = table.index(0, offset)
        return table[offset:end].decode("ascii")

    for section in sections:
        if section[1] != 2:
            continue
        strings_section = sections[section[6]]
        strings = data[
            strings_section[4] : strings_section[4] + strings_section[5]
        ]
        entry_size = section[9]
        for offset in range(section[4], section[4] + section[5], entry_size):
            name_offset, value, size, _, _, section_index = struct.unpack_from(
                "<IIIBBH", data, offset
            )
            if name_offset and string(strings, name_offset) == wanted:
                symbol_section = sections[section_index]
                file_offset = symbol_section[4] + value - symbol_section[3]
                section_name = string(names, symbol_section[0])
                return value, data[file_offset : file_offset + size], section_name
    raise ValueError(f"symbol {wanted!r} is absent from ESP32S3_ROM_ELF")


def median_cycles(archive: Path, cell: str) -> int:
    with tarfile.open(archive, "r:gz") as bundle:
        member = bundle.extractfile(f"./{cell}.json")
        if member is None:
            raise ValueError(f"{cell} is absent from {archive}")
        receipt = json.load(member)
    samples = [sample["cycles"] for sample in receipt["measurement"]["samples"]]
    median = statistics.median(samples)
    if median != int(median):
        raise ValueError(f"{cell} has a noninteger median in {archive}")
    return int(median)


def fraction(value: Fraction) -> dict[str, int | float]:
    return {
        "numerator": value.numerator,
        "denominator": value.denominator,
        "decimal": float(value),
    }


def path_result(length: int, matched_cycles: int) -> dict[str, object]:
    repeats = length // 16
    segments = [
        {"kind": "prefix", "repeat": 1, "addresses": PREFIX},
        {"kind": "loop_body", "repeat": repeats, "addresses": LOOP_BODY},
        {"kind": "aligned_tail", "repeat": 1, "addresses": TAIL_ALIGNED},
    ]
    by_address = {instruction[0]: instruction for instruction in INSTRUCTIONS}
    known = Fraction(0)
    fetches = 0
    counts: dict[str, int] = {}
    for segment in segments:
        repeat = int(segment["repeat"])
        for address in segment["addresses"]:
            _, _, _, price_class = by_address[address]
            known += PRICE[price_class] * repeat
            fetches += repeat
            counts[price_class] = counts.get(price_class, 0) + repeat
    printable_segments = [
        {
            "kind": segment["kind"],
            "repeat": segment["repeat"],
            "instructions": [f"0x{address:08x}" for address in segment["addresses"]],
        }
        for segment in segments
    ]
    residual = Fraction(matched_cycles) - known
    return {
        "length_bytes": length,
        "instruction_segments": printable_segments,
        "instruction_counts_by_price_class": counts,
        "rom_instruction_fetches": fetches,
        "known_priced_cycles": fraction(known),
        "matched_receipt_cycles": matched_cycles,
        "residual_cycles": fraction(residual),
        "candidate_cycles_per_fetch": fraction(residual / fetches),
    }


def main() -> None:
    rom_path_text = os.environ.get("ESP32S3_ROM_ELF")
    if rom_path_text is None:
        raise SystemExit("ESP32S3_ROM_ELF must name the pinned ESP32-S3 ROM ELF")
    rom = Path(rom_path_text).read_bytes()
    digest = hashlib.sha256(rom).hexdigest()
    if digest != ROM_SHA256:
        raise SystemExit(f"ESP32S3_ROM_ELF sha256 {digest} does not match {ROM_SHA256}")
    address, symbol_bytes, section_name = elf_symbol(rom, "memset")
    expected_bytes = bytes.fromhex("".join(instruction[1] for instruction in INSTRUCTIONS))
    if address != 0x400570C8 or symbol_bytes != expected_bytes:
        raise SystemExit("pinned memset symbol address or bytes changed")

    archives = sorted(RECEIPTS.glob("boot-*-recovered.tar.gz"))
    cells = {}
    for length_name in ["zero_length", "0x52e0"]:
        target = f"rom_memset_{length_name}_single_core"
        baseline = f"rom_baseline_memset_{length_name}_single_core"
        boot_values = []
        for archive in archives:
            target_cycles = median_cycles(archive, target)
            baseline_cycles = median_cycles(archive, baseline)
            boot_values.append(
                {
                    "archive": str(archive.relative_to(ROOT)),
                    "target_median_cycles": target_cycles,
                    "baseline_median_cycles": baseline_cycles,
                    "matched_cycles": target_cycles - baseline_cycles,
                }
            )
        matched = {boot["matched_cycles"] for boot in boot_values}
        if len(matched) != 1:
            raise SystemExit(f"{length_name} matched receipts disagree across boots")
        cells[length_name] = {
            "boots": boot_values,
            "path": path_result(0 if length_name == "zero_length" else 0x52E0, matched.pop()),
        }

    candidates = [
        cells[name]["path"]["candidate_cycles_per_fetch"]
        for name in ["zero_length", "0x52e0"]
    ]
    adopted = candidates[0]["denominator"] == 1 and candidates[0] == candidates[1]
    output = {
        "schema_version": 1,
        "rom": {
            "sha256": digest,
            "memset_address": f"0x{address:08x}",
            "memset_size_bytes": len(symbol_bytes),
            "elf_section": section_name,
            "path_address_range": ["0x400570c8", "0x40057112"],
            "crosses_elf_region": False,
        },
        "pricing": {
            "issue_cycles": 1,
            "conditional_branch_cycles": {"taken": 3, "not_taken": 1},
            "loop_setup_cycles": 5,
            "independent_sram_store_additive_cycles": 0,
            "loop_alignment_additive_cycles": 0,
            "load_use_additive_cycles": 0,
            "callx8_retw_pair_cycles": fraction(Fraction(15, 2)),
            "source": "docs/evidence/timing/esp32s3-opcode-ladders-2026-09-02/summary.json",
        },
        "instruction_catalog": [
            {
                "address": f"0x{address:08x}",
                "bytes": encoded,
                "instruction": text,
                "price_class": price_class,
            }
            for address, encoded, text, price_class in INSTRUCTIONS
        ],
        "cells": cells,
        "adoption": {
            "adopted": adopted,
            "tier_candidate": "exact",
            "reason": (
                "both independent cells yield the same integer"
                if adopted
                else "the two cells yield different, noninteger residuals"
            ),
            "hardware_confirmation": "add a mask-ROM straight-line fetch cell to H1",
        },
    }
    print(json.dumps(output, indent=2, sort_keys=True))


if __name__ == "__main__":
    main()
