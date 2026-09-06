# ROM fetch ladder: capture result, 2026-09-06

Status: evidence only. No price is adopted. The pre-declared analysis rule
(README, "Held-out validation") returns no accepted model because one fitting
pair is not constant; the raw per-pair results are reported below without
fitting.

## Capture

- Board: Waveshare ESP32-S3-Touch-AMOLED-1.8 V2, ESP32-S3 QFN56 rev v0.2, 240 MHz,
  ESP-IDF v6.1, xtensa-esp-elf 15.2.0. Image `esp32s3_rom_fetch_ladder_calibration`
  (app ELF SHA-256 `476116ea...`, mask ROM ELF `c0ce0f33...`).
- Archive: `~/Archives/esp32s3/esp32s3-rom-fetch-ladder-20260906-184940/`
  (`session.json`, `boot-1.log`, `boot-2.log`, `elf-verification.json`, flashed
  binaries, `SHA256SUMS`). Session 17:49:40 to 17:49:59 UTC, start and finish
  notifications sent.
- Both boots: `boot:0x2b (SPI_FAST_FLASH_BOOT)`, 20 cells, 2,000 accepted samples,
  zero refusals, all ten `ROM_TWIN` word-for-word checks passed, `CALIBRATION_DONE`.
- Reset method deviation: every reset driven through the USB serial/JTAG control
  lines (pyserial DTR/RTS in any order, and esptool's own hard reset) latched the
  boot strap low on this board today and entered ROM download mode
  (`boot:0x23`, strap register `0x23`, GPIO0 pin high in the stub, force-download
  register clear). The RTC watchdog reset (`esptool --after watchdog-reset`) boots
  normally, so `calibration/tools/capture.py` was changed to reset that way and to
  reopen the re-enumerated port with both control lines deasserted. The firmware
  gained a one-second host-attach delay before its first record. The 2026-09-04
  H1 session on the same board booted normally through the DTR/RTS sequence; the
  cause of the change is not established.
- The two boot logs are byte-identical (same SHA-256), including the outlier
  positions below. The firmware is deterministic and interrupts are masked on
  core 0 during cells, so identical output is possible; it also means the two
  boots do not demonstrate independence of the disturbance.

## Per-pair results (cycles per timed call, all 100 samples per cell)

| pair | words | ROM boot 1 | IRAM boot 1 | ROM boot 2 | IRAM boot 2 | D = ROM - IRAM |
|---|---:|---|---|---|---|---|
| p_none | 2 | 10 | 10 | 10 | 10 | 0 |
| abs | 2 | 11 | 11 | 11 | 11 | 0 |
| mulsi3 | 2 | 11 | 11 | 11 | 11 | 0 |
| clzsi2 | 2 | 11 | 11 | 11 | 11 | 0 |
| temp_to_power | 3 | 12 | 12 | 12 | 12 | 0 |
| roundup2 | 4 | 14 | 14 | 14 | 14 | 0 |
| ctzsi2 | 5 | 15 | 15 | 15 | 15 | 0 |
| ffssi2 | 5 | 15 | 15 | 15 | 15 | 0 |
| clzdi2 | 5 | 16 (96), 17 (2), 18 (1), 19 (1) | 16 | same | 16 | not constant |
| ctzdi2 | 8 | 19 | 19 | 19 | 19 | 0 |

"Words" is the count of 32-bit-aligned words the body spans. The `rom_clzdi2`
outliers sit at sample indices 40, 41, 44 and 46 in both boots (values 17, 18,
19, 17); the first and last samples are all 16, and the IRAM twin is 16 in every
sample. The body is 20 bytes at ROM `0x4005636c` (44 mod 64), ending at
`0x4005637f`, one byte before a 64-byte boundary.

## What this supports and what it does not

- Nine of ten matched windowed-call bodies cost exactly the same number of cycles
  when fetched from mask ROM as from IRAM, in both boots, with every sample
  identical. This is the paired-equality candidate (README model 1) for those
  nine bodies. Under the pre-declared rule it is not an accepted model, because
  the fitting set includes `clzdi2`, whose ROM cell is a distribution.
- The `clzdi2` ROM outliers sit at the same positions in both logs and are absent
  from its IRAM twin. That is consistent with a disturbance specific to the ROM
  cell, but cell timing and order within the run, and the capture-identity
  limitation above, remain confounds, so it is not attributed. Candidates, none
  established: shared ROM access by core 1 (its idle task and tick handler were
  running; interrupts were masked only on core 0), or a prefetch past the body's
  end across the 64-byte boundary at `0x40056380`.
- Nothing here prices instruction fetch in general or the reset-vector path. Scope
  stays as stated in the README; boot pricing remains blocked.

## Proposed follow-up (not run)

1. Rerun the identical image with core 1 held out of the picture
   (`CONFIG_FREERTOS_UNICORE=y`, chip still reports two cores) as a second variant.
   If `rom_clzdi2` becomes constant, that is consistent with a core-1-related
   effect; attribution would still require matched controls (same cell order and
   timing, a boot identity in the log, and a repeat with core 1 running).
2. Add a second `clzdi2`-sized body that does not end adjacent to a 64-byte
   boundary, and one that crosses it, to separate the boundary hypothesis.
3. Only then rerun the pre-declared fit on fresh boots; do not refit these logs.
