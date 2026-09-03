#!/usr/bin/env python3
"""Validate and summarize the five fixed-horizon native READY runs."""

import json
import re
import statistics
import sys
from pathlib import Path


READY = "TINYDRAW_VECTOR_V2_READY"
STOP = re.compile(
    r"^\[emu\] stop: MaxInsns . core0 (\d+) \+ core1 (\d+) insns in "
    r"([0-9.]+)s wall = ([0-9.]+) Minsn/s; emulated ([0-9.]+)s "
    r"\((\d+) cycles\);",
    re.MULTILINE,
)
JIT = re.compile(r"; jit: (\d+) compiled, (\d+) KB code")
REAL = re.compile(r"^real ([0-9.]+)$", re.MULTILINE)


def parse_run(directory: Path, number: int) -> dict[str, int | float | bool]:
    console = (directory / f"run-{number}.console.txt").read_text()
    stderr = (directory / f"run-{number}.stderr.txt").read_text()
    stop = STOP.search(stderr)
    jit = JIT.search(stderr)
    real = REAL.search(stderr)
    if console.count(READY) != 1:
        raise SystemExit(f"run {number}: expected exactly one READY marker")
    if stop is None or jit is None or real is None:
        raise SystemExit(f"run {number}: incomplete stop, JIT, or time receipt")
    core0, core1 = int(stop[1]), int(stop[2])
    if int(jit[1]) == 0:
        raise SystemExit(f"run {number}: no native JIT blocks compiled")
    return {
        "run": number,
        "ready": True,
        "stopReason": "MaxInsns",
        "core0RetiredInstructions": core0,
        "core1RetiredInstructions": core1,
        "totalRetiredInstructions": core0 + core1,
        "emulatorWallSeconds": float(stop[3]),
        "reportedRetiredMips": float(stop[4]),
        "emulatedSeconds": float(stop[5]),
        "cycles": int(stop[6]),
        "processWallSeconds": float(real[1]),
        "jitBlocksCompiled": int(jit[1]),
        "jitCodeKiB": int(jit[2]),
    }


def main() -> None:
    directory = Path(sys.argv[1]) if len(sys.argv) > 1 else Path("raw")
    runs = [parse_run(directory, number) for number in range(1, 6)]
    summary = {
        "runs": runs,
        "medianProcessWallSeconds": statistics.median(
            run["processWallSeconds"] for run in runs
        ),
        "medianRetiredMips": statistics.median(
            run["reportedRetiredMips"] for run in runs
        ),
    }
    print(json.dumps(summary, indent=2))


if __name__ == "__main__":
    main()
