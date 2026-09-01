# E-01 upstream JTAG lock-step receipt

Date: 2026-08-31
Owner: lane E
Board: Waveshare ESP32-S3-Touch-AMOLED-1.8 V2, chip revision v0.2
esp32sim: `aa851249341e8cd122e7f4852d4c0f002e46d887`

The immutable A-01 bundle verified before board access. The two sessions used
its exact 16 MB physical flash image, raw efuse and strap state, and raw reset
state. The flash SHA-256 was
`f2a8856a69b41ff6b5d88e060ca4067cff34517d7bd1a9efda3c60aab221656c`.
Each session's live 16 efuse rows and strap word were byte-identical to A-01;
the strap word was `0x0000002b`.

The unmodified upstream `hw/difftest.sh` flow ran two independent reset
sessions of 8,000 steps. Both sessions captured exactly 8,000 hardware rows
and 8,000 emulator rows, GDB logged `done 8000`, and the command exited zero.
Both report:

- no PC divergence across 8,000 compared steps;
- zero timing resynchronizations;
- one retained register difference at step 15, PC `0x40000492`: hardware
  `a2=0x00000015`, emulator `a2=0x0000001f`.

The hardware traces from the two sessions are byte-identical with SHA-256
`3d9493b0913b6f56cf7d0bc5438502ef6fd407b75755cd37238d67a51b391b96`.
The emulator traces are byte-identical with SHA-256
`d27b4908bd9663f9d8630bc38a23614d0fbe3c335c76b5a4f818a0db05cfe809`.

Session 1 ran from `2026-08-31T18:51:53Z` through
`2026-08-31T19:04:34Z`. Session 2 ran from `2026-08-31T19:05:22Z` through
`2026-08-31T19:18:10Z`. The upstream script resolved OpenOCD
`v0.12.0-esp32-20260424` and Xtensa GDB `17.1_20260402`; these exact binaries
are recorded in the raw provenance. A-01 used newer OpenOCD
`v0.12.0-esp32-20260703`, so the complete outputs from both versions remain
part of the evidence.

The hash-verified raw receipt is at
`/Users/sarah/src/a/esp32s3-lane-e/esp32sim/out/lane-e/idf61/jtag-lockstep`.
Its `PROVENANCE.md` records exact commands, host and tool versions, input
hashes, UTC boundaries, full output locations, and board state effects.
Its `SHA256SUMS` verifies all copied inputs, full logs, traces, and final
reset receipt.

This establishes architectural lock-step for the observed 8,000-step reset
path. It does not establish CCOUNT or calibrated timing equivalence. No flash
write occurred. A final `reset run` exited zero at
`2026-08-31T19:18:58Z`, leaving the installed ESP-IDF 6.1 gate harness
running before lane E released the board.
