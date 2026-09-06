#!/usr/bin/env python3
"""Analyze the v2 ROM fetch ladder across variants. Adopts no price.

Usage: analyze_v2.py <variant>=<archive-dir> [<variant>=<archive-dir> ...]
The expected variant is given per archive and must match every boot's BOOT_IDENTITY;
each archive must hold two boots, 24 cells and 100 samples per cell.
Each archive holds boot-N.log files from one variant (the variant name is read
from the BOOT_IDENTITY line). Reports, per variant and boot: boot identity,
per-cell constancy, outlier positions, and for the clzdi2 ROM cells the spacing
of outlier sample starts in CCOUNT cycles (240 MHz). Then applies the declared
attribution rule:

  contention from core 1 is attributed only if the unicore variant shows no
  ROM-cell variation, the dual-hammer variant shows more ROM-cell variation than
  dual-idle, IRAM twins stay constant in every variant, and the variation does
  not follow the clzdi2 position within the run.
"""

import collections
import json
import re
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
EXPECTED_CELLS = {c["id"] for c in json.loads((HERE / "probe-cells.json").read_text())["cells"]}
STARTS_CELLS = {"rom_clzdi2_first", "iram_clzdi2_first", "rom_clzdi2", "iram_clzdi2", "rom_clzdi2_last", "iram_clzdi2_last"}

PREFIX = "CAL_RECORD "
IDENTITY = re.compile(r"BOOT_IDENTITY counter=(\d+) random=0x([0-9a-f]+) rtc_us=(-?\d+) variant=(\S+)")
STARTS = re.compile(r"SAMPLE_STARTS name=(\S+) values=([0-9,]+)")
STATS = re.compile(r"CELL_STATS name=(\S+) attempts=(\d+) accepted=(\d+) rejected_counters=(\d+) rejected_zero=(\d+)")


def read_boot(path: Path) -> dict:
    boot = {"cells": {}, "starts": {}, "stats": {}, "identity": None, "banner": None}
    for line in path.read_text(errors="replace").splitlines():
        if boot["banner"] is None and line.startswith("rst:"):
            boot["banner"] = line
        m = IDENTITY.search(line)
        if m:
            boot["identity"] = {"counter": int(m.group(1)), "random": m.group(2), "rtc_us": int(m.group(3)), "variant": m.group(4)}
        m = STARTS.search(line)
        if m:
            boot["starts"][m.group(1)] = [int(v) for v in m.group(2).split(",")]
        m = STATS.search(line)
        if m:
            boot["stats"][m.group(1)] = {"attempts": int(m.group(2)), "accepted": int(m.group(3)), "rejected_counters": int(m.group(4)), "rejected_zero": int(m.group(5))}
        i = line.find(PREFIX)
        if i >= 0:
            r = json.loads(line[i + len(PREFIX):])
            if r.get("type") == "metric":
                boot["cells"][r["name"]] = [int(v) for v in r["ccount_samples"]]
    return boot


def summarize_cell(samples: list[int]) -> dict:
    c = collections.Counter(samples)
    mode, _ = c.most_common(1)[0]
    outliers = [(i, v) for i, v in enumerate(samples) if v != mode]
    return {"n": len(samples), "mode": mode, "min": min(samples), "max": max(samples),
            "constant": len(c) == 1, "histogram": dict(sorted(c.items())), "outliers": outliers}


