# BOARD normal TinyDraw V2 validation receipt

Date: 2026-09-01

Owner: lane BOARD cleanup and coordinator verification

Board: Waveshare ESP32-S3-Touch-AMOLED-1.8 V2

Serial port: `/dev/cu.usbmodem101`

## Result

The same normal TinyDraw V2 product source passed on physical hardware and in
the maintained esp32sim Waveshare V2 board model. This was the product build,
not the battery gate harness, a probe, or an emulator-only firmware variant.

The physical device reached `TINYDRAW_VECTOR_V2_READY`, completed
`TINYDRAW_LIVE_SETTLE` with zero failures, and then ran for 35 seconds without
a watchdog, panic, abort, Guru Meditation, stack overflow, or product failure.

The emulator reached the same ready marker and committed a paced seven-sample
stroke:

```text
TINYDRAW_LIVE_STROKE ... operations=1 samples=7 ... presentation_failures=0 ... touch_errors=0 touch_overflows=0 touch_resyncs=0 ... touch_down=1 touch_up=1 ...
TINYDRAW_LIVE_STROKE_DONE committed=1 refresh=1 commit_failed=0
TinyDraw V2 smoke test passed
```

The browser was also checked manually with a paced multi-point drag. The
canvas showed a long diagonal blue stroke and its overview representation.
The earlier single-dot screenshot came from an inadequately paced synthetic
gesture and is not acceptance evidence.

## Pins

- esp32sim repository: `aliceisjustplaying/esp32sim`
- esp32sim branch: `board/tinydraw-v2-maintained`
- esp32sim commit: `b7c9b87f6994b163e40c1deb23bd70a00a8f76ff`
- TinyDraw repository: `aliceisjustplaying/tinydraw`
- TinyDraw branch: `maintenance/idf61-probes`
- TinyDraw commit: `2643aa7f7b3097300d3cfb002bf7432e299a6d95`
- TinyDraw pull request: <https://github.com/aliceisjustplaying/tinydraw/pull/4>
- ESP-IDF: `v6.1.0`
- emulator: release build with JIT enabled

The TinyDraw branch contains the four requested IDF 6.1 probe commits plus
three cleanup commits: the current ESP-IDF touch API, full-tree formatting,
and removal of duplicate core linkage.

## Commands

From the maintained esp32sim checkout:

```text
./scripts/tinydraw-v2.sh run /path/to/tinydraw
./scripts/tinydraw-v2.sh smoke /path/to/tinydraw
./scripts/tinydraw-v2.sh flash /path/to/tinydraw /dev/cu.usbmodem101
```

`run` builds both repositories and opens the normal product in a browser.
`smoke` builds both, injects the paced stroke, and fails closed on missing
markers or crash output. `flash` builds and flashes that normal product.

## Verification

TinyDraw:

```text
./scripts/dev format-check
./scripts/dev test       # 31 passed
./scripts/dev release    # 31 passed
./scripts/dev asan       # 13 passed
./scripts/esp32 clean
./scripts/esp32 build
```

All host logs contain zero warnings. The clean normal-product build contains
zero compiler or deprecation warnings. ESP-IDF 6.1 itself emits five CMake
component-ownership warnings for the cyclic private include relationship
between its `esp_wifi` and `wpa_supplicant` components. Those warnings come
from the installed SDK, not from TinyDraw source.

esp32sim:

```text
./scripts/pre-commit.sh
./scripts/tinydraw-v2.sh smoke /path/to/tinydraw
```

The full esp32sim gate passed with zero compiler, Clippy, rustdoc,
unfulfilled-expectation, or undocumented lint-exception diagnostics. Its
debug and release suites each pass all 17 tests on the maintained BOARD head.

## Artifact hashes

```text
9bbd36034172341da8c2f8bd1cc8c27e245a0db1ce54d1d79fa543aee9204d48  tinydraw_esp32.bin
a93287b561560cc2a4082a4e05840ad3b2d5b099faf34dc350acc1b6d763359c  tinydraw_esp32.elf
9ff6f0daec660c3055dca03a00d773087425a0aeeb9386937717f0a052efa5b1  tinydraw-v2-final-smoke.log
09b4c4afec1d693fa2b2298daf8431865ed1fb1047f3c010f62041b167a4d1a1  tinydraw-final-flash.log
d3be35bbee6df7f508a3dafe19549a9650c1bd8234f9ba07598dbcfae1e418b7  tinydraw-final-serial.log
729c1bc5e06f2b6496ada9dd26f8ec7e9c7a8a1439cfe56811dc23feeebabeb2  tinydraw-final-idf-build.log
```

The raw logs and firmware artifacts are machine-local. The hashes bind this
receipt to the exact files used for the final checks.

## Limits

The GPIO13 TE model is still an approximate compatibility signal. This receipt
does not adopt a hardware cadence, phase, scan-out, or tearing-accuracy claim.
Those claims require the planned hash-pinned logic-analyzer capture.
