#!/usr/bin/env python3

import sys
import tempfile
import types
import unittest
from pathlib import Path
from unittest import mock

sys.path.insert(0, str(Path(__file__).resolve().parent))

import capture


class FakeSerial:
    def __init__(self) -> None:
        self.dtr = True
        self._rts = False
        self.events: list[str] = []
        self.chunks = [b"stale partial line\n"]

    @property
    def rts(self) -> bool:
        return self._rts

    @rts.setter
    def rts(self, value: bool) -> None:
        self._rts = value
        self.events.append(f"rts={value}")
        if not value:
            self.chunks.append(b"first post-reset\r\nTERMINAL\r\n")

    def reset_input_buffer(self) -> None:
        if not self.rts:
            raise AssertionError("serial input was drained after reset release")
        self.events.append("reset_input_buffer")
        self.chunks.clear()

    def read(self, _size: int) -> bytes:
        return self.chunks.pop(0) if self.chunks else b""

    def close(self) -> None:
        self.events.append("close")


class CaptureBootTests(unittest.TestCase):
    def test_stale_serial_bytes_are_drained_while_reset_is_held(self) -> None:
        device = FakeSerial()
        serial_module = types.SimpleNamespace(Serial=lambda *_args, **_kwargs: device)
        clock = iter(index * 0.25 for index in range(100))

        with tempfile.TemporaryDirectory() as temporary:
            output = Path(temporary) / "boot.log"
            with (
                mock.patch.dict(sys.modules, {"serial": serial_module}),
                mock.patch.object(capture.time, "sleep"),
                mock.patch.object(capture.time, "monotonic", side_effect=lambda: next(clock)),
            ):
                lines = capture._capture_boot("fake-port", output, 10.0, "TERMINAL")

            self.assertEqual(lines, ["first post-reset", "TERMINAL"])
            self.assertEqual(output.read_text(), "first post-reset\nTERMINAL\n")
            self.assertEqual(
                device.events[:3], ["rts=True", "reset_input_buffer", "rts=False"]
            )


if __name__ == "__main__":
    unittest.main()
