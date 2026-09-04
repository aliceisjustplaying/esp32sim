#!/usr/bin/env python3
"""Verify and reduce the 2026-09-04 H1 exception-ladder capture."""

from __future__ import annotations

from collections import Counter
import hashlib
import json
import math
import os
from pathlib import Path
import subprocess


REPO = Path(__file__).resolve().parents[4]
ARCHIVE = Path(
    os.environ.get(
        "ESP32S3_H1_ARCHIVE",
        "/Users/sarah/Archives/esp32s3/"
        "hardware-batch-2026-09-04-20260904-102500",
    )
)
BUNDLE = Path(
    os.environ.get(
        "ESP32S3_H1_BUNDLE",
        "/Users/sarah/Archives/esp32s3/pinned-builds/esp32sim-h1-75778a4c",
    )
)

SOURCE_COMMIT = "75778a4cfef4332b09b7e0595d36fde188d0c118"
ARCHIVE_INDEX_SHA256 = "77073c188c671e43d2ef96a7da59a9f17817f9dba570165c9806072700abce6a"
BUNDLE_MANIFEST_SHA256 = "7d4f95cef83208211f3eceb4e62ab3415fde2271948422f27a809c377379cabc"
OPCODE_SUMMARY_SHA256 = "db29ec42ccccc958c96153340497592ecc76203166a5a98c696bdd81496c6515"
TOOLCHAIN_DELTA_SHA256 = "d4a4d3547598ede01573b94b5da3fdd1258d3f4e8161778acb4fd0423ac8a654"
DERIVED_EXCEPTION_SHA256 = "7e97a179219bdaf2435a3536633a0d01ceb9e9d4764a1f56cc43e797fb221f52"

SOURCE_FILES = {
    "calibration/esp32s3-exception-ladders/main/exception_ladders.S":
        "353c3a4eddb8d47668a03b2920b80a7faba62393180442fb5a834ac5188188f7",
    "calibration/esp32s3-exception-ladders/main/exception_ladders.c":
        "93ee09d99829045b71b54b2752699503c2229c6bbeaa41ad33a3d9695f70d777",
    "calibration/esp32s3-exception-ladders/verify_elf.py":
        "f0b845e4b4f863d201b6a43b721401cac5b91118a30bb0babbe08bbac8a294fd",
    "calibration/esp32s3-exception-ladders/probe-cells.json":
        "8e5e7e333c3d15d643a121c7f01cf8500503bd35169a6da18b0f69905402118b",
}

RAW_HASHES = {
    "boot-1.log": "f62755e99638581bdaaa84e9f34075a629dae160326e50a6135ee274edd68e3d",
    "boot-2.log": "656a578dbebab79af605fc40e21475f7e142a40ce387beee2f69151b1d6be71d",
}

CELLS = [
    "call4_window_pair",
    "call8_window_pair",
    "call12_window_pair",
    "syscall_rfe_pair",
    "rfe_alone",
    "rfi3_alone",
    "mask_rom_fetch_straight_line",
]


