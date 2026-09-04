#!/usr/bin/env python3
"""Verify the H2 capture and record its failed adoption gate."""

from __future__ import annotations

import hashlib
import json
import os
from pathlib import Path


ARCHIVE = Path(os.environ.get(
    "ESP32S3_H2_ARCHIVE",
    "/Users/sarah/Archives/esp32s3/"
    "esp32s3-exception-rank-followup-20260904-114902",
))
H2_COMMIT = "1f959078413986a2add01790b3d633b3611c134d"
ARCHIVE_INDEX_SHA256 = (
    "4160ae741b894ad65b8cbc026b9401ef3220fec2ae00eb13afed27f3bf8cca73"
)
ELF_SHA256 = "9d7fb05d7e816d501ac461b468b4eed6560ef1333ef6fb2203a0ebee7a621021"
CELLS = (
    "rfe_alone",
    "rfi3_alone",
    "syscall_rfe_pair",
    "window_overflow8_entry",
    "window_overflow8_control",
    "window_underflow8_entry",
    "window_underflow8_control",
    "rfwo_alone",
    "rfwu_alone",
)
H2_TOTALS = dict(zip(CELLS, (5, 5, 19, 6, 5, 6, 4, 4, 4), strict=True))
H1_DIRECT_TOTALS = {
    "rfe_alone": 6,
    "rfi3_alone": 5,
    "syscall_rfe_pair": 18,
}


def sha256(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def require_hash(path: Path, expected: str) -> None:
    actual = sha256(path.read_bytes())
    if actual != expected:
        raise ValueError(f"{path} sha256 {actual}, expected {expected}")


def verify_archive() -> dict[str, str]:
    index = ARCHIVE / "SHA256SUMS"
    require_hash(index, ARCHIVE_INDEX_SHA256)
    committed = Path(__file__).with_name("archive-SHA256SUMS")
    if index.read_bytes() != committed.read_bytes():
        raise ValueError("committed archive index differs from capture")
    entries = {}
    for line in index.read_text(encoding="utf-8").splitlines():
        digest, relative = line.split("  ", 1)
        require_hash(ARCHIVE / relative, digest)
        entries[relative] = digest
    if entries.get("esp32s3_exception_rank_followup.elf") != ELF_SHA256:
        raise ValueError("archive does not contain the pinned H2 ELF")
    return entries


def verify_proof(entries: dict[str, str]) -> None:
    proof_path = ARCHIVE / "elf-verification.json"
    proof = json.loads(proof_path.read_text())
    if (
        proof.get("sourceCommit") != H2_COMMIT
        or proof.get("sourceDirty") is not False
        or proof.get("idfVersion") != "v6.1"
    ):
        raise ValueError("ELF proof does not pin the clean IDF 6.1 source")
    if proof["artifacts"]["applicationElf"]["sha256"] != ELF_SHA256:
        raise ValueError("ELF proof names a different application ELF")
    if entries.get("elf-verification.json") != sha256(proof_path.read_bytes()):
        raise ValueError("archive index does not pin the ELF proof")
    validation = proof["elfContract"]["executableRankProof"]["independentValidation"]
    if validation["directRawTotals"] != H1_DIRECT_TOTALS:
        raise ValueError("ELF proof names different H1 direct totals")
    if not validation["windowCriterion"].endswith("== 17"):
        raise ValueError("ELF proof names a different H1 window residual")


def parse_boot(path: Path) -> dict[str, int]:
    records = [
        json.loads(line.removeprefix("CAL_RECORD "))
        for line in path.read_text(errors="strict").splitlines()
        if line.startswith("CAL_RECORD ")
    ]
    configs = [row for row in records if row["type"] == "configuration"]
    metrics = [row for row in records if row["type"] == "metric"]
    refusals = [row for row in records if row["type"] == "refusal"]
    if len(configs) != 1 or refusals:
        raise ValueError(f"{path} has a bad configuration or refusal")
    config = configs[0]
    if (
        config.get("idf_version") != "v6.1"
        or config.get("chip_revision") != 2
        or config.get("samples_per_cell") != 100
    ):
        raise ValueError(f"{path} configuration changed")
    if tuple(row["name"] for row in metrics) != CELLS:
        raise ValueError(f"{path} does not contain the exact nine cells")
    totals = {}
    for row in metrics:
        samples = row["ccount_samples"]
        if len(samples) != 100 or len(set(samples)) != 1:
            raise ValueError(f"{path} {row['name']} is not 100 constant samples")
        totals[row["name"]] = samples[0]
    if totals != H2_TOTALS:
        raise ValueError(f"{path} totals changed: {totals}")
    return totals


def verify_session() -> list[dict[str, object]]:
    session = json.loads((ARCHIVE / "session.json").read_text())
    if [row.get("boot") for row in session.get("boots", [])] != [1, 2]:
        raise ValueError("capture is not exactly two boots")
    boots = []
    for row in session["boots"]:
        tally = row["tally"]
        gate = (
            tally.get("capturedSamples"),
            tally.get("completedCells"),
            tally.get("refusals"),
            tally.get("terminalSeen"),
        )
        if gate != (900, 9, 0, True):
            raise ValueError(f"boot {row['boot']} failed its capture gate")
        path = ARCHIVE / row["capture"]
        require_hash(path, row["captureSha256"])
        parse_boot(path)
        boots.append({
            "boot": row["boot"],
            "acceptedSamples": 900,
            "refusals": 0,
            "rawSha256": row["captureSha256"],
        })
    return boots


def main() -> None:
    entries = verify_archive()
    verify_proof(entries)
    boots = verify_session()
    window_excess = (
        H2_TOTALS["window_overflow8_entry"]
        - H2_TOTALS["window_overflow8_control"]
        + H2_TOTALS["window_underflow8_entry"]
        - H2_TOTALS["window_underflow8_control"]
        + H2_TOTALS["rfwo_alone"] - 1
        + H2_TOTALS["rfwu_alone"] - 1
    )
    result = {
        "schemaVersion": 1,
        "sourceCommit": H2_COMMIT,
        "archiveIndexSha256": ARCHIVE_INDEX_SHA256,
        "elfSha256": ELF_SHA256,
        "elfVerificationSha256": entries["elf-verification.json"],
        "boots": boots,
        "rawTotals": H2_TOTALS,
        "declaredH1Gate": {
            "directRawTotals": H1_DIRECT_TOTALS,
            "windowResidual": 17,
        },
        "observedH2WindowExcess": window_excess,
        "adoption": {
            "adopted": False,
            "engineChanged": False,
            "reason": "the declared independent H1 validation gate failed",
        },
    }
    print(json.dumps(result, indent=2, sort_keys=True))


if __name__ == "__main__":
    main()
