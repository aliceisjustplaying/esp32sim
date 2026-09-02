#!/usr/bin/env python3
"""Recompute same-image core-1 contention candidates from IDF 6.1 receipts."""

from __future__ import annotations

import hashlib
import json
import math
import statistics
import tarfile
from pathlib import Path
from typing import Any


ROOT = Path(__file__).resolve().parent
SOURCE = ROOT.parent / "idf61-rebaseline-3db3985" / "receipts"
ARCHIVES = tuple(sorted(SOURCE.glob("boot-*-recovered.tar.gz")))
SINGLE = "_single_core"
CONTENDED = "_core1_contended"
FAMILIES = (
    "branch",
    "cache burst",
    "cache hit",
    "dependent load",
    "MMIO read",
    "MMIO write",
    "PSRAM pattern",
    "ROM routine",
    "oracle",
)


def fail(condition: bool, message: str) -> None:
    if not condition:
        raise ValueError(message)


def number(value: int | float) -> int | float:
    return int(value) if float(value).is_integer() else value


def median(values: list[int | float]) -> int | float:
    return number(statistics.median(values))


def stats(values: list[int | float]) -> dict[str, int | float]:
    fail(bool(values), "cannot summarize an empty observation set")
    ordered = sorted(values)
    return {
        "min": number(ordered[0]),
        "median": median(ordered),
        "p90": number(ordered[math.ceil(0.9 * len(ordered)) - 1]),
        "max": number(ordered[-1]),
    }


def family(name: str) -> str:
    if name.startswith("conditional_branch_"):
        return "branch"
    if name.startswith(("dcache_flash_burst_", "dcache_psram_burst_", "icache_flash_burst_")):
        return "cache burst"
    if name.startswith(("dcache_hit_", "icache_hit_", "flash_instruction_", "flash_mmap_")):
        return "cache hit"
    if name.startswith("dependent_load_"):
        return "dependent load"
    if name.startswith("mmio_read_"):
        return "MMIO read"
    if name.startswith("mmio_write_"):
        return "MMIO write"
    if name.startswith("psram_"):
        return "PSRAM pattern"
    if name.startswith("rom_"):
        return "ROM routine"
    fail(
        name.startswith(("reset_reason_", "rgb565_", "sram_")),
        f"unclassified identity {name}",
    )
    return "oracle"


def classification(observations: list[int | float], paired_boots: int) -> dict[str, Any]:
    measured = stats(observations)
    if (
        paired_boots >= 2
        and measured["min"] == measured["max"]
        and float(measured["min"]).is_integer()
    ):
        return {"tier": "exact", "value": int(measured["min"])}
    if paired_boots >= 2 and measured["max"] - measured["min"] == 1:
        return {"tier": "interval", "range": [measured["min"], measured["max"]]}
    return {"tier": "distribution", **measured}


def read_boot(path: Path) -> tuple[str, dict[str, dict[str, Any]]]:
    result: dict[str, dict[str, Any]] = {}
    boot_id: str | None = None
    with tarfile.open(path, "r:gz") as archive:
        for member in archive.getmembers():
            if not member.isfile() or not member.name.endswith(".json"):
                continue
            stream = archive.extractfile(member)
            fail(stream is not None, f"cannot read {member.name} from {path.name}")
            receipt = json.load(stream)
            fail(receipt["schemaVersion"] == 1, f"{member.name}: schema mismatch")
            fail(receipt["captureMode"] == "hardware", f"{member.name}: not hardware")
            fail(receipt["toolchain"]["espIdfVersion"] == "v6.1", f"{member.name}: IDF mismatch")
            fail(receipt["toolchain"]["compilerVersion"] == "15.2.0", f"{member.name}: compiler mismatch")
            fail(receipt["git"]["dirty"] is False, f"{member.name}: dirty source")
            fail(receipt["measurement"]["kind"] == "ccount-kernel", f"{member.name}: kind mismatch")
            current_boot_id = receipt["boot"]["bootId"]
            if boot_id is None:
                boot_id = current_boot_id
            fail(current_boot_id == boot_id, f"{path.name}: mixed boot identities")
            name = receipt["measurement"]["kernel"]
            samples = receipt["measurement"]["samples"]
            cycles = [sample["cycles"] for sample in samples]
            fail(bool(cycles) and all(isinstance(value, int) for value in cycles), f"{name}: bad cycles")
            fail(name not in result, f"{path.name}: duplicate {name}")
            result[name] = {
                "samples": len(cycles),
                "medianCycles": median(cycles),
            }
    fail(boot_id is not None, f"{path.name}: empty archive")
    return boot_id, result


