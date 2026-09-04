#!/usr/bin/env python3
"""Compare TinyDraw frame counters with esp32sim frame ledgers."""

import argparse
import json
import math
import re
import sys
from pathlib import Path


PREFIXES = ("FRAME_CORRELATION_V1 ", "TINYDRAW_TRACE_V1 ", "TINYDRAW_FRAME_V1 ", "ESP32SIM_FRAME_V1 ")
SHA256 = re.compile(r"^[0-9a-f]{64}$")
COMMIT = re.compile(r"^[0-9a-f]{40}$")
BOARD = "waveshare-esp32-s3-touch-amoled-1.8-v2"
METADATA = {
    "record", "version", "source", "run_id", "manifest_sha256", "source_commit",
    "workload_sha256", "common_config_sha256", "config_receipt_sha256", "board",
    "idf", "psram_hz", "core1_touch_enabled",
}
FRAME = {
    "record", "run_id", "seq", "kind", "event_seq", "total_cycles",
    "non_psram_cycles", "psram_cycles", "unknown_components", "ibus_accesses",
    "ibus_misses", "dbus_accesses", "dbus_flash_misses", "dbus_psram_misses",
}
COUNTERS = (
    "ibus_accesses", "ibus_misses", "dbus_accesses", "dbus_flash_misses",
    "dbus_psram_misses",
)
EXACT_IDENTITY = (
    "manifest_sha256", "source_commit", "workload_sha256", "common_config_sha256",
    "config_receipt_sha256", "board", "idf", "psram_hz", "core1_touch_enabled",
)
CANDIDATE_IDENTITY = (
    "source_commit", "workload_sha256", "common_config_sha256", "board", "idf",
)


class Refusal(ValueError):
    pass


def require_keys(value, keys, label):
    missing = keys - value.keys()
    extra = value.keys() - keys
    if missing:
        raise Refusal(f"{label} missing keys: {', '.join(sorted(missing))}")
    if extra:
        raise Refusal(f"{label} unexpected keys: {', '.join(sorted(extra))}")


def require_int(value, label, minimum=0):
    if isinstance(value, bool) or not isinstance(value, int) or value < minimum:
        raise Refusal(f"{label} must be an integer >= {minimum}")
    return value


def require_sha(value, label):
    if not isinstance(value, str) or SHA256.fullmatch(value) is None:
        raise Refusal(f"{label} must be a lowercase SHA-256")


def require_equal(left, right, label):
    if left != right:
        raise Refusal(f"{label} mismatch: {left!r} != {right!r}")


def payload(line):
    line = line.strip()
    if line.startswith("{"):
        return line
    for prefix in PREFIXES:
        offset = line.find(prefix)
        if offset >= 0:
            return line[offset + len(prefix):].strip()
    return None


def parse_record(text, label):
    try:
        value = json.loads(text)
    except (json.JSONDecodeError, RecursionError) as error:
        raise Refusal(f"{label} has invalid JSON: {error}") from error
    if not isinstance(value, dict):
        raise Refusal(f"{label} must be a JSON object")
    return value


def validate_metadata(record, label):
    require_keys(record, METADATA, label)
    require_equal(record["record"], "metadata", f"{label}.record")
    require_equal(require_int(record["version"], f"{label}.version", 1), 1, f"{label}.version")
    if not isinstance(record["source"], str) or record["source"] not in {"hardware", "emulator"}:
        raise Refusal(f"{label}.source must be hardware or emulator")
    if not isinstance(record["run_id"], str) or not record["run_id"]:
        raise Refusal(f"{label}.run_id must be a non-empty string")
    for key in (
        "manifest_sha256", "workload_sha256", "common_config_sha256",
        "config_receipt_sha256",
    ):
        require_sha(record[key], f"{label}.{key}")
    if not isinstance(record["source_commit"], str) or COMMIT.fullmatch(record["source_commit"]) is None:
        raise Refusal(f"{label}.source_commit must be a full lowercase commit ID")
    require_equal(record["board"], BOARD, f"{label}.board")
    require_equal(record["idf"], "v6.1", f"{label}.idf")
    psram_hz = require_int(record["psram_hz"], f"{label}.psram_hz", 1)
    if psram_hz not in {40_000_000, 80_000_000}:
        raise Refusal(f"{label}.psram_hz must be 40000000 or 80000000")
    if not isinstance(record["core1_touch_enabled"], bool):
        raise Refusal(f"{label}.core1_touch_enabled must be boolean")


