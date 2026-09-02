#!/usr/bin/env python3

import json
import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

from ndjson import (
    ATTRIBUTION_CHECKSUMS,
    ATTRIBUTION_ITERATIONS,
    CAL_DONE,
    PREFIX,
    CalibrationValidator,
    CaptureValidator,
    CellContract,
    ManifestContract,
    PSRAM_SERVICE_BYTES,
    ValidationError,
    design_ranks,
    expected_aggressor_checksum,
    expected_psram_clock_register,
    validate_path,
)


def contract() -> ManifestContract:
    return ManifestContract(
        protocol_version=2,
        harness_version="0.2.0-review",
        chip_model="ESP32-S3",
        chip_revision=2,
        cells=(
            CellContract("store_hit_psram", 2, ("normal", "xip-psram")),
            CellContract("instruction_psram_hot", 1, ("xip-psram",)),
            CellContract("instruction_psram_cold", 1, ("xip-psram",)),
        ),
    )


def manifest_payload() -> dict[str, object]:
    return {
        "protocolVersion": 2,
        "harnessVersion": "1.0.0",
        "chipModel": "ESP32-S3",
        "chipRevision": 2,
        "cells": [
            {
                "id": "rtc_read",
                "family": "register-read",
                "samples": 100,
                "variants": ["normal"],
            }
        ],
    }


def contention_contract() -> ManifestContract:
    return ManifestContract(
        protocol_version=2,
        harness_version="0.2.0-review",
        chip_model="ESP32-S3",
        chip_revision=2,
        cells=tuple(
            CellContract(cell, 1, ("normal",))
            for cell in (
                "arbitration_psram_victim_internal_aggressor",
                "arbitration_psram_victim_flash_aggressor",
                "arbitration_psram_victim_psram_aggressor",
                "psram_bandwidth_cross_core",
                "flash_bandwidth_cross_core",
            )
        ),
    )


def console_contract() -> ManifestContract:
    return ManifestContract(
        protocol_version=2,
        harness_version="gate-main-7a157d4",
        chip_model="ESP32-S3",
        chip_revision=0,
        terminal_line="TINYDRAW_GATE1_AUTOMATED_DONE",
        cells=(
            CellContract(
                "live_present",
                2,
                ("normal",),
                family="console-line",
                console_line="TINYDRAW_LIVE_PRESENT",
                microsecond_fields=("compose_us", "transfer_wait_us"),
            ),
            CellContract(
                "live_stress",
                1,
                ("normal",),
                family="console-line",
                console_line="TINYDRAW_LIVE_STRESS",
                microsecond_fields=("total_us", "maximum_us"),
            ),
        ),
    )


def line(record: str, **fields: object) -> str:
    return PREFIX + json.dumps({"protocolVersion": 2, "record": record, **fields})


def counters(**updates: int) -> dict[str, int]:
    result = {
        "ibusAccesses": 0,
        "ibusMisses": 0,
        "dbusAccesses": 256,
        "dbusFlashMisses": 0,
        "dbusPsramMisses": 0,
    }
    result.update(updates)
    return result


def classifier(start: int = 0x3C02AC00, end: int = 0x3C06ABFF) -> dict[str, int]:
    return {"start": start, "end": end}


def metadata(**updates: object) -> str:
    fields: dict[str, object] = {
        "suite": "tier-b",
        "harnessVersion": "0.2.0-review",
        "idfVersion": "v6.1",
        "spiramRodata": False,
        "gitCommit": "a" * 40,
        "gitDirty": False,
        "variant": "normal",
        "sdkconfigSha256": "b" * 64,
        "manifestSha256": "d" * 64,
        "compilerVersion": "15.2.0",
        "elfSha256": "c" * 64,
        "dbusFlashClassifier": classifier(),
        "chipModel": "ESP32-S3",
        "chipRevision": 2,
        "resetReason": 1,
        "bootId": "1-0123456789abcdef",
        "availableCells": ["store_hit_psram"],
        "selectedCells": ["store_hit_psram"],
    }
    fields.update(updates)
    return line("metadata", **fields)


def complete_lines() -> list[str]:
    return [
        metadata(),
        line("cell-start", cell="store_hit_psram", expectedSamples=2),
        line(
            "sample",
            cell="store_hit_psram",
            ordinal=0,
            cycles=17,
            baselineCycles=8,
            bytes=1024,
            startCore=0,
            endCore=0,
            cacheCounters=counters(),
            baselineCacheCounters=counters(dbusAccesses=0),
        ),
        line(
            "sample",
            cell="store_hit_psram",
            ordinal=1,
            cycles=18,
            baselineCycles=8,
            bytes=1024,
            startCore=0,
            endCore=0,
            cacheCounters=counters(),
            baselineCacheCounters=counters(dbusAccesses=0),
        ),
        line("cell-complete", cell="store_hit_psram", samples=2),
        line(
            "run-complete",
            selectedCells=1,
            completedCells=1,
            samples=2,
            refusals=0,
        ),
    ]


