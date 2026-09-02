# Derived mask-ROM fetch candidate

This analysis reads the pinned ESP32-S3 rev 0 ROM ELF and the four committed
IDF 6.1 single-core receipt archives. It verifies the exact `memset` symbol
bytes, enumerates the aligned zero-length and 0x52e0-byte paths, and subtracts
all adopted issue, branch, loop, store, alignment, dependency, and
`callx8`/`retw` terms.

The zero-length cell leaves -3.5 cycles over 16 ROM fetches. The 0x52e0 cell
leaves -5.5 cycles over 6,646 ROM fetches. These are different, noninteger,
negative candidates, so R8 forbids adopting a mask-ROM fetch price. The paths
remain within the ROM ELF `.text` region from `0x400570c8` to `0x40057112`.

Reproduce with:

```text
ESP32S3_ROM_ELF=/path/to/esp32s3_rev0_rom.elf python3 analyze.py
```

The JSON is canonical `json.dumps(..., indent=2, sort_keys=True)` output. H1
must add a straight-line mask-ROM fetch cell before this row can be revisited.
