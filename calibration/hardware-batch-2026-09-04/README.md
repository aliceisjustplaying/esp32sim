# Three-image hardware batch

`session.json` binds the next board session to one H1 exception-ladder image
and the paired TinyDraw 80 MHz and 40 MHz frame-trace images. Each image gets
two clean captured boots. The final 40 MHz image remains on the board because
this measurement task has no product-restore stage.

Verify every bundle and the paired configuration before connecting the board:

```sh
python3 calibration/hardware-batch-2026-09-04/verify_session.py
FRAME_CORRELATION_TOOL=tools/frame_correlation.py \
  calibration/hardware-batch-2026-09-04/rehearse.sh
```

The rehearsal verifies H1, boots both archived TinyDraw images in esp32sim,
normalizes their complete console logs with `tools/frame_correlation.py`, and
runs the paired candidate analysis. `FRAME_CORRELATION_TOOL` may point at the
frame-correlation feature worktree until that tool is merged.
The image headers and esptool write arguments use DIO. The TinyDraw bootloader
then enables the configured QIO runtime mode. Verification requires both sides
of that transition and the rehearsal requires the runtime QIO boot messages.

The session is three flashes and six captured boots. The clean path is 15 to
20 minutes; reserve 30 minutes for USB re-enumeration, receipt checks, and one
retry. The paired frame result is a distribution or affine candidate. PSRAM
remains an unknown component, so this batch cannot support an exact non-PSRAM
total or the 1 percent frame claim.

The board session starts only after the maintainer connects the named Waveshare
board by a data-capable USB cable and confirms that no serial monitor owns its
port. Flash H1 first, then 80 MHz, then 40 MHz, directly from the bound bundles
with esptool under IDF 6.1. Capture two reset-to-terminal boots after each
flash. Retain the complete logs and normalized TinyDraw NDJSON in a new
`~/Archives/esp32s3/` session directory before analyzing or changing images.

Run the complete physical session once the frame-correlation tool is merged:

```sh
/Users/sarah/.espressif/tools/python/v6.1/venv/bin/python \
  calibration/hardware-batch-2026-09-04/capture_session.py \
  --port /dev/cu.usbmodem101
```

Until then, pass its isolated worktree path with `--frame-tool`. The runner
verifies every pin before creating an archive, then flashes exact bytes without
rebuilding. It writes each raw serial chunk directly to disk, validates both
boots before changing images, normalizes all four TinyDraw logs, produces two
paired candidate reports, and records failure state without continuing.
