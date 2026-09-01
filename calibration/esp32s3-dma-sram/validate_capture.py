#!/usr/bin/env python3
"""Validate and summarize one DMA-on-SRAM CAL_RECORD capture."""

from __future__ import annotations

import argparse
import json
import math
import sys
from pathlib import Path
from typing import Any


IMAGE_DIR = Path(__file__).resolve().parent
TOOLS_DIR = IMAGE_DIR.parent / "tools"
sys.path.insert(0, str(TOOLS_DIR))

from ndjson import (  # noqa: E402
    CAL_PREFIX,
    ManifestContract,
    ValidationError,
    validate_calibration_lines,
)


IDLE = "copy_psram_to_sram_idle"
PSRAM_ACTIVE = "copy_psram_to_sram_dma_active"
SRAM_ACTIVE = "copy_sram_to_sram_dma_active"
SUBMIT_COMPLETE = "spi2_32k_submit_to_complete"
SUBMIT_ONLY = "spi2_32k_submit_only"
CELL_ORDER = [IDLE, PSRAM_ACTIVE, SRAM_ACTIVE, SUBMIT_COMPLETE, SUBMIT_ONLY]


def _median(ordered: list[int]) -> int | float:
    middle = len(ordered) // 2
    if len(ordered) % 2:
        return ordered[middle]
    total = ordered[middle - 1] + ordered[middle]
    return total // 2 if total % 2 == 0 else total / 2


def distribution(values: list[int]) -> dict[str, int | float]:
    if not values:
        raise ValidationError("cannot summarize an empty sample set")
    ordered = sorted(values)
    return {
        "min": ordered[0],
        "median": _median(ordered),
        "p90": ordered[math.ceil(len(ordered) * 0.9) - 1],
        "max": ordered[-1],
    }


def _cal_records(lines: list[str]) -> list[dict[str, Any]]:
    records = []
    for line_number, line in enumerate(lines, 1):
        offset = line.find(CAL_PREFIX)
        if offset < 0:
            continue
        try:
            value = json.loads(line[offset + len(CAL_PREFIX) :].strip())
        except json.JSONDecodeError as error:
            raise ValidationError(
                f"line {line_number} has malformed CAL_RECORD JSON: {error.msg}"
            ) from error
        if not isinstance(value, dict):
            raise ValidationError(f"line {line_number} CAL_RECORD must be an object")
        records.append(value)
    return records


def _samples(record: dict[str, Any], cell: str) -> list[int]:
    values = record.get("ccount_samples")
    if (
        not isinstance(values, list)
        or len(values) != 100
        or any(
            isinstance(value, bool) or not isinstance(value, int) or value <= 0
            for value in values
        )
    ):
        raise ValidationError(f"{cell} must contain 100 positive integer samples")
    return values


def analyze(lines: list[str], manifest: Path) -> dict[str, Any]:
    contract = ManifestContract.load(manifest)
    tally = validate_calibration_lines(lines, contract, "normal", "all", False)
    records = _cal_records(lines)
    metrics = {
        record.get("name"): record for record in records if record.get("type") == "metric"
    }
    if list(metrics) != CELL_ORDER or len(metrics) != len(CELL_ORDER):
        raise ValidationError("capture does not contain the five metrics in contract order")
    samples = {cell: _samples(metrics[cell], cell) for cell in CELL_ORDER}

    for cell in (PSRAM_ACTIVE, SRAM_ACTIVE):
        flags = metrics[cell].get("dma_still_in_flight_samples")
        if (
            not isinstance(flags, list)
            or len(flags) != 100
            or any(flag is not True for flag in flags)
        ):
            raise ValidationError(f"{cell} contains a sample without DMA still in flight")
    expected_shape = {
        IDLE: (8192, 4, None),
        PSRAM_ACTIVE: (8192, 4, IDLE),
        SRAM_ACTIVE: (8192, 4, IDLE),
        SUBMIT_COMPLETE: (1, 32768, None),
        SUBMIT_ONLY: (1, 32768, None),
    }
    for cell, (operations, byte_count, baseline) in expected_shape.items():
        record = metrics[cell]
        if (
            record.get("operations_per_trial") != operations
            or record.get("bytes_per_operation") != byte_count
            or record.get("baseline") != baseline
        ):
            raise ValidationError(f"{cell} metric shape does not match its contract")

    psram_delta = [
        active - idle
        for active, idle in zip(samples[PSRAM_ACTIVE], samples[IDLE], strict=True)
    ]
    sram_delta = [
        active - idle
        for active, idle in zip(samples[SRAM_ACTIVE], samples[IDLE], strict=True)
    ]
    return {
        "ok": True,
        "samplesPerCell": 100,
        "cells": {cell: distribution(samples[cell]) for cell in CELL_ORDER},
        "deltas": {
            "copy_psram_to_sram_dma_active_minus_idle": distribution(psram_delta),
            "copy_sram_to_sram_dma_active_minus_idle": distribution(sram_delta),
        },
        "tally": tally.as_dict(),
    }


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("capture", type=Path)
    parser.add_argument("--manifest", type=Path, default=IMAGE_DIR / "probe-cells.json")
    args = parser.parse_args()
    try:
        lines = args.capture.read_text(encoding="utf-8").splitlines()
        result = analyze(lines, args.manifest)
    except (OSError, ValidationError) as error:
        print(json.dumps({"ok": False, "error": str(error)}, sort_keys=True))
        return 2
    print(json.dumps(result, indent=2, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
