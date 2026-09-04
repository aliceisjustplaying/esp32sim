# ESP32-S3 WebAssembly JIT

This first slice compiles the seven-instruction TinyDraw SRAM kernel into a
self-contained WebAssembly module. Instruction costs come from the same
receipt-backed `Esp32S3SramCostModel` used by the interpreter.

The compiler currently accepts only straight-line `l32i`, `l32i.n`, `movi.n`,
`memw`, `sub`, and `saltu` blocks whose data accesses are explicitly SRAM. It
returns a named error for every other instruction or timing class.

The integration test runs the emitted module under Node and compares all 16
address registers, the program counter, the cycle total, and SRAM bytes with
the interpreter result:

```sh
cargo test -p esp32sim-wasm-jit
```

Measure the emitted kernel under Node with a fixed five-sample harness:

```sh
cargo run --release -p esp32sim-wasm-jit --example sram_kernel_speed
```

This is a compiler and equivalence checkpoint. It is not yet connected to the
browser emulator's block dispatcher.
