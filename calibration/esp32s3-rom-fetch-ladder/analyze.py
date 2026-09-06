#!/usr/bin/env python3
"""Analyze a ROM fetch ladder capture. Fits nothing it was not told to fit.

Inputs: an archive directory holding boot-1.log and boot-2.log (CAL_RECORD lines),
plus rom-cells.json beside this script. Outputs a JSON summary to stdout.

Rules applied (declared in README.md before capture):
- every cell must have all samples identical in a boot, else the cell is a
  distribution and the pair is not usable for an exact fit;
- D_k = rom_k - iram_k per pair and per boot;
- models are fitted only on boot 1 of the fitting pairs; the held-out pairs and
  all of boot 2 must be reproduced exactly, else the model is refused;
- the verdict names a tier candidate; it adopts nothing.
"""

import json
import sys
from fractions import Fraction
from pathlib import Path

HERE = Path(__file__).resolve().parent
FITTING = ["p_none", "abs", "mulsi3", "temp_to_power", "ctzsi2", "clzdi2"]
HELD_OUT = ["clzsi2", "roundup2", "ffssi2", "ctzdi2"]
PREFIX = "CAL_RECORD "


def read_boot(path: Path) -> dict[str, list[int]]:
    cells: dict[str, list[int]] = {}
    for line in path.read_text().splitlines():
        index = line.find(PREFIX)
        if index < 0:
            continue
        record = json.loads(line[index + len(PREFIX):])
        if record.get("type") == "metric":
            cells[record["name"]] = [int(v) for v in record["ccount_samples"]]
        elif record.get("type") == "refusal":
            cells[record["name"]] = []
    return cells


def constant(samples: list[int]) -> int | None:
    return samples[0] if samples and all(s == samples[0] for s in samples) else None


def solve_two(points: list[tuple[int, int]]) -> tuple[Fraction, Fraction] | None:
    """Least-squares-free exact solve: find (f0, f) with D = f0 + f*x for ALL points."""
    (x0, d0), (x1, d1) = points[0], points[1]
    if x0 == x1:
        return None
    f = Fraction(d1 - d0, x1 - x0)
    f0 = Fraction(d0) - f * x0
    if all(Fraction(d) == f0 + f * x for x, d in points):
        return f0, f
    return None


def fit_models(d_fit: dict[str, int], cells: dict[str, dict]) -> list[dict]:
    """Try the declared models in order on the fitting set; return each with its parameters."""
    results = []
    # model 1: zero
    results.append({"model": "zero", "fits": all(v == 0 for v in d_fit.values()), "params": {}})
    for key, name in (("fetchWords32", "per-word"), ("instructions", "per-instruction")):
        points = [(cells[k][key], d) for k, d in d_fit.items()]
        # proportional
        ratios = {Fraction(d, x) for x, d in points if x}
        prop = len(ratios) == 1 and all(Fraction(d) == next(iter(ratios)) * x for x, d in points)
        results.append({"model": name, "fits": prop, "params": {"f": str(next(iter(ratios))) if prop else None}})
        affine = solve_two(points)
        results.append({"model": name + "-affine", "fits": affine is not None,
                        "params": {"f0": str(affine[0]), "f": str(affine[1])} if affine else {}})
    return results


def predict(model: dict, cell: dict) -> Fraction | None:
    p = model["params"]
    if model["model"] == "zero":
        return Fraction(0)
    key = "fetchWords32" if model["model"].startswith("per-word") else "instructions"
    if model["model"].endswith("-affine"):
        return Fraction(p["f0"]) + Fraction(p["f"]) * cell[key]
    return Fraction(p["f"]) * cell[key]


def main() -> int:
    archive = Path(sys.argv[1]).expanduser()
    cells = {c["short"]: c for c in json.loads((HERE / "rom-cells.json").read_text())}
    boots = {n: read_boot(archive / f"boot-{n}.log") for n in (1, 2) if (archive / f"boot-{n}.log").exists()}
    summary = {"archive": str(archive), "boots": {}, "pairs": {}, "verdict": None}
    d_by_boot: dict[int, dict[str, int]] = {}
    for n, data in boots.items():
        boot = {}
        d_by_boot[n] = {}
        for short in cells:
            rom, iram = data.get(f"rom_{short}", []), data.get(f"iram_{short}", [])
            crom, ciram = constant(rom), constant(iram)
            boot[short] = {"rom": {"n": len(rom), "constant": crom, "min": min(rom) if rom else None, "max": max(rom) if rom else None},
                           "iram": {"n": len(iram), "constant": ciram, "min": min(iram) if iram else None, "max": max(iram) if iram else None}}
            if crom is not None and ciram is not None:
                d_by_boot[n][short] = crom - ciram
                boot[short]["D"] = crom - ciram
        summary["boots"][n] = boot
    for short in cells:
        summary["pairs"][short] = {"words": cells[short]["fetchWords32"], "instructions": cells[short]["instructions"],
                                   "D": {n: d_by_boot[n].get(short) for n in boots}}
    if 1 not in d_by_boot or any(k not in d_by_boot[1] for k in FITTING):
        summary["verdict"] = {"tier": "unexplained", "reason": "a fitting pair is missing or not constant in boot 1"}
        print(json.dumps(summary, indent=2, sort_keys=True))
        return 0
    d_fit = {k: d_by_boot[1][k] for k in FITTING}
    models = fit_models(d_fit, cells)
    accepted = None
    for model in models:
        if not model["fits"]:
            continue
        validation = []
        ok = True
        for n in boots:
            for short in (HELD_OUT if n == 1 else list(cells)):
                observed = d_by_boot[n].get(short)
                expected = predict(model, cells[short])
                good = observed is not None and Fraction(observed) == expected
                ok &= good
                validation.append({"boot": n, "pair": short, "observed": observed, "expected": str(expected), "ok": good})
        model["validation"] = validation
        model["validated"] = ok
        if ok and accepted is None:
            accepted = model
    summary["models"] = models
    if accepted is None:
        summary["verdict"] = {"tier": "interval-or-unexplained", "reason": "no declared model reproduces every held-out pair and boot 2 exactly"}
    else:
        integer = all(Fraction(v).denominator == 1 for v in accepted["params"].values()) if accepted["params"] else True
        summary["verdict"] = {"tier": "exact-candidate" if integer else "interval", "model": accepted["model"], "params": accepted["params"],
                              "scope": "matched windowed-call bodies in ROM versus IRAM; not a per-instruction or reset-vector price; adoption requires a non-window-call validation ladder"}
    print(json.dumps(summary, indent=2, sort_keys=True))
    return 0


if __name__ == "__main__":
    sys.exit(main())
