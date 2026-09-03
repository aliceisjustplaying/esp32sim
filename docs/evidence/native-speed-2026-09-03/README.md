# Native TinyDraw READY baseline, 2026-09-03

The pinned TinyDraw V2 product reached `TINYDRAW_VECTOR_V2_READY` in all five
native release/JIT runs. Each run used the same 200,000,000-cycle scheduling
horizon, equal to 0.833 emulated seconds. The median process wall time was
0.55 seconds. In this committed sample, every run completed the
READY-qualified horizon faster than real time on this host.

| Run | Process wall seconds | Retired instructions | Reported MIPS | JIT blocks | READY |
| ---: | ---: | ---: | ---: | ---: | --- |
| 1 | 0.61 | 52,605,596 | 99.8 | 34,072 | yes |
| 2 | 0.55 | 52,605,596 | 97.7 | 34,072 | yes |
| 3 | 0.54 | 52,605,596 | 100.4 | 34,072 | yes |
| 4 | 0.55 | 52,605,596 | 98.0 | 34,072 | yes |
| 5 | 0.55 | 52,605,596 | 98.8 | 34,072 | yes |

The five console transcripts are byte-identical. Architectural counts, JIT
block counts, generated-code size, exceptions, interrupts, and modeled cycles
also agree across every run. The median reported throughput is 98.8 MIPS.
The fixed horizon covers 0.833 emulated seconds in 0.55 wall seconds at the
median, or about 1.5 times real time.

This is a fixed-horizon native throughput receipt that requires READY before
the horizon ends. It is not an exact READY-latency measurement because the
fast CLI stops at the horizon, not at the console marker. It is not measured
mode and adopts no timing price.

Host load was not controlled. Wall-time values are observations of this
five-run sample, not a sustained-throughput guarantee.

The runner verifies every product artifact and the ROM ELF before building
and running. It requires the immutable product directory named by
`TINYDRAW_VECTOR_V2_BUILD` and never reads the TinyDraw source checkout. The
product pin is described in
[`../tinydraw-vector-v2-build-2026-09-03.json`](../tinydraw-vector-v2-build-2026-09-03.json).

Run from the esp32sim repository root:

```text
TINYDRAW_VECTOR_V2_BUILD=/Users/sarah/Archives/esp32s3/pinned-builds/tinydraw-vector-v2-9cb651e0 \
ESP32S3_ROM_ELF=/Users/sarah/.espressif/tools/esp-rom-elfs/20241011/esp32s3_rev0_rom.elf \
bash docs/evidence/native-speed-2026-09-03/run.sh /tmp/native-ready-rerun
```

Pass a new empty output directory as the script's first argument. The committed
`raw/` directory is the original receipt and the runner will not overwrite it.

[`result.json`](result.json) records the repository and binary hashes, host,
toolchain, command contract, per-run values, and raw-file hashes. The ten raw
receipts are in [`raw/`](raw/).

Before commit, both values from TinyDraw's ignored local Wi-Fi credential file
were compared byte-for-byte against this evidence directory. Neither value is
present. The evidence directory contains no copy of a pinned product artifact.
