# Mask-ROM fetch ladders, 2026-09-06 (evidence only, no price adopted)

Ten straight-line mask-ROM bodies (`entry ... retw.n`, 5 to 30 bytes) timed through
one driver against byte-identical IRAM twins at the same address modulo 128; the
timed region is `rsr.ccount; movi.n; movi; callx8; rsr.ccount`, verified from the
ELF. Board: Waveshare ESP32-S3-Touch-AMOLED-1.8 V2, ESP-IDF v6.1. Sources and
verifier: `calibration/esp32s3-rom-fetch-ladder{,-v2}`. Raw captures, flashed
binaries and checksums: `~/Archives/esp32s3/esp32s3-rom-fetch-ladder*-20260906-*`.
Compact values: `result.json`.

- v1 (two boots): nine of ten pairs identical in ROM and IRAM in every sample;
  `rom_clzdi2` a distribution (16 in 96 samples, 17 to 19 in four) while its twin
  was 16 throughout. Pre-declared rule: no model accepted.
- v2 (four variants, two boots each, boot identities, per-cell statistics):
  dual-idle and unicore show every cell constant and ROM equal to IRAM for all ten
  bodies at three positions; both a ROM-target and an IRAM-target core-1 hammer
  slow and scatter both ROM and IRAM cells (IRAM more). Pre-declared rule: not
  attributed. The v1 image, rerun in the same session, reproduced its variation.
- Scope: whole-call ROM-versus-IRAM equality for windowed-call bodies with core 1
  quiet. Not a per-instruction fetch price, not a branch or literal price, not the
  reset path (whose first instruction is a taken ROM jump). Limits and the
  capture-integrity notes (reset method, one preamble anomaly, one code-identical
  rebuild flashed in place of the archived file) are in the two `RESULT.md` files.
- Related: `calibration/esp32s3-rom-reset-path/` (untimed inventory of the ROM
  reset path, 130,317 instructions to bootloader entry, instruction counts only).
