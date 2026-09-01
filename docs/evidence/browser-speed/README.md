# Browser speed measurements

Measured 2026-08-31 on the target machine (2021 M1 Pro MacBook Pro).
The JSON files here are copied verbatim from the archived planning
repository (`experiments/esp32s3-browser-speed/results/`); the
methodology writeup lives in that archive.

Headline numbers, from `2026-08-31-chrome.json` and
`2026-08-31-bun-and-native.json`:

- Real-time dual-core budget: 480M emulated instructions per second
  (2 cores at 240 MHz, worst case 1 cycle per instruction).
- Browser interpreter, real kernel: about 105 MIPS in Chrome
  (confirmation run 104.99; an earlier run measured 109.0).
  Interpreter-only real time is refuted.
- JIT ceiling (emulated-instruction rate of a hand-written wasm
  probe, an upper bound, not a JIT): about 4,393 MIPS in Chrome
  (earlier run 4,618), about 8,587 MIPS native.

Host-side numbers vary 10 to 15 percent between runs; conclusions
rest on the 4x to 10x margins, not the exact figures. The open
question the JIT cost-accounting spike must answer: how much of the
roughly 9x margin over the 480 MIPS budget does cycle accounting
consume.
