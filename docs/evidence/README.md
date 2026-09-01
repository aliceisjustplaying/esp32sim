# Evidence

Hardware receipts and feasibility measurements backing the adopted
numbers in [`../STATUS.md`](../STATUS.md).

Provenance: `timing/`, `E-01-jtag-lockstep.md`,
`board-touch-identity-2026-09-01/`, and
`board-tinydraw-v2-normal-2026-09-01/` were copied verbatim on
2026-09-02 from the archived planning repository
(https://github.com/aliceisjustplaying/esp32s3-cycle-accurate-wasm,
paths `timing/evidence/` and `lanes/receipts/`, at commits `96e4bb8`
and `3adc5f3`). File contents and pinned
hashes are unchanged; `shasum -a 256 -c
timing/idf61-rebaseline-3db3985/SHA256SUMS` must pass. Relative paths
and command lines quoted inside copied files may reference the
archived repository's layout; the bytes, hashes, and numbers are the
receipts, not those paths.

Contents:

- `timing/`: CCOUNT-probe calibration receipts from the physical
  board (ESP32-S3 rev 0.2), adoption and analysis JSONs, and the
  ESP-IDF 6.1 rebaseline ledger under
  `timing/idf61-rebaseline-3db3985/`.
- `browser-speed/`: browser and native speed measurements on the
  target M1 Pro hardware; see `browser-speed/README.md`.
- `E-01-jtag-lockstep.md`: two independent 8,000-step JTAG lock-step
  sessions of upstream esp32sim against the physical board (no PC
  divergence; one persistent register difference at step 15).
- `board-touch-identity-2026-09-01/`: on-device touch controller
  identity capture; adopted as CST820 for the V2 board.
- `board-tinydraw-v2-normal-2026-09-01/`: the TinyDraw V2
  normal-product validation receipt (paced browser stroke, same
  source validated on physical hardware).
