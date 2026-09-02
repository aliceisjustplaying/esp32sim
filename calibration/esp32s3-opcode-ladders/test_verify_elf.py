from __future__ import annotations

import importlib.util
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path


SCRIPT = Path(__file__).with_name("verify_elf.py")
SPEC = importlib.util.spec_from_file_location("opcode_ladders_verify_elf", SCRIPT)
assert SPEC is not None and SPEC.loader is not None
MODULE = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = MODULE
SPEC.loader.exec_module(MODULE)


def measurement_disassembly(address: int = 0x40370000, drom_load: bool = False) -> str:
    load = (
        "40370003: 000081 l32r a8, 40370004 "
        "(3c0168c8 <probes>)\n"
        if drom_load
        else ""
    )
    return (
        f"{address:08x} <measure_probe_samples>:\n"
        "40370000: 0002e0 callx8 a2\n"
        "40370003: 03ea40 rsr.ccount a4\n"
        f"{load}"
        "40370006: 0008e0 callx8 a8\n"
        "40370009: 03ea90 rsr.ccount a9\n"
        "4037000c: 0888 l32i.n a8, a8, 0\n"
        "4037000e: 0ab8 l32i.n a11, a10, 0\n"
        "40370010: 0aa8 l32i.n a10, a10, 0\n"
        "40370012: 0bb8 l32i.n a11, a11, 0\n"
        "40370014: 0aa8 l32i.n a10, a10, 0\n"
        "40370016: 2088b0 or a8, a8, a11\n"
        "40370019: 2088a0 or a8, a8, a10\n"
        "4037001c: 2088b0 or a8, a8, a11\n"
        "4037001f: 2088a0 or a8, a8, a10\n"
        "40370022: 38afa2 movi a10, -200\n"
        "40370025: a7aa add.n a10, a7, a10\n"
        "40370027: c09960 sub a9, a9, a6\n"
        "4037002a: c8cc bnez.n a8, 40370038 <measure_probe_samples+0x38>\n"
        "4037002c: a98c beqz.n a9, 40370038 <measure_probe_samples+0x38>\n"
        "4037002e: 0899 s32i.n a9, a8, 0\n"
        "40370030: 331b addi.n a3, a3, 1\n"
        "40370038: 9cc382 addi a8, a3, -100\n"
        "4037003b: 1a8c beqz.n a10, 40370040 <measure_probe_samples+0x40>\n"
        "4037003d: f8c856 bnez a8, 40370003 <measure_probe_samples+0x3>\n"
        "40370040: f01d retw.n\n"
        "40370042 <next_function>:\n"
    )


class VerifyElfCliTest(unittest.TestCase):
    def test_measurement_window_is_iram_and_has_no_drom_load(self) -> None:
        result = MODULE.verify_measurement_window(measurement_disassembly())
        self.assertEqual(result["dromDescriptorLoads"], 0)
        self.assertEqual(result["acceptedSamplesRequired"], 100)
        self.assertEqual(result["maxAttempts"], 200)
        self.assertIs(result["dirtySamplesDiscarded"], True)

    def test_measurement_window_rejects_descriptor_table_load(self) -> None:
        with self.assertRaisesRegex(
            MODULE.VerificationError, "flash descriptor table"
        ):
            MODULE.verify_measurement_window(measurement_disassembly(drom_load=True))

    def test_measurement_window_rejects_flash_code(self) -> None:
        with self.assertRaisesRegex(MODULE.VerificationError, "outside internal IRAM"):
            MODULE.verify_measurement_window(measurement_disassembly(0x42000000))

    def test_measurement_window_rejects_changed_retry_bound(self) -> None:
        broken = measurement_disassembly().replace("movi a10, -200", "movi a10, -199")
        with self.assertRaisesRegex(MODULE.VerificationError, "100-of-200"):
            MODULE.verify_measurement_window(broken)

    def test_measurement_window_rejects_detached_retry_bound(self) -> None:
        broken = measurement_disassembly().replace(
            "beqz.n a10, 40370040", "beqz.n a11, 40370040"
        )
        with self.assertRaisesRegex(MODULE.VerificationError, "control the retry loop"):
            MODULE.verify_measurement_window(broken)

    def test_measurement_window_rejects_dirty_sample_store(self) -> None:
        broken = measurement_disassembly().replace("40370038 <", "4037002e <")
        with self.assertRaisesRegex(MODULE.VerificationError, "accepted-sample path"):
            MODULE.verify_measurement_window(broken)

    def test_measurement_window_rejects_missing_counter_fold(self) -> None:
        broken = measurement_disassembly().replace("2088a0 or", "2088a0 add", 1)
        with self.assertRaisesRegex(MODULE.VerificationError, "not folded"):
            MODULE.verify_measurement_window(broken)

    def test_capture_objdump_argument_is_accepted(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            result = root / "verification.json"
            process = subprocess.run(
                [
                    sys.executable,
                    str(SCRIPT),
                    str(root / "missing.elf"),
                    str(result),
                    "--objdump",
                    "/capture/toolchain/xtensa-esp32s3-elf-objdump",
                ],
                text=True,
                capture_output=True,
                check=False,
            )

            self.assertEqual(process.returncode, 2)
            self.assertIn("ELF verification failed:", process.stderr)
            self.assertNotIn("unrecognized arguments", process.stderr)
            self.assertFalse(result.exists())


if __name__ == "__main__":
    unittest.main()