def validate_frame(record, label, metadata, expected_seq):
    require_keys(record, FRAME, label)
    require_equal(record["record"], "frame", f"{label}.record")
    require_equal(record["run_id"], metadata["run_id"], f"{label}.run_id")
    require_equal(require_int(record["seq"], f"{label}.seq"), expected_seq, f"{label}.seq")
    if not isinstance(record["kind"], str) or not record["kind"]:
        raise Refusal(f"{label}.kind must be a non-empty string")
    require_int(record["event_seq"], f"{label}.event_seq")
    require_int(record["total_cycles"], f"{label}.total_cycles", 1)
    for key in COUNTERS:
        require_int(record[key], f"{label}.{key}")
    unknown = record["unknown_components"]
    if not isinstance(unknown, list) or any(not isinstance(item, str) or not item for item in unknown):
        raise Refusal(f"{label}.unknown_components must be an array of names")
    if len(unknown) != len(set(unknown)):
        raise Refusal(f"{label}.unknown_components contains duplicates")
    non_psram = record["non_psram_cycles"]
    psram = record["psram_cycles"]
    if (non_psram is None) != (psram is None):
        raise Refusal(f"{label} must supply both partitions or neither")
    if non_psram is None:
        if "psram" not in unknown:
            raise Refusal(f"{label} missing partition must name psram as unknown")
    else:
        require_int(non_psram, f"{label}.non_psram_cycles")
        require_int(psram, f"{label}.psram_cycles")
        require_equal(non_psram + psram, record["total_cycles"], f"{label} cycle sum")


def load(path):
    try:
        lines = path.read_text().splitlines()
    except (OSError, UnicodeError) as error:
        raise Refusal(f"cannot read {path}: {error}") from error
    metadata = None
    frames = []
    complete = False
    for number, line in enumerate(lines, 1):
        text = payload(line)
        if text is None:
            continue
        label = f"{path}:{number}"
        record = parse_record(text, label)
        if complete:
            raise Refusal(f"{label} follows run-complete")
        if record.get("record") == "metadata":
            if metadata is not None or frames:
                raise Refusal(f"{label} metadata is not first")
            validate_metadata(record, label)
            metadata = record
        elif record.get("record") == "frame":
            if metadata is None:
                raise Refusal(f"{label} frame precedes metadata")
            validate_frame(record, label, metadata, len(frames))
            frames.append(record)
        elif record.get("record") == "run-complete":
            require_keys(record, {"record", "run_id", "frames"}, label)
            if metadata is None:
                raise Refusal(f"{label} run-complete precedes metadata")
            require_equal(record["run_id"], metadata["run_id"], f"{label}.run_id")
            require_equal(require_int(record["frames"], f"{label}.frames"), len(frames),
                          f"{label}.frames")
            complete = True
        else:
            raise Refusal(f"{label} has unsupported record {record.get('record')!r}")
    if metadata is None or not frames or not complete:
        raise Refusal(f"{path} is missing metadata, frames, or run-complete")
    return {"metadata": metadata, "frames": frames}


def identity(metadata, keys):
    return {key: metadata[key] for key in keys}


def align(left, right):
    require_equal(len(left["frames"]), len(right["frames"]), "frame count")
    for first, second in zip(left["frames"], right["frames"], strict=True):
        require_equal((first["seq"], first["kind"], first["event_seq"]),
                      (second["seq"], second["kind"], second["event_seq"]), "frame key")
        require_equal(tuple(first[key] for key in COUNTERS), tuple(second[key] for key in COUNTERS),
                      f"frame {first['seq']} cache counters")


def summary(values):
    values = sorted(values)
    return {
        "classification": "distribution", "samples": len(values), "min": values[0],
        "p50": values[math.ceil(0.5 * len(values)) - 1],
        "p90": values[math.ceil(0.9 * len(values)) - 1], "max": values[-1],
    }


