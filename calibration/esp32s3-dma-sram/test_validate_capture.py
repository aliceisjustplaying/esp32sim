from __future__ import annotations

import importlib.util
import json
from pathlib import Path

import pytest


ROOT = Path(__file__).resolve().parent
SPEC = importlib.util.spec_from_file_location("dma_capture", ROOT / "validate_capture.py")
assert SPEC is not None and SPEC.loader is not None
MODULE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(MODULE)
MANIFEST = ROOT / "probe-cells.json"


def capture(*, missing: str | None = None, active_false: str | None = None) -> list[str]:
    lines = []
    for offset, cell in enumerate(MODULE.CELL_ORDER):
        if cell == missing:
            continue
        record = {
            "type": "metric",
            "name": cell,
            "operations_per_trial": 8192 if cell in MODULE.CELL_ORDER[:3] else 1,
            "bytes_per_operation": 4 if cell in MODULE.CELL_ORDER[:3] else 32768,
            "ccount_samples": list(range(1 + offset * 10, 101 + offset * 10)),
            "baseline": MODULE.IDLE if cell in (MODULE.PSRAM_ACTIVE, MODULE.SRAM_ACTIVE) else None,
        }
        if cell in (MODULE.PSRAM_ACTIVE, MODULE.SRAM_ACTIVE):
            record["dma_still_in_flight_samples"] = [True] * 100
            if cell == active_false:
                record["dma_still_in_flight_samples"][17] = False
        lines.append("CAL_RECORD " + json.dumps(record, separators=(",", ":")))
    lines.append("CALIBRATION_DONE sink=1")
    return lines


def test_analyzer_computes_nearest_rank_distributions_and_deltas() -> None:
    result = MODULE.analyze(capture(), MANIFEST)
    assert result["cells"][MODULE.IDLE] == {
        "min": 1,
        "median": 50.5,
        "p90": 90,
        "max": 100,
    }
    assert result["deltas"]["copy_psram_to_sram_dma_active_minus_idle"] == {
        "min": 10,
        "median": 10,
        "p90": 10,
        "max": 10,
    }


def test_analyzer_fails_on_missing_cell() -> None:
    with pytest.raises(MODULE.ValidationError, match="missing cells"):
        MODULE.analyze(capture(missing=MODULE.SUBMIT_ONLY), MANIFEST)


def test_analyzer_fails_when_dma_finished_before_copy() -> None:
    with pytest.raises(MODULE.ValidationError, match="without DMA still in flight"):
        MODULE.analyze(capture(active_false=MODULE.PSRAM_ACTIVE), MANIFEST)


def test_analyzer_fails_on_refusal() -> None:
    lines = capture(missing=MODULE.PSRAM_ACTIVE)
    lines.insert(
        1,
        "CAL_RECORD "
        + json.dumps(
            {
                "type": "refusal",
                "name": MODULE.PSRAM_ACTIVE,
                "reason": "SPI2 DMA was not in flight",
            }
        ),
    )
    with pytest.raises(MODULE.ValidationError, match="refused"):
        MODULE.analyze(lines, MANIFEST)
