#!/usr/bin/env python3
"""Validate and analyze the Tier B decomposition cohort."""

from __future__ import annotations

import gzip
import hashlib
import json
import math
from fractions import Fraction
from pathlib import Path
from typing import Any


ROOT = Path(__file__).resolve().parent
PREFIX = "TINYDRAW_TIER_B_NDJSON "
SOURCE_COMMIT = "7a157d44a9da3312b1ecda2b45b116af2de28e63"
ARCHIVE_SUMS_SHA256 = "f8fdad6a863d6484e1a29ab3a103a3f36d5b29005111c0c4108b538a0ea3653a"
CAPTURES = (
    ("normal", 1, "captures/normal-boot-1.log.gz", "receipts/normal-boot-1.json"),
    ("normal", 2, "captures/normal-boot-2.log.gz", "receipts/normal-boot-2.json"),
    (
        "xip-psram",
        1,
        "captures/xip-psram-boot-1.log.gz",
        "receipts/xip-psram-boot-1.json",
    ),
    (
        "xip-psram",
        2,
        "captures/xip-psram-boot-2.log.gz",
        "receipts/xip-psram-boot-2.json",
    ),
)
MSYNC_PREFIX = "msync_decompose_"
SPI2_PREFIX = "spi2_phased_"
MSYNC_COEFFICIENTS = (
    "fixedCycles",
    "addressedLineCycles",
    "dirtyLineCyclesAt80MHz",
    "slow40MHzFixedCycles",
    "slow40MHzAddressedLineCycles",
    "slow40MHzDirtyLineCycles",
)
SPI2_COEFFICIENTS = (
    "submissionFixedCycles",
    "submissionByteCycles",
    "submissionSlow20MHzFixedCycles",
    "submissionSlow20MHzByteCycles",
    "completionFixedCycles",
    "completionByteCycles",
    "completionSlow20MHzFixedCycles",
    "completionSlow20MHzByteCycles",
)


def fail(condition: bool, message: str) -> None:
    if not condition:
        raise ValueError(message)


