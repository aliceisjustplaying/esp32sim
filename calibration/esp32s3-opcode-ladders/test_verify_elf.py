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
        "40370000: 03ea40 rsr.ccount a4\n"
        f"{load}"
        "40370006: 0008e0 callx8 a8\n"
        "40370009: 03ea90 rsr.ccount a9\n"
        "4037000c <next_function>:\n"
    )


class VerifyElfCliTest(unittest.TestCase):
    def test_measurement_window_is_iram_and_has_no_drom_load(self) -> None:
        result = MODULE.verify_measurement_window(measurement_disassembly())
        self.assertEqual(result["dromDescriptorLoads"], 0)

    def test_measurement_window_rejects_descriptor_table_load(self) -> None:
        with self.assertRaisesRegex(
            MODULE.VerificationError, "flash descriptor table"
        ):
            MODULE.verify_measurement_window(measurement_disassembly(drom_load=True))

    def test_measurement_window_rejects_flash_code(self) -> None:
        with self.assertRaisesRegex(MODULE.VerificationError, "outside internal IRAM"):
            MODULE.verify_measurement_window(measurement_disassembly(0x42000000))

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
