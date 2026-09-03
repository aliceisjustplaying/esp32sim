#!/usr/bin/env python3
"""Search declared local fixture roots for the contract-pinned gate ELF."""

from __future__ import annotations

import argparse
import hashlib
import json
import os
from pathlib import Path
from typing import Iterable


EXPECTED_ELF_SHA256 = (
    "1d67c35762fe58b72202a19b1c06912f0b9503a7331ba881cda3928648b54cd6"
)


def default_roots() -> list[Path]:
    home = Path.home()
    return [
        home / "Archives/esp32s3",
        home / "src/a/tinydraw",
        Path("/private/tmp"),
        Path(os.environ.get("TMPDIR", "/tmp")),
        Path("/Users/sarah/tmp"),
    ]


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def audit(roots: Iterable[Path], expected: str = EXPECTED_ELF_SHA256) -> dict[str, object]:
    requested = [str(root) for root in roots]
    searched: list[str] = []
    missing: list[str] = []
    errors: list[str] = []
    candidates: list[dict[str, str]] = []
    seen: set[Path] = set()

    for root in map(Path, requested):
        try:
            resolved = root.expanduser().resolve(strict=True)
        except OSError:
            missing.append(str(root.expanduser()))
            continue
        if resolved in seen:
            continue
        seen.add(resolved)
        searched.append(str(resolved))

        def record_error(error: OSError) -> None:
            errors.append(f"{error.filename}: {error.strerror}")

        for directory, _, files in os.walk(resolved, onerror=record_error):
            if "tinydraw_esp32.elf" not in files:
                continue
            path = Path(directory) / "tinydraw_esp32.elf"
            try:
                digest = sha256(path)
            except OSError as error:
                record_error(error)
                continue
            candidates.append({"path": str(path), "sha256": digest})

    candidates.sort(key=lambda item: item["path"])
    matching = [item["path"] for item in candidates if item["sha256"] == expected]
    return {
        "schemaVersion": 1,
        "expectedElfSha256": expected,
        "requestedRoots": requested,
        "searchedUniqueRoots": searched,
        "missingRoots": missing,
        "candidateElfFiles": len(candidates),
        "matchingContractElfFiles": len(matching),
        "matchingPaths": matching,
        "searchErrors": errors,
        "followSymlinks": False,
    }


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--root", action="append", type=Path, dest="roots")
    args = parser.parse_args()
    print(json.dumps(audit(args.roots or default_roots()), indent=2, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