def compare(hardware, emulator):
    require_equal(hardware["metadata"]["source"], "hardware", "hardware source")
    require_equal(emulator["metadata"]["source"], "emulator", "emulator source")
    require_equal(identity(hardware["metadata"], EXACT_IDENTITY),
                  identity(emulator["metadata"], EXACT_IDENTITY), "identity")
    require_equal(hardware["metadata"]["psram_hz"], 80_000_000, "product PSRAM frequency")
    align(hardware, emulator)
    rows, hardware_psram, emulator_psram = [], [], []
    passed = True
    for actual, simulated in zip(hardware["frames"], emulator["frames"], strict=True):
        if actual["unknown_components"] or simulated["unknown_components"]:
            raise Refusal(f"frame {actual['seq']} has unknown components")
        if actual["non_psram_cycles"] is None or simulated["non_psram_cycles"] is None:
            raise Refusal(f"frame {actual['seq']} lacks an exact partition")
        require_int(actual["non_psram_cycles"], "hardware non_psram_cycles", 1)
        difference = abs(simulated["non_psram_cycles"] - actual["non_psram_cycles"])
        within = difference * 100 <= actual["non_psram_cycles"]
        passed &= within
        rows.append({"seq": actual["seq"],
                     "errorPercent": round(difference * 100 / actual["non_psram_cycles"], 9),
                     "withinOnePercent": within})
        hardware_psram.append(actual["psram_cycles"])
        emulator_psram.append(simulated["psram_cycles"])
    return {
        "identity": identity(hardware["metadata"], EXACT_IDENTITY),
        "nonPsram": {"targetPercent": 1, "targetMet": passed, "frames": rows},
        "psram": {"classification": "distribution", "scalarErrorPercent": None,
                  "hardware": summary(hardware_psram), "emulator": summary(emulator_psram)},
    }, passed


def psram_candidate(first, second):
    require_equal(first["metadata"]["source"], "hardware", "first source")
    require_equal(second["metadata"]["source"], "hardware", "second source")
    runs = {first["metadata"]["psram_hz"]: first, second["metadata"]["psram_hz"]: second}
    if set(runs) != {40_000_000, 80_000_000}:
        raise Refusal("candidate requires one 40 MHz and one 80 MHz run")
    slow, fast = runs[40_000_000], runs[80_000_000]
    require_equal(identity(slow["metadata"], CANDIDATE_IDENTITY),
                  identity(fast["metadata"], CANDIDATE_IDENTITY), "candidate identity")
    if slow["metadata"]["core1_touch_enabled"] or fast["metadata"]["core1_touch_enabled"]:
        raise Refusal("candidate requires core-1 touch to be stopped")
    for key in ("manifest_sha256", "config_receipt_sha256"):
        if slow["metadata"][key] == fast["metadata"][key]:
            raise Refusal(f"candidate requires distinct {key}")
    align(slow, fast)
    deltas = []
    for slow_frame, fast_frame in zip(slow["frames"], fast["frames"], strict=True):
        for frame in (slow_frame, fast_frame):
            if frame["unknown_components"] != ["psram"]:
                raise Refusal(f"frame {frame['seq']} must name only psram as unknown")
            if frame["non_psram_cycles"] is not None or frame["psram_cycles"] is not None:
                raise Refusal(f"frame {frame['seq']} must not claim an exact partition")
        deltas.append(slow_frame["total_cycles"] - fast_frame["total_cycles"])
    return {
        "disposition": "candidate-evidence-only", "classification": "distribution",
        "identity": identity(slow["metadata"], CANDIDATE_IDENTITY),
        "receipts": {
            "40MHz": {"manifest": slow["metadata"]["manifest_sha256"],
                      "configuration": slow["metadata"]["config_receipt_sha256"]},
            "80MHz": {"manifest": fast["metadata"]["manifest_sha256"],
                      "configuration": fast["metadata"]["config_receipt_sha256"]},
        },
        "cycles40MHz": summary([frame["total_cycles"] for frame in slow["frames"]]),
        "cycles80MHz": summary([frame["total_cycles"] for frame in fast["frames"]]),
        "slowMinusFast": summary(deltas), "nonPsramPartition": None,
        "onePercentClaim": "refused",
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest="command", required=True)
    exact = commands.add_parser("compare")
    exact.add_argument("hardware", type=Path)
    exact.add_argument("emulator", type=Path)
    candidate = commands.add_parser("psram-candidate")
    candidate.add_argument("first", type=Path)
    candidate.add_argument("second", type=Path)
    args = parser.parse_args()
    try:
        if args.command == "compare":
            report, passed = compare(load(args.hardware), load(args.emulator))
            print(json.dumps(report, indent=2, sort_keys=True))
            return 0 if passed else 1
        print(json.dumps(psram_candidate(load(args.first), load(args.second)), indent=2, sort_keys=True))
        return 0
    except Refusal as error:
        print(json.dumps({"ok": False, "refusal": str(error)}, sort_keys=True))
        return 2


if __name__ == "__main__":
    sys.exit(main())
