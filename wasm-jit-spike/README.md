# Wasm-emitting LX7 JIT spike

This disposable crate emits a WebAssembly module for one straight-line LX7
block, runs it under Node's WebAssembly runtime, and keeps a compiled-in 64-bit
cycle ledger. The state layout is 16 physical address registers, final PC, and
the ledger in exported linear memory.

Worked block shapes are three-byte `movi`, `addi`, `add`, `sub`, `and`, `or`,
and `xor` instructions in SRAM. Every worked instruction has exact cost 1 from
the straight-line SRAM issue price in `docs/STATUS.md`. The emitter decodes real
LX7 bytes with `xtensa-lx7`; the exit test executes the same bytes through the
product interpreter and compares all 16 registers and PC.

Branches, calls, returns, window operations, loops, loads, stores, special
registers, cache operations, floating point, PIE, and traps are unsupported.
They fail compilation by opcode and PC. Empty and truncated blocks also fail.

Run the exit test:

```sh
cargo test --manifest-path wasm-jit-spike/Cargo.toml -- --nocapture
```

The deterministic 100-block run emitted 43,890 module bytes for 1,993 guest
instructions, or 22.022 wasm bytes per guest instruction. The module includes
its 80-byte initial-state data segment, so the ratio includes fixed overhead.
