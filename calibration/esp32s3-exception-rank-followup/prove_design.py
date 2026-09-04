#!/usr/bin/env python3
"""Prove identifiability of the H2 timing equations using exact rationals."""

from __future__ import annotations

import argparse
from fractions import Fraction
import json
from pathlib import Path


COLUMNS = (
    "rfe", "rfi3", "syscall_entry", "window_overflow8_entry",
    "window_underflow8_entry", "rfwo", "rfwu", "rom_fetch_F",
)
ROWS = {
    "rfe_alone": (1, 0, 0, 0, 0, 0, 0, 0),
    "rfi3_alone": (0, 1, 0, 0, 0, 0, 0, 0),
    "syscall_rfe_pair": (1, 0, 1, 0, 0, 0, 0, 0),
    "window_overflow8_entry": (0, 0, 0, 1, 0, 0, 0, 0),
    "window_underflow8_entry": (0, 0, 0, 0, 1, 0, 0, 0),
    "rfwo_alone": (0, 0, 0, 0, 0, 1, 0, 0),
    "rfwu_alone": (0, 0, 0, 0, 0, 0, 1, 0),
    "rom_minus_iram_control": (0, 0, 0, 0, 0, 0, 0, 2),
}


def exact_rank(rows: list[tuple[int, ...]]) -> int:
    matrix = [[Fraction(value) for value in row] for row in rows]
    rank = 0
    for column in range(len(matrix[0])):
        pivot = next(
            (index for index in range(rank, len(matrix)) if matrix[index][column]),
            None,
        )
        if pivot is None:
            continue
        matrix[rank], matrix[pivot] = matrix[pivot], matrix[rank]
        divisor = matrix[rank][column]
        matrix[rank] = [value / divisor for value in matrix[rank]]
        for index, row in enumerate(matrix):
            if index == rank or not row[column]:
                continue
            factor = row[column]
            matrix[index] = [
                value - factor * pivot_value
                for value, pivot_value in zip(row, matrix[rank], strict=True)
            ]
        rank += 1
    return rank


def proof(rows: dict[str, tuple[int, ...]] = ROWS) -> dict[str, object]:
    if tuple(rows) != tuple(ROWS) or any(rows[name] != ROWS[name] for name in ROWS):
        raise SystemExit("executable timing rows do not match the paper design")
    full = list(rows.values())
    full_rank = exact_rank(full)
    if full_rank != len(COLUMNS):
        raise SystemExit("H2 timing design is not full column rank")
    without_each_column = {
        column: exact_rank([
            tuple(value for index, value in enumerate(row) if index != column_index)
            for row in full
        ])
        for column_index, column in enumerate(COLUMNS)
    }
    if any(rank != len(COLUMNS) - 1 for rank in without_each_column.values()):
        raise SystemExit("H2 timing design has a non-identifiable column")
    return {
        "schemaVersion": 2,
        "columns": list(COLUMNS),
        "rows": {name: list(row) for name, row in rows.items()},
        "rank": full_rank,
        "rankWithoutEachColumn": without_each_column,
        "rowBoundaries": {
            "rfe_alone": "raw - rsr.ccount(1) = rfe",
            "rfi3_alone": "raw - rsr.ccount(1) = rfi3",
            "syscall_rfe_pair": "raw - rsr.ccount(1) - syscall(1) - handler_prefix(4) = syscall_entry + rfe",
            "window_overflow8_entry": "overflow_raw - matched_no_overflow_raw = window_overflow8_entry",
            "window_underflow8_entry": "underflow_raw - matched_no_underflow_raw = window_underflow8_entry",
            "rfwo_alone": "raw - rsr.ccount(1) = rfwo",
            "rfwu_alone": "raw - rsr.ccount(1) = rfwu",
            "rom_minus_iram_control": "rom_raw - iram_raw = 2 * rom_fetch_F",
        },
        "independentValidation": {
            "receiptCommit": "c6c0d5af528f0988004b7f77427a9259d9d2db3a",
            "sourceCommit": "75778a4cfef4332b09b7e0595d36fde188d0c118",
            "summaryPath": "docs/evidence/timing/h1-exception-ladders-2026-09-04/summary.json",
            "summarySha256": "511dd814024a7385dc2185f9f155819802c8e81e913568307c311262b541a613",
            "directRawTotals": {
                "rfe_alone": 6,
                "rfi3_alone": 5,
                "syscall_rfe_pair": 18,
                "mask_rom_fetch_straight_line": 15,
            },
            "windowCriterion": "window_overflow8_entry + window_underflow8_entry + rfwo + rfwu == 17",
            "windowKnownEighteenCycles": "two verified nine-cycle IDF 6.1 handler prefixes",
            "adoptionGate": "unadopted until a committed H2 capture matches every unused H1 validation target",
        },
        "executableProof": {
            "required": True,
            "description": "verify_elf.py reconstructs the rows after exact boundaries and all restoration exits pass",
        },
        "romBoundary": {
            "equation": "rom_total - iram_control_total = 2 * rom_fetch_F",
            "requirements": [
                "both targets are exactly entry a1, 16; retw.n",
                "both targets have address residue zero modulo four",
                "both use the same callx8 measurement wrapper",
                "PS.WOE is clear and WINDOWBASE/WINDOWSTART match for both calls",
                "all accepted samples have zero cache-counter and state deltas",
            ],
        },
    }


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--check", type=Path)
    args = parser.parse_args()
    rendered = json.dumps(proof(), indent=2, sort_keys=True) + "\n"
    if args.check is not None:
        if json.loads(args.check.read_text(encoding="utf-8")) != proof():
            raise SystemExit(f"{args.check} does not match the exact H2 rank proof")
        return
    print(rendered, end="")


if __name__ == "__main__":
    main()
