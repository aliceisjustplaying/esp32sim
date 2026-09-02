#!/usr/bin/env python3
"""Recompute the register-block receipt summary from both boot logs."""

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


def overhead(name):
    if "read" in name:
        return 1 if any(token in name for token in ("system_", "sensitive_", "extmem_", "assist_debug_")) else 2
    return 1


def classify(name, medians):
    if "rtc_" in name or "efuse_" in name:
        return {"tier": "distribution", "medianRange": [number(min(medians)), number(max(medians))]}
    low = math.floor(min(medians))
    high = math.ceil(max(medians))
    if medians[0] == medians[1] and float(medians[0]).is_integer():
        return {"tier": "exact", "value": int(medians[0])}
    if high - low <= 1:
        return {"tier": "interval", "range": [low, high]}
    return {"tier": "distribution", "medianRange": [number(min(medians)), number(max(medians))]}


def sample_range(boots, name, adjustment):
    values = [
        (sample - adjustment) / record["operations_per_trial"]
        for boot in boots
        for record in (boot[name],)
        for sample in record["ccount_samples"]
    ]
    return [number(min(values)), number(max(values))]


def main():
    boots = [records(ROOT / f"boot-{boot}.log.gz") for boot in (1, 2)]
    if set(boots[0]) != set(boots[1]):
        raise SystemExit("boot cell sets differ")
    cells = {}
    for name in sorted(boots[0]):
        adjustment = overhead(name)
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
            raw["adjustedCyclesPerOperation"] = {
                key: number((value - adjustment) / operations) for key, value in raw.items()
            }
            per_boot.append(raw)
            medians.append((statistics.median(samples) - adjustment) / operations)
        cells[name] = {
            "operationsPerTrial": boots[0][name]["operations_per_trial"],
            "matchedWrapperOverheadCycles": adjustment,
            "boots": per_boot,
            "classification": classify(name, medians),
        }

    output = {
        "schemaVersion": 1,
        "cellCount": len(cells),
        "statistics": "raw CCOUNT min, median, nearest-rank p90, and max",
        "cells": cells,
        "tierModel": {
            "reads": {
                "systemSensitiveExtmemAssistDebug": {"tier": "exact", "cycles": 9},
                "apbPeripheral": {"tier": "exact", "cycles": 15},
                "nrx": {"tier": "exact", "cycles": 18},
                "rtc": {"tier": "distribution", "range": sample_range(boots, "mmio_read_rtc_reset_state", 2)},
                "efuse": {"tier": "distribution", "range": sample_range(boots, "mmio_read_efuse_repeat_data3", 2)},
            },
            "writes": {
                "postedBufferDepth": {"tier": "exact", "writes": 8},
                "enqueueThroughDepth": {"tier": "exact", "totalCycles": "n + 1"},
                "steadyDrainCyclesPerWrite": {
                    "systemSensitiveExtmemAssistDebug": {"tier": "exact", "cycles": 4},
                    "apbPeripheral": {"tier": "exact", "cycles": 15},
                    "nrx": {"tier": "interval", "range": [17, 18]},
                    "rtc": {"tier": "distribution", "range": sample_range(boots, "mmio_write_rtc_clock_config", 1)},
                },
                "drainDerivation": "slope from the 16-write through 256-write run cells",
            },
            "blocks": {
                "fast": ["SYSTEM", "SENSITIVE", "EXTMEM", "ASSIST_DEBUG", "INTERRUPT_CORE"],
                "apb": ["IO_MUX", "SPI0", "SPI1", "SPI2", "I2C0", "I2C_MST", "SHA", "SYSTIMER", "APB_SARADC", "APB_CTRL", "TIMG0", "TIMG1", "GDMA", "UART0", "USB_SERIAL_JTAG", "FE2"],
                "nrx": ["NRX"],
                "rtc": ["RTC_CNTL"],
                "efuse": ["EFUSE"],
            },
        },
    }
    (ROOT / "summary.json").write_text(json.dumps(output, indent=2, sort_keys=True) + "\n")


if __name__ == "__main__":
    main()
