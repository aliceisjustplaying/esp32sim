#!/usr/bin/env python3
"""Recompute the opcode-ladder receipt summary from both boot logs."""

import gzip
import json
import math
import statistics
from pathlib import Path


ROOT = Path(__file__).resolve().parent
PREFIX = "CAL_RECORD "
OPERATIONS = 256

# Cycles outside each verified ladder body, derived from the ELF body's entry,
# exit, and alignment instructions. The standard ladder overhead is 15 cycles.
OVERHEAD = {
    "beqz_n_not_taken": 19,
    "bnez_n_not_taken": 19,
    "j": 13,
    "jx": 13,
    "call0_ret": 15,
    "callx0_ret": 16,
    "call8_retw": 13,
    "callx8_retw": 15,
    "loop": 14,
    "loopnez": 14,
    "loopgtz": 14,
    "s32c1i": 11,
    "load_use_distance_1": 13,
    "load_use_distance_2": 13,
}


def records(path):
    result = {}
    with gzip.open(path, "rt", errors="replace") as stream:
        for line in stream:
            marker = line.find(PREFIX)
            if marker < 0:
                continue
            record = json.loads(line[marker + len(PREFIX) :])
            if record.get("type") == "metric":
                result[record["name"]] = record
    return result


def nearest_rank_p90(values):
    return sorted(values)[math.ceil(0.9 * len(values)) - 1]


def number(value):
    return int(value) if float(value).is_integer() else value


def classify(medians):
    low = math.floor(min(medians))
    high = math.ceil(max(medians))
    if medians[0] == medians[1] and float(medians[0]).is_integer():
        return {"tier": "exact", "value": int(medians[0])}
    if high - low <= 1:
        return {"tier": "interval", "range": [low, high]}
    return {"tier": "distribution", "medianRange": [number(min(medians)), number(max(medians))]}


def main():
    boots = [records(ROOT / f"boot-{boot}.log.gz") for boot in (1, 2)]
    if set(boots[0]) != set(boots[1]):
        raise SystemExit("boot cell sets differ")

    cells = {}
    for name in sorted(boots[0]):
        overhead = OVERHEAD.get(name, 15)
        per_boot = []
        medians = []
        for record in (boots[0][name], boots[1][name]):
            samples = record["ccount_samples"]
            operations = record["operations_per_trial"]
            if operations != OPERATIONS:
                raise SystemExit(f"{name}: unexpected operation count {operations}")
            stats = {
                "min": min(samples),
                "median": number(statistics.median(samples)),
                "p90": nearest_rank_p90(samples),
                "max": max(samples),
            }
            stats["adjustedCyclesPerOperation"] = {
                key: number((value - overhead) / operations) for key, value in stats.items()
            }
            per_boot.append(stats)
            medians.append((statistics.median(samples) - overhead) / operations)
        cells[name] = {
            "operationsPerTrial": OPERATIONS,
            "matchedWrapperOverheadCycles": overhead,
            "boots": per_boot,
            "classification": classify(medians),
        }

    branches = sorted(name for name in cells if name.endswith(("_taken", "_not_taken")))
    output = {
        "schemaVersion": 1,
        "cellCount": len(cells),
        "statistics": "raw CCOUNT min, median, nearest-rank p90, and max",
        "wrapperOverhead": {
            "standardCyclesPerTrial": 15,
            "derivation": "271-cycle 256-nop ladder minus 256 issued instructions",
            "specializedCyclesPerTrial": {name: OVERHEAD[name] for name in sorted(OVERHEAD)},
        },
        "cells": cells,
        "adoptedPerInstruction": {
            "conditionalBranches": {name: cells[name]["classification"] for name in branches},
            "j": cells["j"]["classification"],
            "jx": cells["jx"]["classification"],
            "loopSetup": {name: cells[name]["classification"] for name in ("loop", "loopgtz", "loopnez")},
            "simpleOneCycle": [
                name
                for name in (
                    "issue_nop_baseline", "mull", "mulsh", "muluh", "nsa", "nsau",
                    "sext", "memw", "extw", "rsr", "wsr", "xsr", "rsync", "movsp",
                    "min", "max", "minu", "maxu",
                )
            ],
            "quosQuou": 4,
            "remsRemu": 5,
            "l32r": cells["l32r"]["classification"],
            "s32c1i": cells["s32c1i"]["classification"],
            "isync": cells["isync"]["classification"],
            "loadUseAdditiveCycles": {"distance1": 1, "distance2": 0, "tier": "exact"},
        },
        "correlationTargets": {
            name: {
                "tier": "exact",
                "cyclesPer256Pairs": number(statistics.median(boots[0][name]["ccount_samples"]) - OVERHEAD[name]),
            }
            for name in ("call0_ret", "callx0_ret", "call8_retw", "callx8_retw")
        },
    }
    (ROOT / "summary.json").write_text(json.dumps(output, indent=2, sort_keys=True) + "\n")


if __name__ == "__main__":
    main()
