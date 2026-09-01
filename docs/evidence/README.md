# Evidence

Hardware receipts and feasibility measurements backing the adopted
numbers in [`../STATUS.md`](../STATUS.md).

Provenance: `timing/` and `E-01-jtag-lockstep.md` were copied verbatim
on 2026-09-02 from the archived planning repository
(https://github.com/aliceisjustplaying/esp32s3-cycle-accurate-wasm at
commit `96e4bb808127983d7b25409440377657b176dbee`, paths
`timing/evidence/` and `lanes/receipts/`). File contents and pinned
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
