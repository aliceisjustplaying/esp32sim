#!/usr/bin/env python3
"""Recompute the paired TinyDraw frame candidate from committed captures."""

import gzip
import hashlib
import json
import re
import sys
from pathlib import Path


HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[3]
sys.path.insert(0, str(ROOT / "tools"))

import frame_correlation as correlation  # noqa: E402


def sha256(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def read_json(path):
    value = json.loads(path.read_text())
    if not isinstance(value, dict):
        raise ValueError(f"{path} must contain a JSON object")
    return value


def raw_capture(speed, boot):
    marker = "TINYDRAW_FRAME_TELEMETRY_V1 "
    with gzip.open(HERE / f"{speed}mhz-boot-{boot}.log.gz", "rt") as source:
        lines = source.readlines()
    starts = [index for index, line in enumerate(lines) if "TINYDRAW_TRACE_V1 " in line]
    ends = [index for index, line in enumerate(lines) if "TINYDRAW_DEMO_REPLAY_END" in line]
    if len(starts) != 1 or len(ends) != 1 or starts[0] >= ends[0]:
        raise ValueError(f"{speed} MHz boot {boot} lacks one accepted trace window")
    rows = [
        json.loads(line.split(marker, 1)[1])
        for line in lines[starts[0]:ends[0] + 1]
        if marker in line
    ]
    if [row.get("seq") for row in rows] != list(range(1, 22)):
        raise ValueError(f"{speed} MHz boot {boot} telemetry is not 21 contiguous frames")
    rom_line = next(line for line in lines if "ESP-ROM:" in line)
    panel_line = next(line for line in lines if "TINYDRAW_PANEL_HARD_RESET=" in line)
    field = lambda name: re.search(rf"(?:^| ){name}=([^ ]+)", panel_line).group(1)
    caveat = {
        "psramMHz": speed,
        "boot": boot,
        "stalePartialSerialBeforeReset": bool(rom_line.split("ESP-ROM:", 1)[0]),
        "panelAttempts": int(field("attempts")),
        "panelBusResets": int(field("bus_resets")),
        "panelFirstFailureStage": field("first_failure_stage"),
        "panelFirstFailure": field("first_failure"),
    }
    return rows, caveat


def sign(value):
    return (value > 0) - (value < 0)


def main():
    receipt = read_json(HERE / "receipt.json")
    if sha256(ROOT / receipt["tool"]["path"]) != receipt["tool"]["sha256"]:
        raise ValueError("frame correlation tool does not match the pinned receipt")

    captures = {(item["psramMHz"], item["boot"]): item for item in receipt["captures"]}
    for (speed, boot), capture in captures.items():
        normalized = HERE / f"{speed}mhz-boot-{boot}.normalized.ndjson"
        raw = HERE / f"{speed}mhz-boot-{boot}.log.gz"
        if sha256(normalized) != capture["normalizedSha256"]:
            raise ValueError(f"{normalized.name} hash mismatch")
        with gzip.open(raw, "rb") as source:
            if hashlib.sha256(source.read()).hexdigest() != capture["rawSha256"]:
                raise ValueError(f"{raw.name} raw hash mismatch")

    all_pairs = []
    boot_pairs = []
    raw_caveats = []
    runs = {}
    for boot in (1, 2):
        slow = correlation.load(HERE / f"40mhz-boot-{boot}.normalized.ndjson")
        fast = correlation.load(HERE / f"80mhz-boot-{boot}.normalized.ndjson")
        report = correlation.psram_candidate(slow, fast)
        if report != read_json(HERE / f"frame-pair-boot-{boot}.json"):
            raise ValueError(f"boot {boot} paired report is not reproducible")
        slow_telemetry, slow_caveat = raw_capture(40, boot)
        fast_telemetry, fast_caveat = raw_capture(80, boot)
        raw_caveats.extend((slow_caveat, fast_caveat))
        pairs = []
        for slow_frame, fast_frame, slow_time, fast_time in zip(
            slow["frames"], fast["frames"], slow_telemetry, fast_telemetry, strict=True
        ):
            frame_key = (slow_frame["seq"], slow_frame["kind"], slow_frame["event_seq"])
            if frame_key != (fast_frame["seq"], fast_frame["kind"], fast_frame["event_seq"]):
                raise ValueError(f"boot {boot} frame key mismatch")
            if slow_time["seq"] != slow_frame["seq"] or fast_time["seq"] != fast_frame["seq"]:
                raise ValueError(f"boot {boot} telemetry alignment mismatch")
            pairs.append({
                "boot": boot,
                "seq": slow_frame["seq"],
                "cycles": slow_frame["total_cycles"] - fast_frame["total_cycles"],
                "transferWaitUs": slow_time["transfer_wait_us"] - fast_time["transfer_wait_us"],
                **{key: slow_frame[key] - fast_frame[key] for key in correlation.COUNTERS},
            })
        all_pairs.extend(pairs)
        boot_pairs.append({
            "boot": boot,
            "totalCycleDelta": report["slowMinusFast"],
            "transferWaitDeltaUs": correlation.summary([row["transferWaitUs"] for row in pairs]),
            "negativeFrames": sum(row["cycles"] < 0 for row in pairs),
            "positiveFrames": sum(row["cycles"] > 0 for row in pairs),
            "transferWaitSignMatchesCycleDelta": sum(
                sign(row["cycles"]) == sign(row["transferWaitUs"]) for row in pairs
            ),
        })
        runs[40, boot], runs[80, boot] = slow, fast

    repeatability = {}
    for speed in (40, 80):
        differences = [
            second["total_cycles"] - first["total_cycles"]
            for first, second in zip(runs[speed, 1]["frames"], runs[speed, 2]["frames"], strict=True)
        ]
        repeatability[f"{speed}MHzBoot2MinusBoot1Cycles"] = correlation.summary(differences)

    caveats = receipt["captureCaveats"]
    observed_stale = [
        {"psramMHz": row["psramMHz"], "boot": row["boot"]}
        for row in raw_caveats if row["stalePartialSerialBeforeReset"]
    ]
    if observed_stale != caveats["stalePartialSerialBeforeReset"]:
        raise ValueError("stale serial caveat mismatch")
    recovered = next(row for row in raw_caveats if row["panelBusResets"])
    expected_recovery = caveats["recoveredPanelConfigurationFailure"]
    if {
        "psramMHz": recovered["psramMHz"], "boot": recovered["boot"],
        "attempts": recovered["panelAttempts"], "busResets": recovered["panelBusResets"],
        "stage": recovered["panelFirstFailureStage"], "error": recovered["panelFirstFailure"],
    } != expected_recovery:
        raise ValueError("panel recovery caveat mismatch")

    tail = [row for row in all_pairs if row["cycles"] > 200_000]
    tail_keys = {
        row["seq"] for row in tail
        if {item["boot"] for item in tail if item["seq"] == row["seq"]} == {1, 2}
    }
    repeated_tail = [row for row in tail if row["seq"] in tail_keys]
    isolated_tail = [row for row in tail if row["seq"] not in tail_keys]
    summary = {
        "schemaVersion": 1,
        "status": "candidate",
        "classification": "distribution",
        "framePairs": 42,
        "rawCaptureCaveats": raw_caveats,
        "bootPairs": boot_pairs,
        "repeatability": repeatability,
        "phaseCoupling": {
            "transferWaitSignMatchesCycleDelta": sum(
                sign(row["cycles"]) == sign(row["transferWaitUs"]) for row in all_pairs
            ),
            "shorterTransferWaitAt40MHz": sum(row["transferWaitUs"] < 0 for row in all_pairs),
            "longerTransferWaitAt40MHz": sum(row["transferWaitUs"] > 0 for row in all_pairs),
            "positiveTailThresholdCycles": 200_000,
            "repeatedPositiveTailFrameKeys": sorted(tail_keys),
            "repeatedPositiveTailTransferWaitDeltaUs": correlation.summary(
                [row["transferWaitUs"] for row in repeated_tail]
            ),
            "repeatedPositiveTailIbusAccessDelta": correlation.summary(
                [row["ibus_accesses"] for row in repeated_tail]
            ),
            "isolatedPositiveTailFrames": isolated_tail,
        },
        "counterCovariates": {
            "dbusPsramMissPairsEqual": sum(row["dbus_psram_misses"] == 0 for row in all_pairs),
            "dbusFlashMissPairsBothZero": sum(
                slow["dbus_flash_misses"] == fast["dbus_flash_misses"] == 0
                for speed in (40,)
                for boot in (1, 2)
                for slow, fast in zip(
                    runs[speed, boot]["frames"], runs[80, boot]["frames"], strict=True
                )
            ),
            **{
                f"{key}Delta": correlation.summary([row[key] for row in all_pairs])
                for key in correlation.COUNTERS
            },
        },
        "conclusion": {
            "tier": "distribution",
            "use": "paired total-cycle and shared cache-counter candidate",
            "distributionAgreement": "not established",
            "psramScalarPrice": None,
            "nonPsramPartition": None,
            "onePercentClaim": "refused",
            "reason": "PSRAM remains unknown and transfer-wait phase changes dominate paired signs",
        },
    }
    (HERE / "summary.json").write_text(json.dumps(summary, indent=2, sort_keys=True) + "\n")


if __name__ == "__main__":
    main()
