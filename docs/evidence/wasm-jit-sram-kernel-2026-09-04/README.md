# WebAssembly JIT SRAM kernel checkpoint

This receipt measures the first upstream-focused WebAssembly JIT slice at
commit `90243953749c00d7e9d6f8dce4b2d345b41e23b6`. The compiler emits the first
seven instructions of the committed TinyDraw SRAM kernel and obtains their
cycle prices from `Esp32S3SramCostModel`.

The benchmark instantiates one module under Node, warms it for 100,000 runs,
then measures five groups of 1,000,000 runs. Each run executes seven guest
instructions and accumulates seven receipt-backed cycles. The measured median
is 386.757255 million guest instructions per second.

Run from the repository root:

```sh
TMPDIR="$PWD/target/test-tmp" cargo run --release -p esp32sim-wasm-jit --example sram_kernel_speed
```

`result.json` records the exact source hashes, runtime versions, raw sample
times, and scope. This is a compiler microbenchmark with one JavaScript call
per kernel run; it does not claim browser-emulator integration performance.
