#!/usr/bin/env python3
"""Verify the pinned TinyDraw gate-harness ELF and build configuration."""

from __future__ import annotations

import argparse
import hashlib
import json
from pathlib import Path


ELF_SHA256 = "1d67c35762fe58b72202a19b1c06912f0b9503a7331ba881cda3928648b54cd6"
SDKCONFIG_SHA256 = "7490046d6e8b00d80f2bb550439821fa9d4a50da762e6e46d2aa9bdf8d520b8b"
REQUIRED_CONFIG = {
    "CONFIG_COMPILER_OPTIMIZATION_PERF=y",
    "CONFIG_ESP_DEFAULT_CPU_FREQ_MHZ=240",
    "CONFIG_SPIRAM_MODE_OCT=y",
    "CONFIG_ESP_MAIN_TASK_STACK_SIZE=20480",
}


def sha256(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def verify(elf: Path) -> dict[str, object]:
    sdkconfig = elf.parent / "sdkconfig"
    if not sdkconfig.is_file():
        raise ValueError(f"missing build configuration: {sdkconfig}")
    elf_hash = sha256(elf)
    if elf_hash != ELF_SHA256:
        raise ValueError(f"ELF SHA-256 is {elf_hash}, expected {ELF_SHA256}")
    sdkconfig_hash = sha256(sdkconfig)
    if sdkconfig_hash != SDKCONFIG_SHA256:
        raise ValueError(
            f"sdkconfig SHA-256 is {sdkconfig_hash}, expected {SDKCONFIG_SHA256}"
        )
    configured = set(sdkconfig.read_text().splitlines())
    missing = sorted(REQUIRED_CONFIG - configured)
    if missing:
        raise ValueError(f"sdkconfig is missing: {', '.join(missing)}")
    return {
        "ok": True,
        "tinydrawCommit": "7a157d44a9da3312b1ecda2b45b116af2de28e63",
        "idfVersion": "v6.1",
        "elfSha256": elf_hash,
        "sdkconfigSha256": sdkconfig_hash,
        "requiredConfig": sorted(REQUIRED_CONFIG),
    }


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("elf", type=Path)
    parser.add_argument("output", type=Path)
    parser.add_argument("--objdump")
    args = parser.parse_args()
    try:
        result = verify(args.elf)
    except (OSError, ValueError) as error:
        result = {"ok": False, "error": str(error)}
    args.output.write_text(json.dumps(result, indent=2, sort_keys=True) + "\n")
    return 0 if result["ok"] else 2


if __name__ == "__main__":
    raise SystemExit(main())
