# Wasm JIT cycle-accounting spike

This spike measures the TinyDraw RGB565 scalar loop compiled to WebAssembly
with cycle accounting on and off. Both modules execute the same architectural
work. Accounting adds direct-mapped D-cache tag checks, miss classification,
and an inlined 64-bit synthetic cycle ledger. Cache misses are counted but
unpriced because first-line fill is a blocked exact-tier candidate and is not
adopted. The ledger measures the cost of compiled-in accounting machinery; it
is not a silicon timing claim.
The shape is a wasm-emitting JIT ceiling, not the product JIT, because block
dispatch, window guards, and interrupt polls are not included.

Run the target measurement on the 2021 M1 Pro MacBook in Google Chrome:

```sh
./docs/evidence/wasm-jit-accounting-spike-2026-09-01/run-chrome.py
```

The command fails closed on another CPU or browser, builds both modules with
Zig, runs seven 1.5-second samples per mode in paired order, verifies matching
full architectural-output hashes and exact synthetic ledgers, and writes `result.json`.
That file contains every raw sample and recomputable medians, accounting cost,
and clearance plus margin against the 480 MIPS real-time budget.

On the target Apple M1 Pro in Google Chrome 151.0.7922.174, the accounting-off
median was 10,486.56 MIPS and the accounting-on median was 4,478.24 MIPS, a
57.30 percent accounting cost. The accounted JIT spike clears the 480 MIPS
real-time budget by 3,998.24 MIPS, a margin of 832.97 percent. This is a
feasibility ceiling for the compiled block shape described above, not a
product-JIT throughput claim.

The kernel is the prior browser-speed probe's eight-instruction-per-pixel
model, extended with a compile-time accounting switch. Its source receipt is
the archived repository at `f00de0222812fb406eec22006dc1ea4b01382622`, path
`experiments/esp32s3-browser-speed/jit_ceiling.c`; the existing adopted
measurement points to that archive from `docs/evidence/browser-speed/README.md`.