class CaptureValidatorTest(unittest.TestCase):
    def validator(self, expected_build: dict[str, object] | None = None) -> CaptureValidator:
        return CaptureValidator(contract(), "normal", "store_hit_psram", expected_build)

    def contention_validator(self, cell: str) -> CaptureValidator:
        actual = contention_contract()
        validator = CaptureValidator(actual, "normal", cell)
        validator.feed_line(
            metadata(
                availableCells=[item.id for item in actual.available("normal")],
                selectedCells=[cell],
            ),
            1,
        )
        validator.feed_line(line("cell-start", cell=cell, expectedSamples=1), 2)
        return validator

    def instruction_validator(self, cell: str) -> CaptureValidator:
        validator = CaptureValidator(contract(), "xip-psram", cell)
        validator.feed_line(
            metadata(
                variant="xip-psram",
                availableCells=[
                    "store_hit_psram",
                    "instruction_psram_hot",
                    "instruction_psram_cold",
                ],
                selectedCells=[cell],
            ),
            1,
        )
        validator.feed_line(line("cell-start", cell=cell, expectedSamples=1), 2)
        return validator

    def decomposition_validator(self, cell: CellContract) -> CaptureValidator:
        actual = ManifestContract(
            protocol_version=2,
            harness_version="0.2.0-review",
            chip_model="ESP32-S3",
            chip_revision=2,
            cells=(cell,),
        )
        validator = CaptureValidator(actual, "normal", cell.id)
        validator.feed_line(
            metadata(availableCells=[cell.id], selectedCells=[cell.id]), 1
        )
        validator.feed_line(
            line("cell-start", cell=cell.id, expectedSamples=cell.samples), 2
        )
        return validator

    @staticmethod
    def msync_cell() -> CellContract:
        return CellContract(
            "msync_decompose_l16_d0_p40",
            1,
            ("normal", "xip-psram"),
            family="msync-decomposition",
            factors=(("bytes", 1024), ("dirtyLines", 0), ("psramClockHz", 40_000_000)),
        )

    @staticmethod
    def spi2_cell() -> CellContract:
        return CellContract(
            "spi2_phased_b4096_c20",
            1,
            ("normal", "xip-psram"),
            family="spi2-decomposition",
            factors=(("bytes", 4096), ("spiClockHz", 20_000_000)),
        )

    @staticmethod
    def msync_sample(**updates: object) -> str:
        fields: dict[str, object] = {
            "cell": "msync_decompose_l16_d0_p40",
            "ordinal": 0,
            "cycles": 30,
            "bytes": 1024,
            "startCore": 0,
            "endCore": 0,
            "cacheCounters": counters(dbusAccesses=0),
            "dirtyLines": 0,
            "psramClockHz": 40_000_000,
            "psramClockRegister": expected_psram_clock_register(40_000_000),
            "psramCoreClockRegister": 2,
            "psramServiceBytes": PSRAM_SERVICE_BYTES,
            "psramServiceCycles": 80,
            "psramServiceCounters": counters(dbusAccesses=64, dbusPsramMisses=64),
        }
        fields.update(updates)
        return line("sample", **fields)

    @staticmethod
    def spi2_sample(**updates: object) -> str:
        fields: dict[str, object] = {
            "cell": "spi2_phased_b4096_c20",
            "ordinal": 0,
            "cycles": 100,
            "bytes": 4096,
            "startCore": 0,
            "endCore": 0,
            "cacheCounters": counters(dbusAccesses=0),
            "spiClockHz": 20_000_000,
            "submissionCycles": 15,
            "completionCycles": 85,
        }
        fields.update(updates)
        return line("sample", **fields)

    @staticmethod
    def instruction_sample(cell: str, accesses: int, misses: int) -> str:
        return line(
            "sample",
            cell=cell,
            ordinal=0,
            cycles=9,
            bytes=256,
            startCore=0,
            endCore=0,
            cacheCounters=counters(
                ibusAccesses=accesses,
                ibusMisses=misses,
                dbusAccesses=0,
            ),
        )

    def contention_sample(
        self, cell: str, aggressor_counters: dict[str, int]
    ) -> str:
        victim = (
            counters(dbusAccesses=32, dbusFlashMisses=4, dbusPsramMisses=0)
            if cell == "flash_bandwidth_cross_core"
            else counters(dbusAccesses=32, dbusFlashMisses=4, dbusPsramMisses=4)
        )
        source = "internal"
        if "flash_aggressor" in cell:
            source = "flash"
        elif "psram_aggressor" in cell or cell.endswith("_cross_core"):
            source = "psram"
        return line(
            "sample",
            cell=cell,
            ordinal=0,
            cycles=100,
            bytes=4096,
            startCore=0,
            endCore=0,
            cacheCounters=victim,
            baselineCycles=80,
            baselineCacheCounters=victim,
            attributionSource=source,
            isolatedAttributionIterations=ATTRIBUTION_ITERATIONS,
            isolatedAttributionChecksum=ATTRIBUTION_CHECKSUMS[source],
            isolatedAttributionCounters=aggressor_counters,
            aggressorIterations=64,
            aggressorChecksum=expected_aggressor_checksum(source, 64),
        )

    def test_complete_capture_uses_manifest_counts(self) -> None:
        validator = self.validator()
        for index, record in enumerate(complete_lines(), 1):
            validator.feed_line(record, index)
        tally = validator.finalize()
        self.assertEqual(tally.expected_cells, 1)
        self.assertEqual(tally.expected_samples, 2)
        self.assertTrue(tally.as_dict()["complete"])

    def test_committed_manifest_defines_variant_cells(self) -> None:
        root = Path(__file__).resolve().parents[1]
        actual = ManifestContract.load(
            root / "esp32s3-core-timing" / "probe-cells.json"
        )
        cells = actual.available("normal")
        self.assertEqual(len(cells), 29)
        self.assertIn("intr_entry_level3", [cell.id for cell in cells])

    def test_committed_manifest_does_not_invoke_tier_b_design_ranks(self) -> None:
        root = Path(__file__).resolve().parents[1]
        actual = ManifestContract.load(
            root / "esp32s3-core-timing" / "probe-cells.json"
        )
        self.assertEqual(design_ranks(actual.cells), {})
        self.assertEqual(sum(cell.samples for cell in actual.cells), 357)

    def test_manifest_accepts_rtc_domain_and_register_exclusions(self) -> None:
        payload = manifest_payload()
        payload["cells"][0]["clockDomain"] = "rtc"
        payload["exclusions"] = [
            {
                "register": "0x60038000",
                "block": "USB_SERIAL_JTAG",
                "reason": "EP1 FIFO write changes device state",
            },
            {
                "register": "0x60007000-0x60007028",
                "block": "EFUSE",
                "reason": "Programming bank changes one-time state",
            },
        ]
        actual = ManifestContract.from_bytes(json.dumps(payload).encode())
        self.assertEqual(actual.cells[0].clock_domain, "rtc")
        self.assertEqual(actual.exclusions[0].register, "0x60038000")
        self.assertEqual(actual.exclusions[1].block, "EFUSE")

    def test_manifest_keeps_new_fields_optional(self) -> None:
        without_exclusions = ManifestContract.from_bytes(
            json.dumps(manifest_payload()).encode()
        )
        self.assertIsNone(without_exclusions.cells[0].clock_domain)
        self.assertEqual(without_exclusions.exclusions, ())
        payload = manifest_payload()
        payload["exclusions"] = []
        with_empty_exclusions = ManifestContract.from_bytes(
            json.dumps(payload).encode()
        )
        self.assertEqual(with_empty_exclusions.exclusions, ())
        positional = ManifestContract(2, "1.0.0", "ESP32-S3", 2, (), "a" * 64)
        self.assertEqual(positional.manifest_sha256, "a" * 64)
        self.assertEqual(positional.exclusions, ())

    def test_manifest_rejects_unsupported_clock_domain(self) -> None:
        for value in ("apb", ""):
            with self.subTest(value=value):
                payload = manifest_payload()
                payload["cells"][0]["clockDomain"] = value
                with self.assertRaisesRegex(ValidationError, "clockDomain"):
                    ManifestContract.from_bytes(json.dumps(payload).encode())

    def test_manifest_rejects_malformed_exclusions(self) -> None:
        malformed = (
            "not-an-array",
            [{}],
            [{"register": "0x60038000", "block": "USB_SERIAL_JTAG"}],
            [
                {
                    "register": "0x60038000",
                    "block": "USB_SERIAL_JTAG",
                    "reason": "side effect",
                    "extra": "rejected",
                }
            ],
            [{"register": "0x60038000", "block": "", "reason": "side effect"}],
        )
        for exclusions in malformed:
            with self.subTest(exclusions=exclusions):
                payload = manifest_payload()
                payload["exclusions"] = exclusions
                with self.assertRaises(ValidationError):
                    ManifestContract.from_bytes(json.dumps(payload).encode())

    def test_manifest_rejects_invalid_or_duplicate_register_exclusions(self) -> None:
        for register in ("0x6003800", "60038000", "0x60007028-0x60007000", 0x60038000):
            with self.subTest(register=register):
                payload = manifest_payload()
                payload["exclusions"] = [
                    {
                        "register": register,
                        "block": "SYSTEM",
                        "reason": "side effect",
                    }
                ]
                with self.assertRaises(ValidationError):
                    ManifestContract.from_bytes(json.dumps(payload).encode())
        payload = manifest_payload()
        exclusion = {
            "register": "0x60038000",
            "block": "USB_SERIAL_JTAG",
            "reason": "side effect",
        }
        payload["exclusions"] = [exclusion, exclusion]
        with self.assertRaisesRegex(ValidationError, "duplicates"):
            ManifestContract.from_bytes(json.dumps(payload).encode())

    def test_manifest_new_fields_preserve_unknown_key_rejection(self) -> None:
        payload = manifest_payload()
        payload["unknown"] = []
        with self.assertRaisesRegex(ValidationError, "unexpected keys"):
            ManifestContract.from_bytes(json.dumps(payload).encode())
        payload = manifest_payload()
        payload["cells"][0]["clock_domain"] = "rtc"
        with self.assertRaisesRegex(ValidationError, "unexpected keys"):
            ManifestContract.from_bytes(json.dumps(payload).encode())

    def test_manifest_rejects_a_missing_noop_control(self) -> None:
        root = Path(__file__).resolve().parents[1]
        path = root / "esp32s3-core-timing" / "probe-cells.json"
        payload = json.loads(path.read_text())
        payload["cells"].append(payload["cells"][0])
        with self.assertRaisesRegex(ValidationError, "duplicate IDs"):
            ManifestContract.from_bytes(json.dumps(payload).encode())

    def test_manifest_rejects_coupled_service_factors(self) -> None:
        root = Path(__file__).resolve().parents[1]
        path = root / "esp32s3-core-timing" / "probe-cells.json"
        payload = json.loads(path.read_text())
        payload["cells"][0]["variants"] = ["other"]
        with self.assertRaisesRegex(ValidationError, "unsupported variant"):
            ManifestContract.from_bytes(json.dumps(payload).encode())

    def test_manifest_rejects_a_missing_spi_clock_point(self) -> None:
        root = Path(__file__).resolve().parents[1]
        path = root / "esp32s3-core-timing" / "probe-cells.json"
        payload = json.loads(path.read_text())
        del payload["cells"][0]["samples"]
        with self.assertRaisesRegex(ValidationError, "missing keys"):
            ManifestContract.from_bytes(json.dumps(payload).encode())

    def test_runtime_manifest_provenance_is_pinned(self) -> None:
        base = contract()
        actual = ManifestContract(
            protocol_version=base.protocol_version,
            harness_version=base.harness_version,
            chip_model=base.chip_model,
            chip_revision=base.chip_revision,
            cells=base.cells,
            manifest_sha256="e" * 64,
        )
        with self.assertRaisesRegex(ValidationError, "committed manifest"):
            CaptureValidator(actual, "normal", "store_hit_psram").feed_line(metadata(), 1)

    def test_msync_control_accepts_exact_independent_factors(self) -> None:
        self.decomposition_validator(self.msync_cell()).feed_line(self.msync_sample(), 3)

    def test_msync_control_rejects_factor_or_clock_drift(self) -> None:
        for update in (
            {"dirtyLines": 16},
            {"psramClockHz": 80_000_000},
            {"psramClockRegister": expected_psram_clock_register(80_000_000)},
            {"psramCoreClockRegister": 0},
            {"bytes": 64},
        ):
            with self.subTest(update=update):
                with self.assertRaises(ValidationError):
                    self.decomposition_validator(self.msync_cell()).feed_line(
                        self.msync_sample(**update), 3
                    )

    def test_msync_control_requires_service_counter_evidence(self) -> None:
        with self.assertRaisesRegex(ValidationError, "exclusive PSRAM service evidence"):
            self.decomposition_validator(self.msync_cell()).feed_line(
                self.msync_sample(
                    psramServiceCounters=counters(dbusAccesses=0, dbusPsramMisses=0)
                ),
                3,
            )

    def test_decomposition_refusal_fails_closed(self) -> None:
        with self.assertRaisesRegex(ValidationError, "tier candidate affine"):
            self.decomposition_validator(self.msync_cell()).feed_line(
                line(
                    "refusal",
                    cell=self.msync_cell().id,
                    ordinal=0,
                    reason="PSRAM service clock readback mismatch",
                    tierCandidate="affine",
                ),
                3,
            )

    def test_spi2_control_accepts_separate_reconciled_phases(self) -> None:
        self.decomposition_validator(self.spi2_cell()).feed_line(self.spi2_sample(), 3)

    def test_spi2_control_rejects_missing_or_combined_phases(self) -> None:
        payload = json.loads(self.spi2_sample()[len(PREFIX) :])
        del payload["completionCycles"]
        with self.assertRaisesRegex(ValidationError, "separate SPI2 phase"):
            self.decomposition_validator(self.spi2_cell()).feed_line(
                PREFIX + json.dumps(payload), 3
            )
        with self.assertRaisesRegex(ValidationError, "do not reconcile"):
            self.decomposition_validator(self.spi2_cell()).feed_line(
                self.spi2_sample(completionCycles=84), 3
            )

    def test_spi2_control_rejects_payload_or_clock_drift(self) -> None:
        for update in ({"bytes": 64}, {"spiClockHz": 40_000_000}):
            with self.subTest(update=update):
                with self.assertRaisesRegex(ValidationError, "manifest SPI2 factors"):
                    self.decomposition_validator(self.spi2_cell()).feed_line(
                        self.spi2_sample(**update), 3
                    )

    def test_refusal_fails_immediately(self) -> None:
        validator = self.validator()
        validator.feed_line(metadata(), 1)
        validator.feed_line(
            line("cell-start", cell="store_hit_psram", expectedSamples=2), 2
        )
        with self.assertRaisesRegex(ValidationError, "refused 'store_hit_psram'"):
            validator.feed_line(
                line(
                    "refusal",
                    cell="store_hit_psram",
                    ordinal=0,
                    reason="counter mismatch",
                    tierCandidate="exact",
                ),
                3,
            )

    def test_duplicate_request_is_rejected(self) -> None:
        with self.assertRaisesRegex(ValidationError, "request contains duplicates"):
            CaptureValidator(
                contract(), "normal", "store_hit_psram,store_hit_psram"
            )

    def test_missing_sample_fails_closed(self) -> None:
        records = complete_lines()
        del records[2]
        validator = self.validator()
        for index, record in enumerate(records[:2], 1):
            validator.feed_line(record, index)
        with self.assertRaisesRegex(ValidationError, "ordinal is 1, expected 0"):
            validator.feed_line(records[2], 3)

    def test_metadata_must_match_exact_manifest_availability(self) -> None:
        validator = self.validator()
        with self.assertRaisesRegex(ValidationError, "availableCells"):
            validator.feed_line(metadata(availableCells=[]), 1)

    def test_runtime_build_must_match_preflight(self) -> None:
        expected = {
            "idfVersion": "v6.1",
            "spiramRodata": False,
            "gitCommit": "a" * 40,
            "gitDirty": False,
            "variant": "normal",
            "sdkconfigSha256": "b" * 64,
            "manifestSha256": "d" * 64,
            "compilerVersion": "15.2.0",
            "elfSha256": "d" * 64,
            "dbusFlashClassifier": classifier(),
        }
        with self.assertRaisesRegex(ValidationError, "verified ELF preflight"):
            self.validator(expected).feed_line(metadata(), 1)

    def test_runtime_requires_exact_idf_version(self) -> None:
        with self.assertRaisesRegex(ValidationError, "expected 'v6.1'"):
            self.validator().feed_line(metadata(idfVersion="v6.0.2"), 1)

    def test_runtime_rejects_spiram_rodata(self) -> None:
        with self.assertRaisesRegex(ValidationError, "spiramRodata must be false"):
            self.validator().feed_line(metadata(spiramRodata=True), 1)

    def test_classifier_reset_default_range_is_rejected(self) -> None:
        with self.assertRaisesRegex(ValidationError, "reset default"):
            self.validator().feed_line(
                metadata(dbusFlashClassifier=classifier(0, 0)), 1
            )

    def test_classifier_zero_start_range_is_rejected(self) -> None:
        with self.assertRaisesRegex(ValidationError, "reset default"):
            self.validator().feed_line(
                metadata(dbusFlashClassifier=classifier(0, 0x3FFFF)), 1
            )

    def test_classifier_wrong_range_does_not_match_preflight(self) -> None:
        expected = {
            "idfVersion": "v6.1",
            "spiramRodata": False,
            "gitCommit": "a" * 40,
            "gitDirty": False,
            "variant": "normal",
            "sdkconfigSha256": "b" * 64,
            "manifestSha256": "d" * 64,
            "compilerVersion": "15.2.0",
            "elfSha256": "c" * 64,
            "dbusFlashClassifier": classifier(),
        }
        with self.assertRaisesRegex(ValidationError, "verified ELF preflight"):
            self.validator(expected).feed_line(
                metadata(
                    dbusFlashClassifier=classifier(0x3C02AC40, 0x3C06AC3F)
                ),
                1,
            )

    def test_hot_instruction_sample_accepts_zero_accesses_and_zero_misses(self) -> None:
        self.instruction_validator("instruction_psram_hot").feed_line(
            self.instruction_sample("instruction_psram_hot", 0, 0), 3
        )

    def test_hot_instruction_sample_accepts_accesses_without_misses(self) -> None:
        self.instruction_validator("instruction_psram_hot").feed_line(
            self.instruction_sample("instruction_psram_hot", 8, 0), 3
        )

    def test_hot_instruction_sample_rejects_any_icache_miss(self) -> None:
        with self.assertRaisesRegex(ValidationError, "reports an I-cache miss"):
            self.instruction_validator("instruction_psram_hot").feed_line(
                self.instruction_sample("instruction_psram_hot", 8, 1), 3
            )

    def test_cold_instruction_sample_rejects_zero_counters(self) -> None:
        with self.assertRaisesRegex(ValidationError, "lacks instruction-cache accesses"):
            self.instruction_validator("instruction_psram_cold").feed_line(
                self.instruction_sample("instruction_psram_cold", 0, 0), 3
            )

    def test_flash_attribution_uses_isolated_counters(self) -> None:
        cell = "arbitration_psram_victim_flash_aggressor"
        validator = self.contention_validator(cell)
        with self.assertRaisesRegex(ValidationError, "isolated flash attribution"):
            validator.feed_line(
                self.contention_sample(
                    cell,
                    counters(dbusAccesses=32, dbusFlashMisses=0, dbusPsramMisses=0),
                ),
                3,
            )

    def test_cross_core_requires_isolated_psram_counters(self) -> None:
        cell = "flash_bandwidth_cross_core"
        validator = self.contention_validator(cell)
        with self.assertRaisesRegex(ValidationError, "isolated PSRAM attribution"):
            validator.feed_line(
                self.contention_sample(cell, counters(dbusAccesses=32, dbusPsramMisses=0)),
                3,
            )

    def test_internal_attribution_rejects_external_data_traffic(self) -> None:
        cell = "arbitration_psram_victim_internal_aggressor"
        for contamination in (
            {"dbusAccesses": 1},
            {"dbusFlashMisses": 1},
            {"dbusPsramMisses": 1},
        ):
            with self.subTest(contamination=contamination):
                with self.assertRaisesRegex(ValidationError, "external data-cache traffic"):
                    self.contention_validator(cell).feed_line(
                        self.contention_sample(cell, counters(**contamination)), 3
                    )

    def test_flash_attribution_rejects_psram_cross_contamination(self) -> None:
        cell = "arbitration_psram_victim_flash_aggressor"
        with self.assertRaisesRegex(ValidationError, "exclusive isolated flash"):
            self.contention_validator(cell).feed_line(
                self.contention_sample(
                    cell,
                    counters(dbusAccesses=32, dbusFlashMisses=4, dbusPsramMisses=1),
                ),
                3,
            )

    def test_psram_attribution_rejects_flash_cross_contamination(self) -> None:
        cell = "arbitration_psram_victim_psram_aggressor"
        with self.assertRaisesRegex(ValidationError, "exclusive isolated PSRAM"):
            self.contention_validator(cell).feed_line(
                self.contention_sample(
                    cell,
                    counters(dbusAccesses=32, dbusFlashMisses=1, dbusPsramMisses=4),
                ),
                3,
            )

    def test_instruction_counters_do_not_disqualify_data_attribution(self) -> None:
        cell = "arbitration_psram_victim_flash_aggressor"
        attribution = counters(
            ibusAccesses=7,
            ibusMisses=3,
            dbusAccesses=32,
            dbusFlashMisses=4,
        )
        self.contention_validator(cell).feed_line(
            self.contention_sample(cell, attribution), 3
        )

    def test_contention_cells_accept_isolated_attribution(self) -> None:
        for cell in (
            "arbitration_psram_victim_internal_aggressor",
            "arbitration_psram_victim_flash_aggressor",
            "arbitration_psram_victim_psram_aggressor",
            "psram_bandwidth_cross_core",
            "flash_bandwidth_cross_core",
        ):
            aggressor = counters(dbusAccesses=0)
            if "flash_aggressor" in cell:
                aggressor = counters(dbusAccesses=32, dbusFlashMisses=4)
            elif "psram_aggressor" in cell or cell.endswith("_cross_core"):
                aggressor = counters(dbusAccesses=32, dbusPsramMisses=4)
            with self.subTest(cell=cell):
                self.contention_validator(cell).feed_line(
                    self.contention_sample(cell, aggressor), 3
                )

    def test_isolated_attribution_checksum_is_exact(self) -> None:
        cell = "arbitration_psram_victim_flash_aggressor"
        payload = json.loads(
            self.contention_sample(cell, counters(dbusAccesses=32, dbusFlashMisses=4))[
                len(PREFIX) :
            ]
        )
        payload["isolatedAttributionChecksum"] += 1
        with self.assertRaisesRegex(ValidationError, "checksum mismatch"):
            self.contention_validator(cell).feed_line(PREFIX + json.dumps(payload), 3)

    def test_isolated_checksum_constants_match_bounded_lap(self) -> None:
        for source, checksum in ATTRIBUTION_CHECKSUMS.items():
            with self.subTest(source=source):
                self.assertEqual(
                    checksum,
                    expected_aggressor_checksum(source, ATTRIBUTION_ITERATIONS),
                )

    def test_runtime_checksum_matches_iterations_and_source(self) -> None:
        cell = "arbitration_psram_victim_psram_aggressor"
        payload = json.loads(
            self.contention_sample(cell, counters(dbusAccesses=32, dbusPsramMisses=4))[
                len(PREFIX) :
            ]
        )
        payload["aggressorChecksum"] += 1
        with self.assertRaisesRegex(ValidationError, "runtime checksum mismatch"):
            self.contention_validator(cell).feed_line(PREFIX + json.dumps(payload), 3)

    def test_refusal_preserves_isolated_attribution_diagnostics(self) -> None:
        validator = self.contention_validator(
            "arbitration_psram_victim_flash_aggressor"
        )
        with self.assertRaisesRegex(ValidationError, "refused"):
            validator.feed_line(
                line(
                    "refusal",
                    cell="arbitration_psram_victim_flash_aggressor",
                    ordinal=0,
                    reason="isolated flash attribution lacks flash access or miss counters",
                    tierCandidate="affine",
                    attributionSource="flash",
                    isolatedAttributionIterations=ATTRIBUTION_ITERATIONS,
                    isolatedAttributionChecksum=ATTRIBUTION_CHECKSUMS["flash"],
                    isolatedAttributionCounters=counters(dbusAccesses=0),
                ),
                3,
            )

    def runtime_refusal(self, **overrides: object) -> str:
        source = "psram"
        values: dict[str, object] = {
            "cell": "arbitration_psram_victim_psram_aggressor",
            "ordinal": 0,
            "reason": "contended aggressor runtime evidence failed",
            "tierCandidate": "affine",
            "attributionSource": source,
            "isolatedAttributionIterations": ATTRIBUTION_ITERATIONS,
            "isolatedAttributionChecksum": ATTRIBUTION_CHECKSUMS[source],
            "isolatedAttributionCounters": counters(
                dbusAccesses=32, dbusPsramMisses=4
            ),
            "aggressorIterations": 64,
            "aggressorChecksum": expected_aggressor_checksum(source, 64),
        }
        values.update(overrides)
        return line("refusal", **values)

    def test_runtime_refusal_accepts_paired_exact_evidence(self) -> None:
        validator = self.contention_validator(
            "arbitration_psram_victim_psram_aggressor"
        )
        with self.assertRaisesRegex(ValidationError, "refused"):
            validator.feed_line(self.runtime_refusal(), 3)

    def test_runtime_refusal_requires_paired_evidence(self) -> None:
        payload = json.loads(self.runtime_refusal()[len(PREFIX) :])
        del payload["aggressorChecksum"]
        validator = self.contention_validator(
            "arbitration_psram_victim_psram_aggressor"
        )
        with self.assertRaisesRegex(ValidationError, "must appear together"):
            validator.feed_line(PREFIX + json.dumps(payload), 3)

    def test_runtime_refusal_requires_source_derived_checksum(self) -> None:
        validator = self.contention_validator(
            "arbitration_psram_victim_psram_aggressor"
        )
        with self.assertRaisesRegex(ValidationError, "runtime checksum mismatch"):
            validator.feed_line(self.runtime_refusal(aggressorChecksum=1), 3)

    def test_post_completion_record_fails_tail(self) -> None:
        validator = self.validator()
        for index, record in enumerate(complete_lines(), 1):
            validator.feed_line(record, index)
        with self.assertRaisesRegex(ValidationError, "after run-complete"):
            validator.feed_line(metadata(), len(complete_lines()) + 1)

    def test_malformed_record_fails_on_its_line(self) -> None:
        with self.assertRaisesRegex(ValidationError, "line 1 has malformed NDJSON"):
            self.validator().feed_line(PREFIX + "{", 1)

    def test_truncated_capture_is_incomplete(self) -> None:
        validator = self.validator()
        for index, record in enumerate(complete_lines()[:-1], 1):
            validator.feed_line(record, index)
        with self.assertRaisesRegex(ValidationError, "capture is incomplete"):
            validator.finalize()

    def test_timestamped_offline_log_is_accepted(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "capture.log"
            path.write_text("".join(f"[12:00:00] {record}\n" for record in complete_lines()))
            tally, runtime = validate_path(
                path, contract(), "normal", "store_hit_psram"
            )
            self.assertTrue(tally.as_dict()["complete"])
            self.assertEqual(runtime["bootId"], "1-0123456789abcdef")

    def test_invalid_utf8_is_malformed(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "capture.log"
            path.write_bytes(b"TINYDRAW_TIER_B_NDJSON \xff\n")
            with self.assertRaisesRegex(ValidationError, "not valid UTF-8"):
                validate_path(path, contract(), "normal", "store_hit_psram")

    def test_all_refuses_a_variant_with_no_available_cells(self) -> None:
        unavailable = ManifestContract(
            protocol_version=2,
            harness_version="0.2.0-review",
            chip_model="ESP32-S3",
            chip_revision=2,
            cells=(
                CellContract(
                    "gpio21_edge", 1, ("normal",), status="open-refusal"
                ),
            ),
        )
        with self.assertRaisesRegex(ValidationError, "no available cells"):
            CaptureValidator(unavailable, "normal", "all")

    def test_store_sample_requires_paired_baseline(self) -> None:
        records = complete_lines()
        payload = json.loads(records[2][len(PREFIX) :])
        del payload["baselineCacheCounters"]
        records[2] = PREFIX + json.dumps(payload)
        validator = self.validator()
        validator.feed_line(records[0], 1)
        validator.feed_line(records[1], 2)
        with self.assertRaisesRegex(ValidationError, "baseline cycles and counters"):
            validator.feed_line(records[2], 3)

    def test_calibration_dry_run_ignores_values_and_requires_terminal(self) -> None:
        validator = CalibrationValidator(contract(), "normal", "store_hit_psram", True)
        validator.feed_line(
            'CAL_RECORD {"type":"metric","name":"store_hit_psram",'
            '"ccount_samples":["ignored"]}',
            1,
        )
        validator.feed_line(CAL_DONE, 2)
        tally = validator.finalize()
        self.assertTrue(tally.as_dict()["complete"])
        self.assertEqual(tally.console_lines, 2)

    def test_calibration_strict_mode_checks_sample_count(self) -> None:
        validator = CalibrationValidator(contract(), "normal", "store_hit_psram")
        with self.assertRaisesRegex(ValidationError, "expected 2"):
            validator.feed_line(
                'CAL_RECORD {"type":"metric","name":"store_hit_psram",'
                '"ccount_samples":[17]}',
                1,
            )

    def test_calibration_dry_run_allows_only_cache_counter_refusal(self) -> None:
        accepted = CalibrationValidator(contract(), "normal", "store_hit_psram", True)
        accepted.feed_line(
            'CAL_RECORD {"type":"refusal","name":"store_hit_psram",'
            '"reason":"cache-counter mismatch in emulator"}',
            1,
        )
        accepted.feed_line(CAL_DONE, 2)
        self.assertTrue(accepted.finalize().as_dict()["complete"])
        rejected = CalibrationValidator(contract(), "normal", "store_hit_psram", True)
        with self.assertRaisesRegex(ValidationError, "not a cache-counter mismatch"):
            rejected.feed_line(
                'CAL_RECORD {"type":"refusal","name":"store_hit_psram",'
                '"reason":"DMA did not finish"}',
                1,
            )

    def test_calibration_dry_run_rejects_missing_terminal(self) -> None:
        validator = CalibrationValidator(contract(), "normal", "store_hit_psram", True)
        validator.feed_line(
            'CAL_RECORD {"type":"metric","name":"store_hit_psram",'
            '"ccount_samples":[1]}',
            1,
        )
        with self.assertRaisesRegex(ValidationError, CAL_DONE):
            validator.finalize()

    def test_calibration_dry_run_rejects_malformed_record(self) -> None:
        validator = CalibrationValidator(contract(), "normal", "store_hit_psram", True)
        with self.assertRaisesRegex(ValidationError, "malformed CAL_RECORD"):
            validator.feed_line("CAL_RECORD {", 1)

    def test_console_line_contract_parses_microsecond_counters(self) -> None:
        validator = CalibrationValidator(console_contract(), "normal", "all", True)
        lines = [
            "[00:00:01] TINYDRAW_LIVE_PRESENT kind=startup compose_us=11 transfer_wait_us=23",
            "TINYDRAW_LIVE_PRESENT kind=gate compose_us=7 transfer_wait_us=19",
            "TINYDRAW_LIVE_STRESS total_us=101 maximum_us=13",
            "TINYDRAW_GATE1_AUTOMATED_DONE pass=1",
        ]
        for line_number, value in enumerate(lines, 1):
            validator.feed_line(value, line_number)
        tally = validator.finalize()
        self.assertTrue(tally.as_dict()["complete"])
        self.assertEqual(
            tally.as_dict()["consoleCounters"][0],
            {
                "line": "TINYDRAW_LIVE_PRESENT",
                "compose_us": 11,
                "transfer_wait_us": 23,
            },
        )

    def test_console_line_contract_requires_every_counter(self) -> None:
        validator = CalibrationValidator(console_contract(), "normal", "all", True)
        with self.assertRaisesRegex(ValidationError, "missing transfer_wait_us"):
            validator.feed_line("TINYDRAW_LIVE_PRESENT compose_us=11", 1)

    def test_console_line_contract_rejects_wrong_count_in_dry_run(self) -> None:
        validator = CalibrationValidator(console_contract(), "normal", "all", True)
        validator.feed_line(
            "TINYDRAW_LIVE_PRESENT compose_us=11 transfer_wait_us=23", 1
        )
        validator.feed_line("TINYDRAW_LIVE_STRESS total_us=101 maximum_us=13", 2)
        validator.feed_line("TINYDRAW_GATE1_AUTOMATED_DONE", 3)
        with self.assertRaisesRegex(ValidationError, "2 samples"):
            validator.finalize()

    def test_manifest_console_line_contract_requires_its_family(self) -> None:
        payload = manifest_payload()
        payload["terminalLine"] = "TINYDRAW_GATE1_AUTOMATED_DONE"
        payload["cells"][0]["consoleLine"] = "TINYDRAW_LIVE_PRESENT"
        payload["cells"][0]["microsecondFields"] = ["compose_us"]
        with self.assertRaisesRegex(ValidationError, "requires family 'console-line'"):
            ManifestContract.from_bytes(json.dumps(payload).encode())


if __name__ == "__main__":
    unittest.main()