def main() -> None:
    fail(len(ARCHIVES) == 4, f"expected four receipt archives, found {len(ARCHIVES)}")
    boots = [read_boot(path) for path in ARCHIVES]
    all_names = {name for _, records in boots for name in records}
    bases = sorted(
        name.removesuffix(SINGLE)
        for name in all_names
        if name.endswith(SINGLE) and name.removesuffix(SINGLE) + CONTENDED in all_names
    )
    fail(len(bases) == 103, f"expected 103 identities with both variants, found {len(bases)}")

    identities: dict[str, Any] = {}
    grouped: dict[str, list[str]] = {name: [] for name in FAMILIES}
    for base in bases:
        per_boot = []
        paired_deltas: list[int | float] = []
        single_medians: list[int | float] = []
        contended_medians: list[int | float] = []
        for boot_number, ((boot_id, records), archive_path) in enumerate(zip(boots, ARCHIVES), 1):
            single = records.get(base + SINGLE)
            contended = records.get(base + CONTENDED)
            row: dict[str, Any] = {
                "boot": boot_number,
                "bootId": boot_id,
                "receiptArchive": f"../idf61-rebaseline-3db3985/receipts/{archive_path.name}",
            }
            if single is not None:
                row["singleCore"] = single
                single_medians.append(single["medianCycles"])
            if contended is not None:
                row["core1Contended"] = contended
                contended_medians.append(contended["medianCycles"])
            if single is not None and contended is not None:
                delta = number(contended["medianCycles"] - single["medianCycles"])
                row["deltaCycles"] = delta
                paired_deltas.append(delta)
            if single is not None or contended is not None:
                per_boot.append(row)

        fail(single_medians and contended_medians, f"{base}: missing a variant")
        if paired_deltas:
            observations = paired_deltas
            basis = "same-boot median pairs"
        else:
            observations = [
                number(contended - single)
                for contended in contended_medians
                for single in single_medians
            ]
            basis = "independent cross-boot differences; no same-boot pair survived capture"
        item_family = family(base)
        identities[base] = {
            "family": item_family,
            "boots": per_boot,
            "deltaObservationBasis": basis,
            "deltaObservationsCycles": observations,
            "deltaStatisticsCycles": stats(observations),
            "classification": classification(observations, len(paired_deltas)),
        }
        grouped[item_family].append(base)

    family_summary: dict[str, Any] = {}
    tier_rank = {"exact": 0, "interval": 1, "distribution": 2}
    for name in FAMILIES:
        members = grouped[name]
        tiers = [identities[member]["classification"]["tier"] for member in members]
        counts = {tier: tiers.count(tier) for tier in tier_rank}
        exact_values = [
            identities[member]["classification"]["value"]
            for member in members
            if identities[member]["classification"]["tier"] == "exact"
        ]
        if len(exact_values) == len(members) and set(exact_values) == {0}:
            behavior = "zero"
        elif len(exact_values) == len(members):
            behavior = "constant per identity"
        else:
            behavior = "spread"
        family_summary[name] = {
            "classification": max(tiers, key=tier_rank.get),
            "behavior": behavior,
            "identityCount": len(members),
            "identityTierCounts": counts,
            "identities": members,
        }

    source_archives = []
    for path in ARCHIVES:
        source_archives.append(
            {
                "path": f"../idf61-rebaseline-3db3985/receipts/{path.name}",
                "sha256": hashlib.sha256(path.read_bytes()).hexdigest(),
            }
        )
    output = {
        "schemaVersion": 1,
        "source": {
            "tinyDrawCommit": "3db39856ecec411cbb7c4b697f6b47e65b5a8f2a",
            "espIdf": "v6.1",
            "compiler": "xtensa-esp-elf 15.2.0",
            "archives": source_archives,
        },
        "statistics": {
            "receiptValue": "median of raw CCOUNT cycle samples within each boot and identity",
            "delta": "core-1-contended boot median minus single-core boot median",
            "p90": "nearest rank ceil(0.90 * n)",
            "classification": "exact requires at least two identical integer same-boot deltas; interval requires a one-cycle same-boot range; all other cases are distributions",
        },
        "identityCount": len(identities),
        "families": family_summary,
        "identities": identities,
    }
    (ROOT / "summary.json").write_text(
        json.dumps(output, indent=2, sort_keys=True) + "\n", encoding="utf-8"
    )


if __name__ == "__main__":
    main()
