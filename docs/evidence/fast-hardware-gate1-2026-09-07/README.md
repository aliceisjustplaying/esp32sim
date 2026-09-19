# Frozen browser battery running on hardware

The exact TinyDraw firmware selected by `work/perf-pie-confirm/candidate/assets.json`
completed all **36 automated gates** and entered the interactive app on the
connected Waveshare ESP32-S3 Touch AMOLED 1.8 V2. The final receipt is
`ssaa_receipt=yellow`, matching the browser verdict contract. See the final
lines of [raw serial](boot-1.log) and [parsed results](summary-1.json).

The host observed **78.703252875 seconds** from the startup presentation log to
the final automated verdict. This brackets the battery, including serial
printing and USB buffering; it is not a firmware whole-battery timer and does
not have exactly the browser benchmark's start/end boundaries. The summary
retains every firmware timing record and host arrival timestamps so individual
operations can be compared without treating this duration as a precise ratio.

Selected firmware measurements:

| Operation | Hardware time |
| --- | --- |
| Native panel staging, linear PIE/scalar | 12,640 / 68,151 µs |
| Native panel staging, ring PIE/scalar | 165,815 / 302,845 µs |
| Fixed seed-7 document load, 1000 operations | 558,052 µs |
| SVG + PNG export gate | 9,606,592 µs |
| Display TE at interactive readiness | period 16,821 µs, high 579 µs |

The app image has a valid esptool checksum and validation hash. Its embedded
ELF hash matches the supplied ELF exactly:
`d8e9be336ff6b12cd84580c69fc182ccfee8da7a3cc3813571ba6c25013a850e`.
See [image metadata](image-info.txt) and [asset hashes](asset-sha256.txt).
The frozen image reports IDF v6.0.2 and app version `7a157d4`.

**Initial drawing state differs from the browser's erased flash baseline.**
The board restored 38 operations at generation 1017. Its NVS, PHY and coredump
partitions were erased, but drawing and export partitions contained data:
[non-content partition inspection](original-partition-state.json). Full erase
was rejected by automatic approval review, so only the three firmware image
ranges were written. No erase workaround was used. The original immutable
16 MiB backup remains available privately to seed a matching emulator run.

Exact logical flash reconstruction before this run:

1. Start with `work/hardware-private/original-flash-16MiB.bin` from the main checkout.
2. Fill inclusive ranges `0x00000000..0x00005fff`, `0x00008000..0x00008fff`
   and `0x00010000..0x000c1fff` with `0xff`, matching the actual sector erases.
3. Overlay frozen `bootloader.bin` at `0x0` (22,496 bytes), `ptable.bin` at
   `0x8000` (3072 bytes) and `app.bin` at `0x10000` (726,752 bytes).

The preceding Tier-B probe write was smaller and wholly covered by those
ranges. It did not use the drawing/export partitions. Each write reported
successful hash verification. The board then booted through esptool's watchdog
reset and serial was opened with DTR/RTS deasserted. There was one gate boot;
the board remains in the interactive benchmark firmware pending further work.
The original backup is not modified or included in Git.

[Capture script](../../../tools/fast-hardware/capture-gate.py) records raw serial
and [host timestamps](boot-1.timestamps.jsonl). It is run through `uv` using the
isolated hardware virtual environment. [Summary script](../../../tools/fast-hardware/summarize-gate.py)
extracts timers, checks the 36 final fields and retains the initial restore state.