def sha256(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def read_json(relative: str) -> dict[str, Any]:
    value = json.loads((ROOT / relative).read_text(encoding="utf-8"))
    fail(isinstance(value, dict), f"{relative} is not a JSON object")
    return value


def archive_hashes() -> dict[str, str]:
    hashes: dict[str, str] = {}
    for line in (ROOT / "archive-SHA256SUMS").read_text(encoding="utf-8").splitlines():
        digest, relative = line.split("  ", 1)
        fail(relative.startswith("./"), f"invalid archive path {relative}")
        fail(len(digest) == 64, f"invalid archive digest {digest}")
        hashes[relative[2:]] = digest
    return hashes


def source_session() -> dict[str, str]:
    values: dict[str, str] = {}
    for line in (ROOT / "source-session.txt").read_text(encoding="utf-8").splitlines():
        key, value = line.split("=", 1)
        values[key] = value
    return values


def manifest_contract() -> tuple[dict[str, Any], dict[str, list[str]], dict[str, int]]:
    manifest = read_json("probe-cells.json")
    fail(manifest["protocolVersion"] == 2, "manifest protocol mismatch")
    selected = {"normal": [], "xip-psram": []}
    samples: dict[str, int] = {}
    seen: set[str] = set()
    for cell in manifest["cells"]:
        cell_id = cell["id"]
        fail(cell_id not in seen, f"duplicate manifest cell {cell_id}")
        seen.add(cell_id)
        samples[cell_id] = cell["samples"]
        if cell.get("status") == "open-refusal":
            continue
        for variant in cell["variants"]:
            selected[variant].append(cell_id)
    fail(len(selected["normal"]) == 43, "normal manifest does not select 43 cells")
    fail(len(selected["xip-psram"]) == 44, "XIP manifest does not select 44 cells")
    fail(sum(samples[cell] for cell in selected["normal"]) == 360, "normal manifest sample mismatch")
    fail(sum(samples[cell] for cell in selected["xip-psram"]) == 373, "XIP manifest sample mismatch")
    return manifest, selected, samples


def parse_capture(relative: str) -> tuple[bytes, list[dict[str, Any]]]:
    with gzip.open(ROOT / relative, "rb") as stream:
        raw = stream.read()
    records: list[dict[str, Any]] = []
    for line_number, line in enumerate(raw.decode("utf-8").splitlines(), 1):
        if PREFIX not in line:
            continue
        record = json.loads(line.split(PREFIX, 1)[1])
        fail(isinstance(record, dict), f"{relative}:{line_number} is not an object")
        fail(record.get("protocolVersion") == 2, f"{relative}:{line_number} protocol mismatch")
        records.append(record)
    return raw, records


def median(values: list[int]) -> int | float:
    ordered = sorted(values)
    middle = len(ordered) // 2
    if len(ordered) % 2:
        return ordered[middle]
    total = ordered[middle - 1] + ordered[middle]
    return total // 2 if total % 2 == 0 else total / 2


def median_fraction(values: list[int]) -> Fraction:
    ordered = sorted(values)
    middle = len(ordered) // 2
    if len(ordered) % 2:
        return Fraction(ordered[middle])
    return Fraction(ordered[middle - 1] + ordered[middle], 2)


def number(value: Fraction) -> int | float:
    return value.numerator if value.denominator == 1 else round(float(value), 9)


def statistics(values: list[int]) -> dict[str, int | float]:
    ordered = sorted(values)
    return {
        "samples": len(ordered),
        "min": ordered[0],
        "median": median(ordered),
        "p90": ordered[math.ceil(0.9 * len(ordered)) - 1],
        "max": ordered[-1],
    }


def fixed_classification(values: list[int]) -> str:
    spread = max(values) - min(values)
    if spread == 0:
        return "exact"
    if spread == 1:
        return "interval"
    return "distribution"


def fraction_statistics(values: list[Fraction]) -> dict[str, Any]:
    ordered = sorted(values)
    middle = len(ordered) // 2
    midpoint = (
        ordered[middle]
        if len(ordered) % 2
        else (ordered[middle - 1] + ordered[middle]) / 2
    )
    return {
        "samples": len(ordered),
        "min": number(ordered[0]),
        "median": number(midpoint),
        "p90": number(ordered[math.ceil(0.9 * len(ordered)) - 1]),
        "max": number(ordered[-1]),
        "classification": "exact" if len(set(ordered)) == 1 else "distribution",
    }


def preflight_path(variant: str) -> str:
    return f"preflight/{variant}.json"


def validate_capture(
    variant: str,
    boot: int,
    capture_path: str,
    receipt_path: str,
    selected: dict[str, list[str]],
    expected_samples: dict[str, int],
    archive: dict[str, str],
) -> tuple[dict[str, Any], dict[str, list[dict[str, Any]]]]:
    raw, records = parse_capture(capture_path)
    receipt = read_json(receipt_path)
    preflight = read_json(preflight_path(variant))
    manifest_bytes = (ROOT / "probe-cells.json").read_bytes()
    preflight_bytes = (ROOT / preflight_path(variant)).read_bytes()
    receipt_bytes = (ROOT / receipt_path).read_bytes()
    expected = selected[variant]

    allowed = {"metadata", "cell-start", "sample", "cell-complete", "run-complete", "refusal"}
    fail({record.get("record") for record in records} <= allowed, f"{capture_path} has an unknown record")
    fail(records[0].get("record") == "metadata", f"{capture_path} metadata is not first")
    fail(records[-1].get("record") == "run-complete", f"{capture_path} completion is not last")
    metadata = [record for record in records if record.get("record") == "metadata"]
    complete = [record for record in records if record.get("record") == "run-complete"]
    refusals = [record for record in records if record.get("record") == "refusal"]
    fail(len(metadata) == 1 and len(complete) == 1, f"{capture_path} framing mismatch")
    fail(not refusals, f"{capture_path} contains refusals")
    fail([record["cell"] for record in records if record.get("record") == "cell-start"] == expected, f"{capture_path} cell-start order mismatch")
    fail([record["cell"] for record in records if record.get("record") == "cell-complete"] == expected, f"{capture_path} cell-complete order mismatch")

    runtime = metadata[0]
    fail(runtime["variant"] == variant, f"{capture_path} variant mismatch")
    fail(runtime["selectedCells"] == expected, f"{capture_path} selected cells mismatch")
    fail(runtime["availableCells"] == expected, f"{capture_path} available cells mismatch")
    fail(runtime["gitCommit"] == SOURCE_COMMIT and runtime["gitDirty"] is False, f"{capture_path} source mismatch")
    fail(runtime["idfVersion"] == "v6.1", f"{capture_path} IDF mismatch")
    fail(runtime["compilerVersion"] == "15.2.0", f"{capture_path} compiler mismatch")
    fail(runtime["chipModel"] == "ESP32-S3" and runtime["chipRevision"] == 2, f"{capture_path} chip mismatch")

    by_cell: dict[str, list[dict[str, Any]]] = {}
    for cell in expected:
        starts = [record for record in records if record.get("record") == "cell-start" and record.get("cell") == cell]
        samples = [record for record in records if record.get("record") == "sample" and record.get("cell") == cell]
        ends = [record for record in records if record.get("record") == "cell-complete" and record.get("cell") == cell]
        count = expected_samples[cell]
        fail(len(starts) == 1 and starts[0]["expectedSamples"] == count, f"{capture_path} {cell} start mismatch")
        fail(len(ends) == 1 and ends[0]["samples"] == count, f"{capture_path} {cell} completion mismatch")
        fail(len(samples) == count, f"{capture_path} {cell} sample count mismatch")
        fail([sample["ordinal"] for sample in samples] == list(range(count)), f"{capture_path} {cell} ordinals mismatch")
        fail(all(isinstance(sample.get("cycles"), int) and sample["cycles"] > 0 for sample in samples), f"{capture_path} {cell} cycles mismatch")
        by_cell[cell] = samples

    expected_total = sum(expected_samples[cell] for cell in expected)
    sample_records = [record for record in records if record.get("record") == "sample"]
    fail(len(sample_records) == expected_total, f"{capture_path} total samples mismatch")
    fail(
        complete[0]
        == {
            "protocolVersion": 2,
            "record": "run-complete",
            "selectedCells": len(expected),
            "completedCells": len(expected),
            "samples": expected_total,
            "refusals": 0,
        },
        f"{capture_path} terminal tally mismatch",
    )

    capture_sha = sha256(raw)
    fail(receipt["captureSha256"] == capture_sha, f"{capture_path} receipt hash mismatch")
    fail(receipt["runtimeMetadata"] == runtime, f"{capture_path} receipt metadata mismatch")
    fail(receipt["bootIdentity"] == runtime["bootId"], f"{capture_path} boot identity mismatch")
    fail(receipt["request"] == {"variant": variant, "cells": expected}, f"{capture_path} request mismatch")
    fail(receipt["tally"] == {
        "capturedSamples": expected_total,
        "complete": True,
        "completedCells": len(expected),
        "expectedCells": len(expected),
        "expectedSamples": expected_total,
        "refusals": 0,
    }, f"{capture_path} sidecar tally mismatch")
    fail(receipt["manifestSha256"] == sha256(manifest_bytes), f"{capture_path} manifest hash mismatch")
    fail(receipt["preflightSha256"] == sha256(preflight_bytes), f"{capture_path} preflight hash mismatch")
    fail(receipt["elfVerification"] == preflight, f"{capture_path} embedded preflight mismatch")
    fail(preflight["ok"] is True and preflight["fixture"] is False, f"{capture_path} preflight refusal")
    fail(preflight["variant"] == variant, f"{capture_path} preflight variant mismatch")
    fail(preflight["gitCommit"] == SOURCE_COMMIT and preflight["gitDirty"] is False, f"{capture_path} preflight source mismatch")
    fail(preflight["manifestSha256"] == sha256(manifest_bytes), f"{capture_path} preflight manifest mismatch")
    fail(preflight["elfSha256"] == runtime["elfSha256"], f"{capture_path} ELF mismatch")
    fail(preflight["sdkconfigSha256"] == runtime["sdkconfigSha256"], f"{capture_path} sdkconfig mismatch")

    archive_capture = f"{variant}/boot-{boot}/serial.log"
    archive_receipt = f"{archive_capture}.receipt.json"
    fail(archive[archive_capture] == capture_sha, f"{capture_path} archive hash mismatch")
    fail(archive[archive_receipt] == sha256(receipt_bytes), f"{capture_path} archive receipt mismatch")

    summary = {
        "variant": variant,
        "boot": boot,
        "bootIdentity": runtime["bootId"],
        "cells": len(expected),
        "samples": expected_total,
        "refusals": 0,
        "captureSha256": capture_sha,
    }
    return summary, by_cell


def exact_rank(matrix: list[list[int]]) -> int:
    rows = [[Fraction(value) for value in row] for row in matrix]
    rank = 0
    columns = len(rows[0])
    for column in range(columns):
        pivot = next((index for index in range(rank, len(rows)) if rows[index][column]), None)
        if pivot is None:
            continue
        rows[rank], rows[pivot] = rows[pivot], rows[rank]
        divisor = rows[rank][column]
        rows[rank] = [value / divisor for value in rows[rank]]
        for index, row in enumerate(rows):
            if index == rank or not row[column]:
                continue
            factor = row[column]
            rows[index] = [value - factor * pivot_value for value, pivot_value in zip(row, rows[rank])]
        rank += 1
    return rank


def solve(matrix: list[list[Fraction]], values: list[Fraction]) -> list[Fraction]:
    size = len(values)
    rows = [row[:] + [value] for row, value in zip(matrix, values)]
    for column in range(size):
        pivot = next((index for index in range(column, size) if rows[index][column]), None)
        fail(pivot is not None, "singular least-squares matrix")
        rows[column], rows[pivot] = rows[pivot], rows[column]
        divisor = rows[column][column]
        rows[column] = [value / divisor for value in rows[column]]
        for index, row in enumerate(rows):
            if index == column or not row[column]:
                continue
            factor = row[column]
            rows[index] = [value - factor * pivot_value for value, pivot_value in zip(row, rows[column])]
    return [row[-1] for row in rows]


def least_squares(
    observations: list[tuple[list[int], int | Fraction]], names: tuple[str, ...]
) -> tuple[dict[str, Any], list[Fraction]]:
    width = len(names)
    design = [row for row, _ in observations]
    fail(exact_rank(design) == width, "design matrix is not full rank")
    normal = [[Fraction(0) for _ in range(width)] for _ in range(width)]
    target = [Fraction(0) for _ in range(width)]
    for row, value in observations:
        for left in range(width):
            target[left] += Fraction(row[left]) * Fraction(value)
            for right in range(width):
                normal[left][right] += Fraction(row[left] * row[right])
    coefficients = solve(normal, target)
    observed = [Fraction(value) for _, value in observations]
    predictions = [sum(coefficient * value for coefficient, value in zip(coefficients, row)) for row, _ in observations]
    residuals = [value - prediction for value, prediction in zip(observed, predictions)]
    mean = Fraction(sum(observed), len(observed))
    residual_sum = sum(residual * residual for residual in residuals)
    total_sum = sum((Fraction(value) - mean) ** 2 for value in observed)
    r_squared = Fraction(1) - residual_sum / total_sum if total_sum else Fraction(1)
    rmse = math.sqrt(float(residual_sum / len(residuals)))
    maximum = max(abs(float(residual)) for residual in residuals)
    observed_range = max(observed) - min(observed)
    classification = (
        "affine"
        if float(r_squared) >= 0.999
        and maximum <= max(2.0, 0.02 * float(observed_range))
        else "unexplained"
    )
    result = {
        "classification": classification,
        "observations": len(observations),
        "designRank": exact_rank(design),
        "coefficients": {
            name: round(float(value), 9) for name, value in zip(names, coefficients)
        },
        "rSquared": round(float(r_squared), 12),
        "rmseCycles": round(rmse, 6),
        "maxAbsoluteResidualCycles": round(maximum, 6),
        "observedRangeCycles": number(observed_range),
    }
    return result, coefficients


def residual_check(
    observations: list[tuple[list[int], int | Fraction]], coefficients: list[Fraction]
) -> dict[str, Any]:
    residuals = [
        Fraction(value) - sum(coefficient * axis for coefficient, axis in zip(coefficients, row))
        for row, value in observations
    ]
    squared = sum(residual * residual for residual in residuals)
    return {
        "observations": len(observations),
        "rmseCycles": round(math.sqrt(float(squared / len(residuals))), 6),
        "maxAbsoluteResidualCycles": round(max(abs(float(value)) for value in residuals), 6),
    }


def msync_design(sample: dict[str, Any]) -> list[int]:
    lines = sample["bytes"] // 64
    dirty = sample["dirtyLines"]
    slow = int(sample["psramClockHz"] == 40_000_000)
    return [1, lines, dirty, slow, lines * slow, dirty * slow]


def spi2_design(sample: dict[str, Any], phase: str) -> list[int]:
    byte_count = sample["bytes"]
    slow = int(sample["spiClockHz"] == 20_000_000)
    terms = [1, byte_count, slow, byte_count * slow]
    return terms + [0, 0, 0, 0] if phase == "submission" else [0, 0, 0, 0] + terms


def validate_decomposition_sample(sample: dict[str, Any]) -> None:
    fail(sample["startCore"] == 0 and sample["endCore"] == 0, f"{sample['cell']} core migration")
    if sample["cell"].startswith(MSYNC_PREFIX):
        lines = sample["bytes"] // 64
        dirty = sample["dirtyLines"]
        clock = sample["psramClockHz"] // 1_000_000
        fail(lines in (1, 16, 512), f"{sample['cell']} addressed lines mismatch")
        fail(dirty in (0, lines), f"{sample['cell']} dirty lines mismatch")
        fail(clock in (40, 80), f"{sample['cell']} PSRAM clock mismatch")
        fail(sample["cell"] == f"msync_decompose_l{lines}_d{dirty}_p{clock}", f"{sample['cell']} factors mismatch")
        expected_register = 196867 if clock == 40 else 65537
        fail(sample["psramClockRegister"] == expected_register, f"{sample['cell']} clock readback mismatch")
        fail(sample["psramCoreClockRegister"] == 2, f"{sample['cell']} core clock readback mismatch")
        fail(sample["psramServiceBytes"] == 4096 and sample["psramServiceCycles"] > 0, f"{sample['cell']} service control mismatch")
        counters = sample["psramServiceCounters"]
        fail(counters["dbusAccesses"] == 64 and counters["dbusPsramMisses"] == 64, f"{sample['cell']} service counters mismatch")
        fail(counters["dbusFlashMisses"] == 0, f"{sample['cell']} service source mismatch")
    elif sample["cell"].startswith(SPI2_PREFIX):
        byte_count = sample["bytes"]
        clock = sample["spiClockHz"] // 1_000_000
        fail(byte_count in (64, 4096, 32768), f"{sample['cell']} payload mismatch")
        fail(clock in (20, 40), f"{sample['cell']} SPI2 clock mismatch")
        fail(sample["cell"] == f"spi2_phased_b{byte_count}_c{clock}", f"{sample['cell']} factors mismatch")
        fail(sample["submissionCycles"] > 0 and sample["completionCycles"] > 0, f"{sample['cell']} phase timing mismatch")
        fail(sample["submissionCycles"] + sample["completionCycles"] == sample["cycles"], f"{sample['cell']} phase reconciliation mismatch")


def main() -> None:
    manifest, selected, expected_samples = manifest_contract()
    archive = archive_hashes()
    reference = read_json("archive-reference.json")
    session = source_session()
    fail(sha256((ROOT / "archive-SHA256SUMS").read_bytes()) == ARCHIVE_SUMS_SHA256, "archive checksum file mismatch")
    fail(reference["archiveSha256SumsSha256"] == ARCHIVE_SUMS_SHA256, "archive reference mismatch")
    fail(reference["source"]["commit"] == SOURCE_COMMIT and reference["source"]["dirty"] is False, "source reference mismatch")
    fail(reference["source"]["manifestSha256"] == sha256((ROOT / "probe-cells.json").read_bytes()), "source manifest mismatch")
    fail(reference["source"]["sessionSha256"] == sha256((ROOT / "source-session.txt").read_bytes()), "source session mismatch")
    source_paths = {
        "captureToolSha256": "provenance/tier-b-capture.py",
        "validatorSha256": "provenance/tier_b_ndjson.py",
        "elfVerifierSha256": "provenance/verify_elf.py",
        "draftVerifierSha256": "provenance/verify_draft.py",
        "sessionSha256": "provenance/session.txt",
        "manifestSha256": "provenance/probe-cells.json",
    }
    for field, path in source_paths.items():
        fail(reference["source"][field] == archive[path], f"source archive mismatch: {path}")
    for artifact in reference["largeArtifacts"]:
        fail(archive[artifact["path"]] == artifact["sha256"], f"archive artifact mismatch: {artifact['path']}")
    fail(session["tinydraw_commit"] == SOURCE_COMMIT, "session commit mismatch")
    fail(session["idf_version"] == "v6.1", "session IDF mismatch")
    fail(session["manifest_sha256"] == sha256((ROOT / "probe-cells.json").read_bytes()), "session manifest mismatch")
    for variant in ("normal", "xip-psram"):
        fail(
            archive[f"builds/{variant}/elf-verification.json"]
            == sha256((ROOT / preflight_path(variant)).read_bytes()),
            f"{variant} archived preflight mismatch",
        )

    summaries: list[dict[str, Any]] = []
    captures: dict[tuple[str, int], dict[str, list[dict[str, Any]]]] = {}
    for capture in CAPTURES:
        summary, samples = validate_capture(*capture, selected, expected_samples, archive)
        summaries.append(summary)
        captures[(capture[0], capture[1])] = samples
    identities = [summary["bootIdentity"] for summary in summaries]
    fail(len(set(identities)) == 4, "boot identities are not distinct")
    fail(session["normal_boot_1"] == identities[0] and session["normal_boot_2"] == identities[1], "normal session boot mismatch")
    fail(session["xip_psram_boot_1"] == identities[2] and session["xip_psram_boot_2"] == identities[3], "XIP session boot mismatch")

    decomposition_cells = [
        cell["id"]
        for cell in manifest["cells"]
        if cell["id"].startswith(MSYNC_PREFIX) or cell["id"].startswith(SPI2_PREFIX)
    ]
    fail(len(decomposition_cells) == 18, "decomposition manifest cell count mismatch")
    for samples in captures.values():
        for cell in decomposition_cells:
            for sample in samples[cell]:
                validate_decomposition_sample(sample)

    per_capture: dict[str, Any] = {}
    for variant, boot, _, _ in CAPTURES:
        key = f"{variant}-boot-{boot}"
        per_capture[key] = {}
        for cell in decomposition_cells:
            records = captures[(variant, boot)][cell]
            entry: dict[str, Any] = {
                "cycles": {
                    **statistics([record["cycles"] for record in records]),
                    "classification": fixed_classification([record["cycles"] for record in records]),
                }
            }
            if cell.startswith(SPI2_PREFIX):
                for field in ("submissionCycles", "completionCycles"):
                    values = [record[field] for record in records]
                    entry[field] = {
                        **statistics(values),
                        "classification": fixed_classification(values),
                    }
            per_capture[key][cell] = entry

    pooled: dict[str, Any] = {}
    for cell in decomposition_cells:
        records = [
            record
            for variant, boot, _, _ in CAPTURES
            for record in captures[(variant, boot)][cell]
        ]
        entry = {
            "cycles": {
                **statistics([record["cycles"] for record in records]),
                "classification": fixed_classification([record["cycles"] for record in records]),
            }
        }
        if cell.startswith(SPI2_PREFIX):
            for field in ("submissionCycles", "completionCycles"):
                values = [record[field] for record in records]
                entry[field] = {
                    **statistics(values),
                    "classification": fixed_classification(values),
                }
        pooled[cell] = entry

    msync_medians: list[tuple[list[int], int]] = []
    msync_raw: list[tuple[list[int], int]] = []
    spi2_medians: list[tuple[list[int], int]] = []
    spi2_raw: list[tuple[list[int], int]] = []
    per_capture_models: dict[str, Any] = {}
    reconciliation = 0
    for variant, boot, _, _ in CAPTURES:
        capture = captures[(variant, boot)]
        capture_msync: list[tuple[list[int], int]] = []
        capture_spi2: list[tuple[list[int], int]] = []
        for cell in decomposition_cells:
            records = capture[cell]
            if cell.startswith(MSYNC_PREFIX):
                observation = (msync_design(records[0]), int(median([record["cycles"] for record in records])))
                capture_msync.append(observation)
                msync_medians.append(observation)
                msync_raw.extend((msync_design(record), record["cycles"]) for record in records)
            else:
                for phase, field in (("submission", "submissionCycles"), ("completion", "completionCycles")):
                    observation = (spi2_design(records[0], phase), int(median([record[field] for record in records])))
                    capture_spi2.append(observation)
                    spi2_medians.append(observation)
                    spi2_raw.extend((spi2_design(record, phase), record[field]) for record in records)
                reconciliation += len(records)
        msync_fit, _ = least_squares(capture_msync, MSYNC_COEFFICIENTS)
        spi2_fit, _ = least_squares(capture_spi2, SPI2_COEFFICIENTS)
        per_capture_models[f"{variant}-boot-{boot}"] = {
            "msync": msync_fit,
            "spi2PhaseExpanded": spi2_fit,
        }

    msync_fit, msync_coefficients = least_squares(msync_medians, MSYNC_COEFFICIENTS)
    msync_fit["rawResiduals"] = residual_check(msync_raw, msync_coefficients)
    msync_fit["design"] = "[1,L,D,S40,L*S40,D*S40]"
    msync_fit["medianTotalFitClassification"] = msync_fit["classification"]
    clean_observations: list[tuple[list[int], Fraction]] = []
    dirty_observations: list[tuple[list[int], int]] = []
    for lines in (1, 16, 512):
        for clock in (40, 80):
            clean_cell = f"msync_decompose_l{lines}_d0_p{clock}"
            clean_values = [
                record["cycles"]
                for samples in captures.values()
                for record in samples[clean_cell]
            ]
            slow = int(clock == 40)
            clean_observations.append(
                ([1, lines, slow, lines * slow], median_fraction(clean_values))
            )
            dirty_cell = f"msync_decompose_l{lines}_d{lines}_p{clock}"
            for samples in captures.values():
                dirty_delta = int(
                    median([record["cycles"] for record in samples[dirty_cell]])
                    - median([record["cycles"] for record in samples[clean_cell]])
                )
                dirty_observations.append(([lines, lines * slow], dirty_delta))
    clean_fit, _ = least_squares(
        clean_observations,
        ("fixedCycles", "addressedLineCycles", "slow40MHzFixedCycles", "slow40MHzAddressedLineCycles"),
    )
    dirty_fit, _ = least_squares(
        dirty_observations,
        ("dirtyLineCyclesAt80MHz", "slow40MHzDirtyLineCycles"),
    )
    msync_fit["components"] = {
        "matchedCleanBaseline": clean_fit,
        "dirtyWritebackDelta": dirty_fit,
    }
    msync_fit["classification"] = "unexplained"
    msync_fit["transactionBoundary"] = "CCOUNT around esp_cache_msync(..., ESP_CACHE_MSYNC_FLAG_DIR_C2M)"
    msync_fit["stability"] = "dirty-delta medians repeat across both boots and differ by at most 8 cycles at 512 lines between firmware variants"
    msync_fit["productAdoptable"] = False
    msync_fit["productDisposition"] = "dirty writeback is an affine candidate inside a typed C2M boundary, but the matched-clean baseline is unexplained and product accounting still needs a non-double-counted cache-msync transaction"

    spi2_fit, spi2_coefficients = least_squares(spi2_medians, SPI2_COEFFICIENTS)
    spi2_fit["rawResiduals"] = residual_check(spi2_raw, spi2_coefficients)
    spi2_fit["design"] = "phase-expanded [1,B,S20,B*S20] with disjoint submission and completion columns"
    submission_observations = [(row[:4], value) for row, value in spi2_medians if any(row[:4])]
    completion_observations = [(row[4:], value) for row, value in spi2_medians if any(row[4:])]
    submission_fit, _ = least_squares(submission_observations, SPI2_COEFFICIENTS[:4])
    completion_fit, _ = least_squares(completion_observations, SPI2_COEFFICIENTS[4:])
    submission_fit["affineFitClassification"] = submission_fit["classification"]
    submission_fit["classification"] = "distribution"
    completion_fit["affineFitClassification"] = completion_fit["classification"]
    completion_fit["classification"] = "unexplained"
    spi2_fit["phases"] = {"submission": submission_fit, "completion": completion_fit}
    spi2_fit["classification"] = "unexplained"
    spi2_fit["reconciliation"] = {
        "samples": reconciliation,
        "exact": True,
        "maximumDifferenceCycles": 0,
    }
    spi2_fit["transactionBoundary"] = {
        "submission": "CCOUNT around spi_device_queue_trans",
        "completion": "CCOUNT from queue return through spi_device_get_trans_result return",
    }
    serialization_20: list[Fraction] = []
    serialization_40_steady: list[Fraction] = []
    serialization_40_short: list[Fraction] = []
    fixed_20: list[int] = []
    fixed_40: list[int] = []
    blocking_differences: dict[str, list[int]] = {"64": [], "4096": [], "32768": []}
    sweep_ordinals = {64: 0, 4096: 3, 32768: 5}
    for samples in captures.values():
        completion_medians: dict[tuple[int, int], int] = {}
        total_medians: dict[tuple[int, int], int] = {}
        for clock in (20, 40):
            for byte_count in (64, 4096, 32768):
                cell = f"spi2_phased_b{byte_count}_c{clock}"
                records = samples[cell]
                completion_medians[(clock, byte_count)] = int(
                    median([record["completionCycles"] for record in records])
                )
                total_medians[(clock, byte_count)] = int(
                    median([record["cycles"] for record in records])
                )
        for left, right in ((64, 4096), (4096, 32768)):
            serialization_20.append(
                Fraction(
                    completion_medians[(20, right)] - completion_medians[(20, left)],
                    right - left,
                )
            )
        serialization_40_short.append(
            Fraction(
                completion_medians[(40, 4096)] - completion_medians[(40, 64)],
                4096 - 64,
            )
        )
        serialization_40_steady.append(
            Fraction(
                completion_medians[(40, 32768)] - completion_medians[(40, 4096)],
                32768 - 4096,
            )
        )
        for byte_count in (64, 4096, 32768):
            fixed_20.append(completion_medians[(20, byte_count)] - 96 * byte_count)
            fixed_40.append(completion_medians[(40, byte_count)] - 48 * byte_count)
            blocking = next(
                record["cycles"]
                for record in samples["spi2_transfer_sweep"]
                if record["ordinal"] == sweep_ordinals[byte_count]
            )
            blocking_differences[str(byte_count)].append(
                total_medians[(40, byte_count)] - blocking
            )
    spi2_fit["deviceSerializationCandidates"] = {
        "20MHz": {
            "scope": "both adjacent intervals from 64 through 32768 bytes",
            "cyclesPerByte": fraction_statistics(serialization_20),
        },
        "40MHzSteadyState": {
            "scope": "4096 through 32768 bytes only",
            "cyclesPerByte": fraction_statistics(serialization_40_steady),
        },
        "40MHzShortTransition": {
            "scope": "64 through 4096 bytes",
            "cyclesPerByte": fraction_statistics(serialization_40_short),
        },
    }
    spi2_fit["completionFixedCycles"] = {
        "20MHz": {
            **statistics(fixed_20),
            "classification": fixed_classification(fixed_20),
        },
        "40MHz": {
            **statistics(fixed_40),
            "classification": fixed_classification(fixed_40),
        },
    }
    spi2_fit["priorBlockingComparisonAt40MHz"] = {
        byte_count: {
            **statistics(values),
            "classification": fixed_classification(values),
        }
        for byte_count, values in blocking_differences.items()
    }
    spi2_fit["productAdoptable"] = False
    spi2_fit["productDisposition"] = "submission and completion fixed costs remain non-affine; only scoped device serialization slopes are exact candidates, and neither IDF API phase is bound to a non-double-counted product transaction"

    service: dict[str, Any] = {}
    for clock in (40_000_000, 80_000_000):
        values = [
            record["psramServiceCycles"]
            for samples in captures.values()
            for cell, records in samples.items()
            if cell.startswith(MSYNC_PREFIX)
            for record in records
            if record["psramClockHz"] == clock
        ]
        service[str(clock)] = {
            **statistics(values),
            "classification": fixed_classification(values),
        }

    output = {
        "schemaVersion": 1,
        "suite": "tier-b-decomposition",
        "disposition": "candidate-evidence-only",
        "sourceCommit": SOURCE_COMMIT,
        "manifestSha256": sha256((ROOT / "probe-cells.json").read_bytes()),
        "archiveSha256SumsSha256": ARCHIVE_SUMS_SHA256,
        "statistics": {
            "median": "arithmetic midpoint of the two middle sorted values for even sample counts",
            "p90": "nearest rank ceil(0.90 * n) on ascending values",
            "modelInput": "one median for each cell in each independent boot and firmware variant",
        },
        "captures": summaries,
        "decomposition": {
            "perCapture": per_capture,
            "pooled": pooled,
            "psramServiceControlCycles": service,
            "msync": msync_fit,
            "spi2": spi2_fit,
            "perCaptureModels": per_capture_models,
        },
        "open": [
            "bind cache-msync accounting to a product transaction without double counting CPU execution",
            "resolve the non-affine SPI2 submission phase",
            "bind SPI2 completion accounting to the product GDMA and peripheral transaction boundary",
            "gpio21_edge",
        ],
        "adoptedMeasuredModeCosts": [],
    }
    print(json.dumps(output, indent=2, sort_keys=True) + "\n", end="")


if __name__ == "__main__":
    main()