def main() -> int:
    report = {"variants": {}, "rule": {}}
    problems = []
    for spec in sys.argv[1:]:
        expected_variant, _, archive = spec.partition("=")
        archive = Path(archive).expanduser()
        logs = sorted(archive.glob("boot-*.log"))
        if len(logs) != 2:
            problems.append(f"{archive}: expected 2 boots, found {len(logs)}")
        for log in logs:
            boot = read_boot(log)
            variant = boot["identity"]["variant"] if boot["identity"] else "unknown"
            if variant != expected_variant:
                problems.append(f"{log}: variant {variant!r} != expected {expected_variant!r}")
            if set(boot["cells"]) != EXPECTED_CELLS or any(len(v) != 100 for v in boot["cells"].values()):
                problems.append(f"{log}: cell set or sample counts differ from the manifest (24 names x 100)")
            if set(boot["stats"]) != EXPECTED_CELLS:
                problems.append(f"{log}: CELL_STATS names differ from the manifest")
            for name, st in boot["stats"].items():
                if st["accepted"] != 100 or st["attempts"] != st["accepted"] + st["rejected_counters"] + st["rejected_zero"]:
                    problems.append(f"{log}: CELL_STATS {name} inconsistent: {st}")
            if set(boot["starts"]) != STARTS_CELLS or any(len(v) != 100 for v in boot["starts"].values()):
                problems.append(f"{log}: SAMPLE_STARTS must cover exactly {sorted(STARTS_CELLS)} with 100 values each")
            for name, starts in boot["starts"].items():
                if any(((b - a_) & 0xFFFFFFFF) == 0 for a_, b in zip(starts, starts[1:])):
                    problems.append(f"{log}: SAMPLE_STARTS {name} has non-advancing start times")
            if boot["identity"] is None:
                problems.append(f"{log}: no BOOT_IDENTITY line")
            session = archive / "session.json"
            if not session.exists():
                problems.append(f"{archive}: no session.json capture receipt")
            else:
                tallies = [b.get("tally", {}) for b in json.loads(session.read_text()).get("boots", [])]
                if len(tallies) != 2 or not all(t.get("complete") and t.get("refusals") == 0 and t.get("terminalSeen") for t in tallies):
                    problems.append(f"{archive}: session receipt does not show two complete refusal-free boots")
            entry = report["variants"].setdefault(variant, {"boots": []})
            cells = {name: summarize_cell(s) for name, s in boot["cells"].items()}
            spacing = {}
            for name, starts in boot["starts"].items():
                summary = cells.get(name)
                if summary and summary["outliers"]:
                    idx = [i for i, _ in summary["outliers"]]
                    spacing[name] = {"outlier_starts": [starts[i] for i in idx],
                                     "gaps_cycles": [(starts[b] - starts[a]) & 0xFFFFFFFF for a, b in zip(idx, idx[1:])],
                                     "sample_period_cycles": ((starts[-1] - starts[0]) & 0xFFFFFFFF) // max(1, len(starts) - 1)}
            entry["boots"].append({"log": str(log), "banner": boot["banner"], "identity": boot["identity"], "stats": boot.get("stats", {}),
                                   "rom_varying": sorted(n for n, c in cells.items() if n.startswith("rom_") and not c["constant"]),
                                   "iram_varying": sorted(n for n, c in cells.items() if n.startswith("iram_") and not c["constant"]),
                                   "cells": cells, "outlier_spacing": spacing})
    v = report["variants"]
    def rom_var(name):
        return [b["rom_varying"] for b in v.get(name, {"boots": []})["boots"]]
    def iram_var_any():
        return any(b["iram_varying"] for e in v.values() for b in e["boots"])
    def position_moves():
        moves = []
        for e in v.values():
            for b in e["boots"]:
                pos = {p: (not b["cells"][f"rom_clzdi2{p}"]["constant"]) for p in ("_first", "", "_last") if f"rom_clzdi2{p}" in b["cells"]}
                moves.append(pos)
        return moves
    identities = [b["identity"] for e in v.values() for b in e["boots"]]
    distinct = len({(i["counter"], i["random"], i["rtc_us"]) for i in identities if i}) == len(identities) and all(identities)
    if not distinct:
        problems.append("boot identities are missing or not pairwise distinct across all boots")
    report["rule"] = {
        "boot_identities_distinct": distinct,
        "unicore_rom_varying": rom_var("unicore"),
        "dual_idle_rom_varying": rom_var("dual-idle"),
        "dual_rom_hammer_rom_varying": rom_var("dual-rom-hammer"),
        "iram_varies_anywhere": iram_var_any(),
        "clzdi2_position_variation": position_moves(),
    }
    contention = (v.get("unicore") and all(not x for x in rom_var("unicore"))
                  and v.get("dual-rom-hammer") and all(len(x) > 0 for x in rom_var("dual-rom-hammer"))
                  and sum(len(x) for x in rom_var("dual-rom-hammer")) > sum(len(x) for x in rom_var("dual-idle"))
                  and not iram_var_any() and distinct)
    iram_hammer_increase = (v.get("dual-iram-hammer") is not None
                            and sum(len(x) for x in rom_var("dual-iram-hammer")) > sum(len(x) for x in rom_var("dual-idle")))
    report["rule"]["dual_iram_hammer_rom_varying"] = rom_var("dual-iram-hammer")
    report["rule"]["iram_hammer_shows_increase"] = iram_hammer_increase
    for required in ("dual-idle", "unicore", "dual-rom-hammer", "dual-iram-hammer"):
        if required not in v:
            problems.append(f"required variant {required!r} has no archive")
    # position rule: the variation must not appear only at one position (first/middle/last)
    # in any dual-idle boot; a body-tied disturbance should not depend on position alone.
    position_only = any(len(pos) == 3 and any(pos.values()) and not all(pos.values())
                        for pos in position_moves())
    report["rule"]["variation_confined_to_one_position"] = position_only
    contention = contention and v.get("dual-iram-hammer") is not None and not iram_hammer_increase and not position_only
    report["problems"] = problems
    report["rule"]["verdict"] = ("input problems; no verdict" if problems else
                                 "consistent with shared-ROM contention from core 1 in this session; the original idle-run mechanism is not thereby established" if contention
                                 else "not attributed: the declared pattern was not observed")
    print(json.dumps(report, indent=2, sort_keys=True))
    return 0


if __name__ == "__main__":
    sys.exit(main())
