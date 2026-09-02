# Wasm-emitting LX7 JIT spike

This disposable crate emits one WebAssembly module for an SRAM LX7 block and
runs it under Node. Exported memory holds 16 address registers, PC, a 64-bit
cycle field, and the zero-overhead-loop registers.

Coverage is three-byte `movi`, `addi`, `add`, `sub`, `and`, `or`, and `xor`;
the full 24-opcode conditional-branch set plus observed `bltz` and `bgez`;
`j`; `loop`, `loopnez`, and `loopgtz`; and `l32r`. Branches charge the adopted
3 cycles taken or 1 not taken, `j` charges 3, and loop setup charges 5.
`l32r` architecture emits only when the caller resolves its adopted 1 to 2
cycle interval for that observation. The default fails closed.

Calls, returns, window operations, exceptions, indirect jumps, other loads and
stores, special registers, cache operations, floating point, PIE, and traps end
compilation with opcode and PC. The caller falls back to the interpreter for
that block. Empty and truncated blocks also fail.

On `calibration/esp32s3-opcode-ladders/tinydraw-opcode-histogram.json`, the
emitter covers 15,705,620 of 46,690,498 dynamic instructions. The instruction
fallback rate is 66.362 percent. The 100-case branch-and-loop exit run emits
88,149 bytes for 600 static guest instructions, or 146.915 bytes per guest
instruction, including each module's state and section overhead.

Run the spike tests:

```sh
TMPDIR=/tmp cargo test --manifest-path wasm-jit-spike/Cargo.toml -- --nocapture
```
