from __future__ import annotations

import tempfile
import unittest
from pathlib import Path

import audit_fixture


class AuditFixtureTest(unittest.TestCase):
    def test_reports_matching_elf_and_deduplicates_roots(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            elf = root / "build/tinydraw_esp32.elf"
            elf.parent.mkdir()
            elf.write_bytes(b"fixture")
            expected = audit_fixture.sha256(elf)

            result = audit_fixture.audit([root, root], expected)

            self.assertEqual(result["searchedUniqueRoots"], [str(root.resolve())])
            self.assertEqual(result["candidateElfFiles"], 1)
            self.assertEqual(result["matchingContractElfFiles"], 1)
            self.assertEqual(result["matchingPaths"], [str(elf.resolve())])

    def test_records_missing_root_without_a_match(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            missing = Path(directory) / "missing"

            result = audit_fixture.audit([missing])

            self.assertEqual(result["searchedUniqueRoots"], [])
            self.assertEqual(result["missingRoots"], [str(missing)])
            self.assertEqual(result["matchingContractElfFiles"], 0)


if __name__ == "__main__":
    unittest.main()
