#!/usr/bin/env python3
"""Recompute the DMA-on-SRAM receipt summary from both boot logs."""

import gzip
import json
import math
import statistics
from pathlib import Path


ROOT = Path(__file__).resolve().parent
PREFIX = "CAL_RECORD "


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
        per_boot = []
        medians = []
        for record in (boots[0][name], boots[1][name]):
            samples = record["ccount_samples"]
            operations = record["operations_per_trial"]
            raw = {
                "min": min(samples),
                "median": number(statistics.median(samples)),
                "p90": nearest_rank_p90(samples),
                "max": max(samples),
            }
            raw["cyclesPerOperation"] = {
                key: number(value / operations) for key, value in raw.items()
            }
            per_boot.append(raw)
            medians.append(statistics.median(samples) / operations)
        cells[name] = {
            "operationsPerTrial": boots[0][name]["operations_per_trial"],
            "boots": per_boot,
            "classification": classify(medians),
        }

    paired = []
    for boot in boots:
        idle = boot["copy_psram_to_sram_idle"]["ccount_samples"]
        active = boot["copy_psram_to_sram_dma_active"]["ccount_samples"]
        paired.append(number(statistics.median([a - i for a, i in zip(active, idle)])))
    output = {
        "schemaVersion": 1,
        "cellCount": len(cells),
        "statistics": "raw CCOUNT min, median, nearest-rank p90, and max",
        "cells": cells,
        "adoptedAdditiveDelays": {
            "spi2DmaContentionOnPsramToSramCopy": {
                "tier": "exact",
                "cycles": 0,
                "pairedBootMedianRangeCyclesPer32768ByteCopy": [min(paired), max(paired)],
                "scope": "CPU-copy slowdown attributable to concurrent SPI2 DMA",
            }
        },
        "correlationTargets": {
            "spi2_32k_submit_to_complete": {"tier": "exact", "cycles": 401589},
            "spi2_32k_submit_only": {"tier": "exact", "cycles": 5755},
        },
    }
    (ROOT / "summary.json").write_text(json.dumps(output, indent=2, sort_keys=True) + "\n")


if __name__ == "__main__":
    main()