def sha256_bytes(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def sha256_file(path: Path) -> str:
    return sha256_bytes(path.read_bytes())


def require_hash(path: Path, expected: str) -> None:
    actual = sha256_file(path)
    if actual != expected:
        raise ValueError(f"{path} sha256 {actual}, expected {expected}")


def load_json(path: Path) -> object:
    return json.loads(path.read_text())


def verify_archive() -> dict[str, str]:
    index = ARCHIVE / "SHA256SUMS"
    require_hash(index, ARCHIVE_INDEX_SHA256)
    committed_index = Path(__file__).with_name("archive-SHA256SUMS")
    if committed_index.read_bytes() != index.read_bytes():
        raise ValueError("committed archive index differs from the captured index")
    entries: dict[str, str] = {}
    for line in index.read_text().splitlines():
        digest, relative = line.split("  ", 1)
        require_hash(ARCHIVE / relative, digest)
        entries[relative] = digest
    for name, digest in RAW_HASHES.items():
        relative = f"h1-exception-ladders/{name}"
        if entries.get(relative) != digest:
            raise ValueError(f"archive index does not pin {relative}")

    contract = load_json(ARCHIVE / "session-contract.json")
    capture = contract["captureOrder"][0]
    if capture != {
        "id": "h1-exception-ladders",
        "bundlePath": str(BUNDLE),
        "manifestSha256": BUNDLE_MANIFEST_SHA256,
        "boots": 2,
        "terminal": "CALIBRATION_DONE",
    }:
        raise ValueError("session contract does not bind the expected H1 bundle")
    state = load_json(ARCHIVE / "session-state.json")
    image = state["images"][0]
    if state["status"] != "complete" or image["id"] != "h1-exception-ladders":
        raise ValueError("hardware session or H1 image is incomplete")
    for boot in image["boots"]:
        validation = boot["validation"]
        if not validation["ok"] or validation["capturedSamples"] != 700:
            raise ValueError("H1 validation did not accept exactly 700 samples")
        if validation["completedCells"] != 7 or validation["refusals"] != 0:
            raise ValueError("H1 validation did not complete all seven cells")
    return entries


def verify_bundle() -> dict:
    manifest_path = BUNDLE / "MANIFEST.json"
    require_hash(manifest_path, BUNDLE_MANIFEST_SHA256)
    manifest = load_json(manifest_path)
    for artifact in manifest["artifacts"].values():
        require_hash(BUNDLE / artifact["path"], artifact["sha256"])
    if manifest["toolchain"]["idfVersion"] != "v6.1":
        raise ValueError("H1 is not an IDF 6.1 build")
    if manifest["cells"]["rfe_alone"]["returnEncoding"] != "003000":
        raise ValueError("rfe encoding changed")
    if manifest["cells"]["rfi3_alone"]["returnEncoding"] != "003310":
        raise ValueError("rfi 3 encoding changed")
    if manifest["cells"]["syscall_rfe_pair"]["handlerEncodings"] != [
        "03b120", "03c222", "13b120", "002010", "003000"
    ]:
        raise ValueError("syscall handler changed")
    rom = manifest["cells"]["mask_rom_fetch_straight_line"]
    if rom["address"] != "0x400559a4" or rom["instructionFetchesPerTrial"] != 2:
        raise ValueError("mask-ROM target changed")
    if [(row["mnemonic"], row["encoding"]) for row in rom["instructions"]] != [
        ("entry", "002136"), ("retw.n", "f01d")
    ]:
        raise ValueError("mask-ROM body changed")
    return manifest


def verify_source_commit() -> None:
    resolved = subprocess.run(
        ["git", "rev-parse", f"{SOURCE_COMMIT}^{{commit}}"],
        cwd=REPO,
        check=True,
        capture_output=True,
        text=True,
    ).stdout.strip()
    if resolved != SOURCE_COMMIT:
        raise ValueError("H1 source commit does not resolve exactly")
    for relative, expected in SOURCE_FILES.items():
        data = subprocess.run(
            ["git", "show", f"{SOURCE_COMMIT}:{relative}"],
            cwd=REPO,
            check=True,
            capture_output=True,
        ).stdout
        if sha256_bytes(data) != expected:
            raise ValueError(f"source hash changed at {SOURCE_COMMIT}:{relative}")


def parse_boot(path: Path) -> tuple[dict, dict[str, list[int]]]:
    records = []
    for line in path.read_text(errors="strict").splitlines():
        marker = "CAL_RECORD "
        if marker in line:
            records.append(json.loads(line.split(marker, 1)[1]))
    configs = [record for record in records if record["type"] == "configuration"]
    metrics = [record for record in records if record["type"] == "metric"]
    refusals = [record for record in records if record["type"] == "refusal"]
    if len(configs) != 1 or refusals or [row["name"] for row in metrics] != CELLS:
        raise ValueError(f"{path} does not contain the exact seven-cell record set")
    config = configs[0]
    expected_config = {
        "schema_version": "1.0.0",
        "harness_version": "1.2.0",
        "idf_version": "v6.1",
        "target": "esp32s3",
        "chip_revision": 2,
        "cores": 2,
        "cpu_hz": 240_000_000,
        "ccount_hz": 240_000_000,
        "probe": "exception-ladders",
        "samples_per_cell": 100,
        "max_attempts_per_cell": 200,
        "recursion_depth": 20,
    }
    if {key: config[key] for key in expected_config} != expected_config:
        raise ValueError(f"{path} configuration does not match H1")
    samples = {}
    for metric in metrics:
        values = metric["ccount_samples"]
        if len(values) != 100 or not metric["cache_counters_required_zero"]:
            raise ValueError(f"{path} {metric['name']} is not a 100-sample cache-clean cell")
        samples[metric["name"]] = values
    return config, samples


def sample_summary(values: list[int]) -> dict:
    ordered = sorted(values)
    counts = Counter(ordered)
    return {
        "count": len(values),
        "min": ordered[0],
        "median": (ordered[49] + ordered[50]) / 2,
        "p90_nearest_rank": ordered[math.ceil(0.9 * len(ordered)) - 1],
        "max": ordered[-1],
        "frequencies": {str(value): counts[value] for value in sorted(counts)},
    }


def main() -> None:
    archive_entries = verify_archive()
    manifest = verify_bundle()
    verify_source_commit()

    opcode_path = REPO / "docs/evidence/timing/esp32s3-opcode-ladders-2026-09-02/summary.json"
    target_path = REPO / "docs/evidence/timing/idf61-rebaseline-3db3985/toolchain-delta.json"
    old_exception_path = REPO / "docs/evidence/timing/derived-exception-idf61/summary.json"
    require_hash(opcode_path, OPCODE_SUMMARY_SHA256)
    require_hash(target_path, TOOLCHAIN_DELTA_SHA256)
    require_hash(old_exception_path, DERIVED_EXCEPTION_SHA256)
    opcode = load_json(opcode_path)
    target = load_json(target_path)
    old_exception = load_json(old_exception_path)

    boots = []
    boot_samples = []
    for boot_name in RAW_HASHES:
        path = ARCHIVE / "h1-exception-ladders" / boot_name
        _, samples = parse_boot(path)
        boot_samples.append(samples)
        boots.append({
            "raw_path": f"h1-exception-ladders/{boot_name}",
            "raw_sha256": RAW_HASHES[boot_name],
            "cells": {name: sample_summary(samples[name]) for name in CELLS},
        })

    exact_medians = {}
    for name in CELLS:
        medians = [sample_summary(samples[name])["median"] for samples in boot_samples]
        if medians[0] != medians[1]:
            raise ValueError(f"{name} boot medians differ")
        exact_medians[name] = int(medians[0])

    if exact_medians != {
        "call4_window_pair": 352,
        "call8_window_pair": 674,
        "call12_window_pair": 862,
        "syscall_rfe_pair": 18,
        "rfe_alone": 6,
        "rfi3_alone": 5,
        "mask_rom_fetch_straight_line": 15,
    }:
        raise ValueError("H1 medians changed")

    issue_cycles = target["siliconArchitectural"]["straightLineIssueCyclesPerInstruction"]
    window_pair = target["siliconArchitectural"][
        "windowOverflowUnderflowPairCyclesPastDepth6"
    ]
    known_handler = sum(
        row["known_cycles"]
        for row in old_exception["attempts"]
        if row["name"] in {"window_overflow8", "window_underflow8"}
    )
    rfe = exact_medians["rfe_alone"] - issue_cycles
    rfi3 = exact_medians["rfi3_alone"] - issue_cycles
    syscall_entry = (
        exact_medians["syscall_rfe_pair"]
        - exact_medians["rfe_alone"]
        - issue_cycles
        - 4 * issue_cycles
    )
    window_unknown_sum = window_pair - known_handler
    conditional_rfwo_rfwu = window_unknown_sum - 2 * syscall_entry

    call_pair = opcode["cells"]["callx8_retw"]["classification"]["range"]
    rom_fixed_terms = 8
    rom_candidates = [
        (exact_medians["mask_rom_fetch_straight_line"] - rom_fixed_terms - pair) / 2
        for pair in call_pair
    ]

    result = {
        "schema_version": 1,
        "inputs": {
            "archive_index_path": str(ARCHIVE / "SHA256SUMS"),
            "archive_index_sha256": ARCHIVE_INDEX_SHA256,
            "archive_index_entries_verified": len(archive_entries),
            "bundle_manifest_path": str(BUNDLE / "MANIFEST.json"),
            "bundle_manifest_sha256": BUNDLE_MANIFEST_SHA256,
            "application_elf_sha256": manifest["elfSha256"],
            "source_commit": SOURCE_COMMIT,
            "source_files": SOURCE_FILES,
            "opcode_summary_sha256": OPCODE_SUMMARY_SHA256,
            "toolchain_delta_sha256": TOOLCHAIN_DELTA_SHA256,
            "derived_exception_summary_sha256": DERIVED_EXCEPTION_SHA256,
        },
        "boots": boots,
        "candidates": {
            "rfe_instruction_cycles": {
                "equation": "6 - 1 leading rsr.ccount",
                "tier_candidate": "exact",
                "value": rfe,
            },
            "rfi3_instruction_cycles": {
                "equation": "5 - 1 leading rsr.ccount",
                "tier_candidate": "exact",
                "value": rfi3,
            },
            "syscall_exception_entry_cycles": {
                "equation": "18 - 6 matched rfe cell - 1 syscall issue - 4 handler issues",
                "tier_candidate": "exact",
                "value": syscall_entry,
            },
            "window_entry_and_return_sum_cycles": {
                "equation": (
                    "35 window pair - 18 handler prefixes = "
                    "E_window_overflow8 + E_window_underflow8 + rfwo + rfwu"
                ),
                "rank": 1,
                "tier_candidate": "exact correlation target",
                "value": window_unknown_sum,
            },
            "conditional_rfwo_plus_rfwu_cycles": {
                "condition": (
                    "E_window_overflow8 = E_window_underflow8 = "
                    "E_syscall = 7; H1 and the pinned sources do not prove this"
                ),
                "equation": "17 window unknown sum - 2 * 7 assumed window entries",
                "tier_candidate": "unexplained",
                "value_if_condition_holds": conditional_rfwo_rfwu,
            },
            "mask_rom_fetch_cycles": {
                "callx8_entry_retw_interval": call_pair,
                "equation": "15 - 8 fixed non-ROM cycles - [7,8] call sequence, divided by 2 fetches",
                "fixed_non_rom_terms": [
                    "rsr.ccount issue",
                    "depth l32i issue",
                    "memw issue after return",
                    "benchmark sink l32i issue",
                    "add issue",
                    "l32i-to-add load-use delay",
                    "memw issue before sink store",
                    "benchmark sink s32i issue",
                ],
                "interval": [min(rom_candidates), max(rom_candidates)],
                "tier_candidate": "interval",
                "value": None,
            },
        },
        "rank_findings": {
            "window": (
                "The exact window equation has four unknowns: class-specific overflow and "
                "underflow entry delays plus rfwo and rfwu. Every completed recursion also "
                "pairs overflow and underflow, so the return columns are identical. Equating "
                "either window entry delay with the syscall entry candidate is unsupported."
            ),
            "mask_rom": (
                "The cell has no matched IRAM control and contains an interval-priced "
                "callx8/entry/retw sequence, so it cannot identify an exact fetch price."
            ),
        },
        "adoption": {
            "adopted": False,
            "engine_changed": False,
            "reason": (
                "R8(b) is unmet: no unused committed receipt validates the directly derived "
                "return and syscall-entry candidates through the measured engine. Window "
                "returns and mask-ROM fetch are not independently identifiable."
            ),
        },
    }
    print(json.dumps(result, indent=2, sort_keys=True))


if __name__ == "__main__":
    main()
