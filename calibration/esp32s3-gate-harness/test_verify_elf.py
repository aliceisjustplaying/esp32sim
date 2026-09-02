#!/usr/bin/env python3

import sys
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parent))

import verify_elf


class VerifyElfTest(unittest.TestCase):
    def test_rejects_unpinned_elf(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            elf = root / "tinydraw_esp32.elf"
            elf.write_bytes(b"not the gate harness")
            (root / "sdkconfig").write_text("\n".join(verify_elf.REQUIRED_CONFIG) + "\n")
            with self.assertRaisesRegex(ValueError, "ELF SHA-256"):
                verify_elf.verify(elf)

    def test_pinned_hashes_are_lowercase_sha256(self) -> None:
        for value in (verify_elf.ELF_SHA256, verify_elf.SDKCONFIG_SHA256):
            self.assertEqual(len(value), 64)
            self.assertEqual(value, value.lower())
            int(value, 16)

    def test_accepts_matching_elf_and_configuration(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            elf = root / "tinydraw_esp32.elf"
            elf.write_bytes(b"pinned gate harness")
            sdkconfig = root / "sdkconfig"
            sdkconfig.write_text("\n".join(verify_elf.REQUIRED_CONFIG) + "\n")
            with (
                patch.object(verify_elf, "ELF_SHA256", verify_elf.sha256(elf)),
                patch.object(verify_elf, "SDKCONFIG_SHA256", verify_elf.sha256(sdkconfig)),
            ):
                self.assertTrue(verify_elf.verify(elf)["ok"])


if __name__ == "__main__":
    unittest.main()
