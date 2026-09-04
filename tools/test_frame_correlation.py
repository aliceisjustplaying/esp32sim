#!/usr/bin/env python3
"""Acceptance checks for frame correlation."""

import copy
import unittest
from pathlib import Path

import frame_correlation as correlation


FIXTURES = Path(__file__).resolve().parents[1] / "tests" / "correlation" / "frame-v1"


class FrameCorrelationTest(unittest.TestCase):
    def test_partitioned_comparison(self):
        hardware = correlation.load(FIXTURES / "hardware-partitioned.ndjson")
        emulator = correlation.load(FIXTURES / "emulator-partitioned.ndjson")
        report, passed = correlation.compare(hardware, emulator)
        self.assertTrue(passed)
        self.assertEqual(report["nonPsram"]["frames"][1]["errorPercent"], 1.0)
        self.assertIsNone(report["psram"]["scalarErrorPercent"])

        missed = copy.deepcopy(emulator)
        missed["frames"][0]["non_psram_cycles"] = 1011
        missed["frames"][0]["total_cycles"] = 1311
        self.assertFalse(correlation.compare(hardware, missed)[1])

        unknown = copy.deepcopy(emulator)
        unknown["frames"][0]["unknown_components"] = ["rtc"]
        with self.assertRaises(correlation.Refusal):
            correlation.compare(hardware, unknown)

    def test_paired_psram_candidate(self):
        slow = correlation.load(FIXTURES / "hardware-40mhz.ndjson")
        fast = correlation.load(FIXTURES / "hardware-80mhz.ndjson")
        report = correlation.psram_candidate(slow, fast)
        self.assertEqual(report["classification"], "distribution")
        self.assertEqual(report["onePercentClaim"], "refused")
        self.assertIsNone(report["nonPsramPartition"])
        self.assertEqual(report["cacheCounters"]["dbus_psram_misses"]["slowMinusFast"]["max"], 0)

        mismatch = copy.deepcopy(slow)
        mismatch["frames"][0]["kind"] = "wrong"
        with self.assertRaises(correlation.Refusal):
            correlation.psram_candidate(mismatch, fast)


if __name__ == "__main__":
    unittest.main()
