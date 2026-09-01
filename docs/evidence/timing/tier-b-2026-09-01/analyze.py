#!/usr/bin/env python3
"""Validate and summarize the committed Tier B capture cohort."""

from __future__ import annotations

import gzip
import hashlib
import json
import math
from pathlib import Path
from typing import Any


ROOT = Path(__file__).resolve().parent
PREFIX = "TINYDRAW_TIER_B_NDJSON "
SOURCE_COMMIT = "fc6d9347549730a0e57aa926f8f6935e12636844"
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
SWEEPS = (
    "panel_qspi_flush_sweep",
    "gdma_transfer_sweep",
    "spi2_transfer_sweep",
    "cache_msync_writeback_sweep",
    "cache_msync_invalidate_clean_sweep",
)
CLASSIFICATIONS = {"exact", "interval", "distribution", "affine", "unexplained"}


def fail(condition: bool, message: str) -> None:
    if not condition:
        raise ValueError(message)


def sha256(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def read_json(relative: str) -> dict[str, Any]:
    payload = json.loads((ROOT / relative).read_text(encoding="utf-8"))
    fail(isinstance(payload, dict), f"{relative} is not a JSON object")
    return payload


def archive_hashes() -> dict[str, str]:
    result: dict[str, str] = {}
    for line in (ROOT / "archive-SHA256SUMS").read_text(encoding="utf-8").splitlines():
        digest, relative = line.split("  ", 1)
        fail(relative.startswith("./"), f"invalid archive checksum path {relative}")
        fail(len(digest) == 64, f"invalid archive checksum {digest}")
        result[relative[2:]] = digest
    return result


def manifest_contract() -> tuple[dict[str, Any], dict[str, list[str]], dict[str, int]]:
    manifest = read_json("probe-cells.json")
    fail(manifest["protocolVersion"] == 2, "manifest protocol is not 2")
    selected = {"normal": [], "xip-psram": []}
    expected_samples: dict[str, int] = {}
    seen: set[str] = set()
    for cell in manifest["cells"]:
        cell_id = cell["id"]
        fail(cell_id not in seen, f"duplicate manifest cell {cell_id}")
        seen.add(cell_id)
        expected_samples[cell_id] = cell["samples"]
        if cell.get("status") == "open-refusal":
            continue
        for variant in cell["variants"]:
            selected[variant].append(cell_id)
    fail(len(selected["normal"]) == 25, "normal manifest does not select 25 cells")
    fail(len(selected["xip-psram"]) == 26, "XIP manifest does not select 26 cells")
    return manifest, selected, expected_samples


def parse_capture(relative: str) -> tuple[bytes, list[dict[str, Any]]]:
    with gzip.open(ROOT / relative, "rb") as stream:
        raw = stream.read()
    records: list[dict[str, Any]] = []
    for line_number, line in enumerate(raw.decode("utf-8").splitlines(), 1):
        if PREFIX not in line:
            continue
        payload = json.loads(line.split(PREFIX, 1)[1])
        fail(isinstance(payload, dict), f"{relative}:{line_number} is not an object")
        fail(payload.get("protocolVersion") == 2, f"{relative}:{line_number} protocol mismatch")
        records.append(payload)
    return raw, records


def median(values: list[int]) -> int | float:
    ordered = sorted(values)
    middle = len(ordered) // 2
    if len(ordered) % 2:
        return ordered[middle]
    total = ordered[middle - 1] + ordered[middle]
    return total // 2 if total % 2 == 0 else total / 2


def stats(values: list[int]) -> dict[str, int | float]:
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


def validate_capture(
    variant: str,
    boot: int,
    capture_path: str,
    receipt_path: str,
    selected: dict[str, list[str]],
    expected_samples: dict[str, int],
    session: dict[str, Any],
) -> tuple[dict[str, Any], dict[str, list[dict[str, Any]]]]:
    raw, records = parse_capture(capture_path)
    receipt = read_json(receipt_path)
    preflight_path = "preflight/normal.json" if variant == "normal" else "preflight/xip-psram.json"
    preflight = read_json(preflight_path)
    manifest_bytes = (ROOT / "probe-cells.json").read_bytes()
    preflight_bytes = (ROOT / preflight_path).read_bytes()
    receipt_bytes = (ROOT / receipt_path).read_bytes()

    expected = selected[variant]
    metadata = [record for record in records if record.get("record") == "metadata"]
    run_complete = [record for record in records if record.get("record") == "run-complete"]
    refusals = [record for record in records if record.get("record") == "refusal"]
    record_types = {record.get("record") for record in records}
    fail(record_types <= {"metadata", "cell-start", "sample", "cell-complete", "run-complete", "refusal"}, f"{capture_path} contains an unknown record type")
    fail(len(metadata) == 1, f"{capture_path} does not have one metadata record")
    fail(len(run_complete) == 1, f"{capture_path} does not have one terminal record")
    fail(not refusals, f"{capture_path} contains refusals")
    fail(records[0].get("record") == "metadata", f"{capture_path} metadata is not first")
    fail(records[-1].get("record") == "run-complete", f"{capture_path} completion is not last")
    fail([record["cell"] for record in records if record.get("record") == "cell-start"] == expected, f"{capture_path} cell-start order mismatch")
    fail([record["cell"] for record in records if record.get("record") == "cell-complete"] == expected, f"{capture_path} cell-complete order mismatch")
    runtime = metadata[0]
    fail(runtime["variant"] == variant, f"{capture_path} variant mismatch")
    fail(runtime["selectedCells"] == expected, f"{capture_path} selected cells mismatch")
    fail(runtime["availableCells"] == expected, f"{capture_path} available cells mismatch")
    fail(runtime["gitCommit"] == SOURCE_COMMIT, f"{capture_path} source mismatch")
    fail(runtime["gitDirty"] is False, f"{capture_path} dirty source")
    fail(runtime["idfVersion"] == "v6.1", f"{capture_path} IDF mismatch")
    fail(runtime["compilerVersion"] == "15.2.0", f"{capture_path} compiler mismatch")
    fail(runtime["chipModel"] == "ESP32-S3" and runtime["chipRevision"] == 2, f"{capture_path} chip mismatch")

    samples_by_cell: dict[str, list[dict[str, Any]]] = {}
    for cell in expected:
        starts = [record for record in records if record.get("record") == "cell-start" and record.get("cell") == cell]
        samples = [record for record in records if record.get("record") == "sample" and record.get("cell") == cell]
        completes = [record for record in records if record.get("record") == "cell-complete" and record.get("cell") == cell]
        count = expected_samples[cell]
        fail(len(starts) == 1 and starts[0]["expectedSamples"] == count, f"{capture_path} {cell} start mismatch")
        fail(len(completes) == 1 and completes[0]["samples"] == count, f"{capture_path} {cell} completion mismatch")
        fail(len(samples) == count, f"{capture_path} {cell} sample count mismatch")
        fail([sample["ordinal"] for sample in samples] == list(range(count)), f"{capture_path} {cell} ordinals mismatch")
        fail(all(isinstance(sample.get("cycles"), int) and sample["cycles"] >= 0 for sample in samples), f"{capture_path} {cell} cycle value mismatch")
        samples_by_cell[cell] = samples

    sample_records = [record for record in records if record.get("record") == "sample"]
    expected_total = sum(expected_samples[cell] for cell in expected)
    terminal = run_complete[0]
    fail(len(sample_records) == expected_total, f"{capture_path} total sample mismatch")
    fail(terminal == {
        "protocolVersion": 2,
        "record": "run-complete",
        "selectedCells": len(expected),
        "completedCells": len(expected),
        "samples": expected_total,
        "refusals": 0,
    }, f"{capture_path} terminal tally mismatch")

    capture_sha = sha256(raw)
    fail(receipt["captureSha256"] == capture_sha, f"{capture_path} receipt hash mismatch")
    fail(receipt["runtimeMetadata"] == runtime, f"{capture_path} receipt metadata mismatch")
    fail(receipt["bootIdentity"] == runtime["bootId"], f"{capture_path} boot identity mismatch")
    fail(receipt["request"] == {"variant": variant, "cells": expected}, f"{capture_path} request mismatch")
    fail(receipt["manifestSha256"] == sha256(manifest_bytes), f"{capture_path} manifest hash mismatch")
    fail(receipt["preflightSha256"] == sha256(preflight_bytes), f"{capture_path} preflight hash mismatch")
    fail(receipt["elfVerification"] == preflight, f"{capture_path} preflight receipt mismatch")
    fail(preflight["ok"] is True and preflight["fixture"] is False, f"{capture_path} preflight did not authorize capture")
    fail(preflight["variant"] == variant and preflight["gitCommit"] == SOURCE_COMMIT, f"{capture_path} preflight provenance mismatch")
    fail(preflight["manifestSha256"] == sha256(manifest_bytes), f"{capture_path} preflight manifest mismatch")
    fail(preflight["elfSha256"] == runtime["elfSha256"], f"{capture_path} runtime ELF mismatch")
    fail(preflight["sdkconfigSha256"] == runtime["sdkconfigSha256"], f"{capture_path} runtime sdkconfig mismatch")

    session_capture = next(item for item in session["canonicalCaptures"] if item["variant"] == variant and item["boot"] == boot)
    fail(session_capture["bootIdentity"] == runtime["bootId"], f"{capture_path} session boot mismatch")
    fail(session_capture["captureSha256"] == capture_sha, f"{capture_path} session capture hash mismatch")
    fail(session_capture["receiptSha256"] == sha256(receipt_bytes), f"{capture_path} session receipt hash mismatch")
    fail(session_capture["cells"] == len(expected) and session_capture["samples"] == expected_total, f"{capture_path} session tally mismatch")
    fail(session_capture["refusals"] == 0, f"{capture_path} session refusal mismatch")

    archive = archive_hashes()
    archive_capture = f"{variant}/boot-{boot}/serial.log"
    archive_receipt = f"{archive_capture}.receipt.json"
    fail(archive[archive_capture] == capture_sha, f"{capture_path} archive capture hash mismatch")
    fail(archive[archive_receipt] == sha256(receipt_bytes), f"{capture_path} archive receipt hash mismatch")

    per_cell = {
        cell: {**stats([sample["cycles"] for sample in samples]), "classification": fixed_classification([sample["cycles"] for sample in samples])}
        for cell, samples in samples_by_cell.items()
    }
    summary = {
        "variant": variant,
        "boot": boot,
        "bootIdentity": runtime["bootId"],
        "cells": len(expected),
        "samples": expected_total,
        "refusals": 0,
        "captureSha256": capture_sha,
        "perCell": per_cell,
    }
    return summary, samples_by_cell


def affine_fit(points: list[tuple[int, float]]) -> dict[str, Any]:
    x_mean = sum(x for x, _ in points) / len(points)
    y_mean = sum(y for _, y in points) / len(points)
    denominator = sum((x - x_mean) ** 2 for x, _ in points)
    slope = sum((x - x_mean) * (y - y_mean) for x, y in points) / denominator
    intercept = y_mean - slope * x_mean
    residuals = [y - (intercept + slope * x) for x, y in points]
    total = sum((y - y_mean) ** 2 for _, y in points)
    residual = sum(value**2 for value in residuals)
    r_squared = 1.0 - residual / total
    observed_range = max(y for _, y in points) - min(y for _, y in points)
    maximum = max(abs(value) for value in residuals)
    classification = "affine" if r_squared >= 0.999 and maximum <= max(2.0, 0.02 * observed_range) else "unexplained"
    return {
        "classification": classification,
        "points": len(points),
        "interceptCycles": round(intercept, 6),
        "slopeCyclesPerUnit": round(slope, 9),
        "rSquared": round(r_squared, 9),
        "maxAbsoluteResidualCycles": round(maximum, 6),
    }


def family_classification(classifications: list[str]) -> str:
    """Retain a common classification, or refuse a mixed family."""
    fail(bool(classifications), "cannot classify an empty family")
    fail(set(classifications) <= CLASSIFICATIONS, "unknown family classification")
    return classifications[0] if len(set(classifications)) == 1 else "unexplained"


def analysis_self_check() -> None:
    fail(median([4, 1, 3, 2]) == 2.5, "median self-check failed")
    fail(fixed_classification([7, 7]) == "exact", "exact self-check failed")
    fail(fixed_classification([7, 8]) == "interval", "interval self-check failed")
    fail(fixed_classification([7, 9]) == "distribution", "distribution self-check failed")
    fit = affine_fit([(1, 5.0), (2, 8.0), (4, 14.0)])
    fail(fit["classification"] == "affine", "affine self-check failed")
    fail(fit["interceptCycles"] == 2.0, "affine intercept self-check failed")
    fail(fit["slopeCyclesPerUnit"] == 3.0, "affine slope self-check failed")
    fail(
        affine_fit([(1, 1.0), (2, 4.0), (3, 9.0)])["classification"]
        == "unexplained",
        "unexplained self-check failed",
    )
    fail(
        family_classification(["exact", "interval"]) == "unexplained",
        "mixed-family self-check failed",
    )


def main() -> None:
    analysis_self_check()
    manifest, selected, expected_samples = manifest_contract()
    session = read_json("session-metadata.json")
    fail(session["gitCommit"] == SOURCE_COMMIT and session["gitDirty"] is False, "session source provenance mismatch")
    fail(session["selection"] == "all" and session["excludedOpenCell"] == "gpio21_edge", "session selection mismatch")
    fail(session["noncanonicalAttempts"]["count"] == 7, "session attempt count mismatch")
    fail(session["toolchain"]["espIdf"] == "v6.1", "session IDF mismatch")
    fail(session["toolchain"]["gcc"].startswith("xtensa-esp-elf-gcc 15.2.0"), "session compiler mismatch")

    archive = archive_hashes()
    archive_reference = read_json("archive-reference.json")
    fail(archive_reference["archiveSha256SumsSha256"] == sha256((ROOT / "archive-SHA256SUMS").read_bytes()), "archive checksum receipt mismatch")
    for artifact in archive_reference["largeArtifacts"]:
        fail(archive[artifact["path"]] == artifact["sha256"], f"archive artifact mismatch: {artifact['path']}")
    for prefix in archive_reference["noncanonical"]["pathPrefixes"]:
        fail(any(path.startswith(prefix) for path in archive), f"missing noncanonical prefix {prefix}")
    for artifact in archive_reference["noncanonical"]["earlierNormalCorroboration"]:
        fail(archive[artifact["path"]] == artifact["sha256"], f"normal corroboration hash mismatch: {artifact['path']}")
    fail(archive["session-metadata.json"] == sha256((ROOT / "session-metadata.json").read_bytes()), "archived session metadata hash mismatch")
    for variant, prefix in (("normal", "normal"), ("xip-psram", "xip")):
        preflight = read_json(f"preflight/{variant}.json")
        build_prefix = f"builds/{variant}"
        fail(archive[f"{build_prefix}/esp32s3_tier_b_calibration.elf"] == preflight["elfSha256"], f"{variant} archived ELF mismatch")
        fail(archive[f"{build_prefix}/sdkconfig"] == preflight["sdkconfigSha256"], f"{variant} archived sdkconfig mismatch")
        fail(session["builds"][f"{prefix}ElfSha256"] == preflight["elfSha256"], f"{variant} session ELF mismatch")
        fail(session["builds"][f"{prefix}SdkconfigSha256"] == preflight["sdkconfigSha256"], f"{variant} session sdkconfig mismatch")
        fail(session["builds"][f"{prefix}PreflightSha256"] == sha256((ROOT / f"preflight/{variant}.json").read_bytes()), f"{variant} session preflight mismatch")

    capture_summaries: list[dict[str, Any]] = []
    capture_samples: dict[tuple[str, int], dict[str, list[dict[str, Any]]]] = {}
    for capture in CAPTURES:
        summary, samples = validate_capture(*capture, selected, expected_samples, session)
        capture_summaries.append(summary)
        capture_samples[(capture[0], capture[1])] = samples
    identities = [summary["bootIdentity"] for summary in capture_summaries]
    fail(len(set(identities)) == 4, "canonical boot identities are not distinct")

    variants: dict[str, Any] = {}
    for variant in ("normal", "xip-psram"):
        pooled: dict[str, Any] = {}
        for cell in selected[variant]:
            values = [
                sample["cycles"]
                for boot in (1, 2)
                for sample in capture_samples[(variant, boot)][cell]
            ]
            pooled[cell] = {**stats(values), "classification": fixed_classification(values)}

        fits: dict[str, Any] = {}
        for label, prefix in (("writeback-clean-lines", "writeback_clean_"), ("writeback-dirty-lines", "writeback_dirty_")):
            points: list[tuple[int, float]] = []
            for boot in (1, 2):
                for lines in (1, 2, 4, 8, 16):
                    cell = f"{prefix}{lines}_lines"
                    values = [sample["cycles"] for sample in capture_samples[(variant, boot)][cell]]
                    points.append((lines, float(median(values))))
            fits[label] = {"unit": "cache-lines", **affine_fit(points)}

        for cell in SWEEPS:
            points = [
                (sample["bytes"], float(sample["cycles"]))
                for boot in (1, 2)
                for sample in capture_samples[(variant, boot)][cell]
            ]
            fit = {"unit": "bytes", **affine_fit(points)}
            fits[cell] = fit
            pooled[cell]["classification"] = fit["classification"]

        variants[variant] = {
            "boots": 2,
            "cellsPerBoot": len(selected[variant]),
            "samplesPerBoot": sum(expected_samples[cell] for cell in selected[variant]),
            "pooledPerCell": pooled,
            "affineFits": fits,
        }

    shared_cells = sorted(set(selected["normal"]) & set(selected["xip-psram"]))
    fail(len(shared_cells) == 24, "cross-variant pool does not contain 24 shared cells")
    cross_pooled: dict[str, Any] = {}
    for cell in shared_cells:
        values = [
            sample["cycles"]
            for variant in ("normal", "xip-psram")
            for boot in (1, 2)
            for sample in capture_samples[(variant, boot)][cell]
        ]
        cross_pooled[cell] = {
            **stats(values),
            "classification": fixed_classification(values),
        }

    cross_fits: dict[str, Any] = {}
    for label, prefix in (
        ("writeback-clean-lines", "writeback_clean_"),
        ("writeback-dirty-lines", "writeback_dirty_"),
    ):
        points: list[tuple[int, float]] = []
        for variant in ("normal", "xip-psram"):
            for boot in (1, 2):
                for lines in (1, 2, 4, 8, 16):
                    cell = f"{prefix}{lines}_lines"
                    values = [
                        sample["cycles"]
                        for sample in capture_samples[(variant, boot)][cell]
                    ]
                    points.append((lines, float(median(values))))
        cross_fits[label] = {"unit": "cache-lines", **affine_fit(points)}

    for cell in SWEEPS:
        points = [
            (sample["bytes"], float(sample["cycles"]))
            for variant in ("normal", "xip-psram")
            for boot in (1, 2)
            for sample in capture_samples[(variant, boot)][cell]
        ]
        fit = {"unit": "bytes", **affine_fit(points)}
        cross_fits[cell] = fit
        cross_pooled[cell]["classification"] = fit["classification"]

    cells_by_family: dict[str, list[str]] = {}
    manifest_cells = {cell["id"]: cell for cell in manifest["cells"]}
    for cell in shared_cells:
        family = manifest_cells[cell]["family"]
        cells_by_family.setdefault(family, []).append(cell)
    cross_families = {
        family: {
            "cells": cells,
            "classification": family_classification(
                [cross_pooled[cell]["classification"] for cell in cells]
            ),
            "cellClassifications": {
                cell: cross_pooled[cell]["classification"] for cell in cells
            },
        }
        for family, cells in sorted(cells_by_family.items())
    }

    identifiability = {
        "dirty-writeback": {
            "observedTotalClassification": cross_fits["writeback-dirty-lines"][
                "classification"
            ],
            "componentClassification": "unexplained",
            "separableCpuCacheDeviceCosts": False,
            "reason": (
                "The single cache-line-count axis identifies an end-to-end intercept and "
                "aggregate per-line slope only; CPU, cache-controller, and PSRAM transaction "
                "coefficients are rank-deficient."
            ),
            "requiredProbe": (
                "Cross dirty-line count with a controlled PSRAM service-rate change and add "
                "a no-op cache-msync control that isolates fixed CPU and API overhead."
            ),
            "productAdoptable": False,
        },
        "spi2-transfer": {
            "observedTotalClassification": cross_fits["spi2_transfer_sweep"][
                "classification"
            ],
            "componentClassification": "unexplained",
            "separableCpuCacheDeviceCosts": False,
            "reason": (
                "Payload size varies while the CPU driver path, DMA path, and 40 MHz SPI2 "
                "device transfer remain coupled in one blocking interval."
            ),
            "requiredProbe": (
                "Repeat the payload sweep at two verified SPI clocks and pair it with a "
                "submission-only control that reports DMA completion separately."
            ),
            "productAdoptable": False,
        },
        "cache-msync-writeback": {
            "observedTotalClassification": cross_fits[
                "cache_msync_writeback_sweep"
            ]["classification"],
            "componentClassification": "unexplained",
            "separableCpuCacheDeviceCosts": False,
            "reason": (
                "Byte count changes cache scan work and PSRAM writeback traffic together, "
                "so the fitted total does not identify either component independently."
            ),
            "requiredProbe": (
                "Measure matched clean and dirty residency at the same byte counts while "
                "independently varying the number of dirty lines and PSRAM service rate."
            ),
            "productAdoptable": False,
        },
    }
    fail(
        all(
            item["observedTotalClassification"] in CLASSIFICATIONS
            and item["componentClassification"] in CLASSIFICATIONS
            and item["separableCpuCacheDeviceCosts"] is False
            and item["productAdoptable"] is False
            for item in identifiability.values()
        ),
        "identifiability disposition is incomplete",
    )
    fail(
        set(cross_pooled) == set(shared_cells),
        "cross-variant cell classification is incomplete",
    )
    fail(
        set(cross_fits)
        == {
            "writeback-clean-lines",
            "writeback-dirty-lines",
            *SWEEPS,
        },
        "cross-variant family classification is incomplete",
    )

    output = {
        "schemaVersion": 2,
        "suite": "tier-b",
        "disposition": "candidate-evidence-only",
        "sourceCommit": SOURCE_COMMIT,
        "manifestSha256": sha256((ROOT / "probe-cells.json").read_bytes()),
        "archiveSha256SumsSha256": sha256((ROOT / "archive-SHA256SUMS").read_bytes()),
        "statistics": {
            "median": "arithmetic midpoint of the two middle sorted values for even sample counts",
            "p90": "nearest rank ceil(0.90 * n) on ascending values",
        },
        "captures": capture_summaries,
        "variants": variants,
        "crossVariant": {
            "variants": ["normal", "xip-psram"],
            "sharedCells": len(shared_cells),
            "pooledPerCell": cross_pooled,
            "affineFits": cross_fits,
            "families": cross_families,
            "identifiability": identifiability,
        },
        "open": ["gpio21_edge"],
        "adoptedMeasuredModeCosts": [],
    }
    print(json.dumps(output, indent=2, sort_keys=True) + "\n", end="")


if __name__ == "__main__":
    main()
