#!/usr/bin/env python3
"""Recompute the exception timing derivation stop from pinned inputs."""

from __future__ import annotations

import hashlib
import json
import os
from pathlib import Path
import subprocess


ROOT = Path(__file__).resolve().parents[4]
ELF_SHA256 = "7f598fd3580cf52078fb6aa04a5f6fe5179b0de9d89bb6468fdb06ed5e40e424"
TARGETS_SHA256 = "d4a4d3547598ede01573b94b5da3fdd1258d3f4e8161778acb4fd0423ac8a654"
TARGETS_PATH = ROOT / "docs/evidence/timing/idf61-rebaseline-3db3985/toolchain-delta.json"
MARKER = "EXCEPTION_DERIVATION_ATTEMPTS="
EXPECTED_ATTEMPTS = {
    "level1_entry": ("0x40374340", "0x403791a4", 17, 15, "l32r_interval"),
    "level3_entry": ("0x403741c0", "0x403792f0", 12, 12, "l32r_interval"),
    "level1_resume": ("0x40379214", "0x403791c0", 5, 3, "l32r_interval"),
    "level3_resume": ("0x40379360", "0x4037930c", 5, 3, "l32r_interval"),
    "window_overflow8": (
        "0x40374080",
        "0x4037409b",
        9,
        9,
        "rfwo_zero_placeholder",
    ),
    "window_underflow8": (
        "0x403740c0",
        "0x403740db",
        9,
        9,
        "rfwu_zero_placeholder",
    ),
}


def sha256(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def load_attempts() -> list[dict[str, object]]:
    build = os.environ.get("TINYDRAW_VECTOR_V2_BUILD")
    if build is None:
        raise SystemExit("TINYDRAW_VECTOR_V2_BUILD must name the TinyDraw product build")
    elf = Path(build) / "tinydraw_esp32.elf"
    digest = sha256(elf)
    if digest != ELF_SHA256:
        raise SystemExit(f"TinyDraw ELF sha256 {digest} does not match {ELF_SHA256}")

    environment = os.environ.copy()
    environment["CARGO_TERM_COLOR"] = "never"
    result = subprocess.run(
        [
            "cargo",
            "test",
            "-p",
            "esp32s3",
            "--test",
            "exception_derivation",
            "real_idf61_exception_paths_expose_the_incomplete_known_ledgers",
            "--",
            "--ignored",
            "--nocapture",
        ],
        cwd=ROOT,
        env=environment,
        check=True,
        text=True,
        capture_output=True,
    )
    lines = [line for line in result.stdout.splitlines() if line.startswith(MARKER)]
    if len(lines) != 1:
        raise SystemExit("exception derivation test emitted no unique attempt marker")
    attempts = json.loads(lines[0][len(MARKER) :])
    observed = {
        attempt["name"]: (
            attempt["start_pc"],
            attempt["stop_pc"],
            attempt["known_cycles"],
            attempt["ledger_entries"],
            attempt["stop"],
        )
        for attempt in attempts
    }
    if observed != EXPECTED_ATTEMPTS:
        raise SystemExit(f"real IDF exception path stops changed: {observed!r}")
    return attempts


def main() -> None:
    targets_digest = sha256(TARGETS_PATH)
    if targets_digest != TARGETS_SHA256:
        raise SystemExit(
            f"toolchain-delta sha256 {targets_digest} does not match {TARGETS_SHA256}"
        )
    targets = json.loads(TARGETS_PATH.read_text())
    interrupts = targets["idfOwned"]["interruptDispatcherCycles"]
    window_target = targets["siliconArchitectural"][
        "windowOverflowUnderflowPairCyclesPastDepth6"
    ]
    attempts = load_attempts()
    by_name = {attempt["name"]: attempt for attempt in attempts}
    blocker = (
        "the real IDF entry and resume ledgers stop at l32r, whose adopted cost "
        "is interval 1..2 rather than exact"
    )
    output = {
        "schema_version": 1,
        "inputs": {
            "tiny_draw_elf_sha256": ELF_SHA256,
            "targets": str(TARGETS_PATH.relative_to(ROOT)),
            "targets_sha256": targets_digest,
        },
        "attempts": attempts,
        "equations": {
            "entry_delay_E": {
                "source": "level1_entry",
                "receipt_target_cycles": interrupts["level1"]["entryV61"],
                "known_ledger_prefix_cycles": by_name["level1_entry"]["known_cycles"],
                "value_cycles": None,
                "stop": by_name["level1_entry"]["stop"],
            },
            "return_redirect_R": {
                "source": "level1_resume",
                "receipt_target_cycles": interrupts["level1"]["resumeV61"],
                "known_ledger_prefix_cycles": by_name["level1_resume"]["known_cycles"],
                "value_cycles": None,
                "stop": by_name["level1_resume"]["stop"],
            },
        },
        "validations": {
            "level3_entry": {
                "receipt_target_cycles": interrupts["level3"]["entryV61"],
                "known_ledger_prefix_cycles": by_name["level3_entry"]["known_cycles"],
                "residual_cycles": None,
                "status": "not_evaluated",
            },
            "level3_resume": {
                "receipt_target_cycles": interrupts["level3"]["resumeV61"],
                "known_ledger_prefix_cycles": by_name["level3_resume"]["known_cycles"],
                "residual_cycles": None,
                "status": "not_evaluated",
            },
            "window_pair": {
                "receipt_target_cycles": window_target,
                "known_handler_ledger_cycles": (
                    by_name["window_overflow8"]["known_cycles"]
                    + by_name["window_underflow8"]["known_cycles"]
                ),
                "residual_cycles": None,
                "status": "not_evaluated",
            },
        },
        "adoption": {
            "adopted": False,
            "tier_candidate": "exact",
            "reason": blocker,
            "hardware_confirmation": "complete hardware queue item H1",
        },
    }
    print(json.dumps(output, indent=2, sort_keys=True))


if __name__ == "__main__":
    main()
