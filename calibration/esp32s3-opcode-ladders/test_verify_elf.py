from __future__ import annotations

import subprocess
import sys
import tempfile
import unittest
from pathlib import Path


SCRIPT = Path(__file__).with_name("verify_elf.py")


class VerifyElfCliTest(unittest.TestCase):
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
