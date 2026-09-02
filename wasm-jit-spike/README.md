# Wasm-emitting LX7 JIT spike

This disposable crate emits WebAssembly for receipt-priced LX7 blocks and runs
it under Node. It keeps upstream fast mode untouched.

Coverage includes scalar `movi`, `movi.n`, `addi`, `add`, `sub`, `and`, `or`,
`xor`, `saltu`, `memw`, all 24 conditional branches plus `bltz` and `bgez`,
`j`, zero-overhead loops, and caller-resolved `l32r`. Register-window coverage
is `entry`, `call4/8/12`, `callx4/8/12`, `retw`, `retw.n`, and `movsp`, with
all 64 physical address registers and `WINDOWBASE`, `WINDOWSTART`, and PS
rotation state represented in wasm memory.

Window overflow and underflow checks run before mutation. They return a typed
fallback code so the interpreter takes the exception and the same compiled
block resumes after its handler. Exceptions and interrupt entry are not
emitted.

Loads and stores cover 8-, 16-, and 32-bit immediate forms. SRAM is direct
wasm memory at issue cost. MMIO uses receipt read tiers and the eight-entry
posted-write state stored with accounting. The caller supplies the
register-derived `ChipConfig`; MMIO and cache pricing refuse unmatched
configurations by name. RTC, eFuse, and NRX write costs fail closed where no
scalar price exists. Flash and PSRAM call the one `env.cache_access` host
import for cache state, value, and fill cost.

The committed TinyDraw histogram has 46,690,498 dynamic instructions. The
covered union accounts for 32,639,139, leaving an instruction fallback rate
of 30.095 percent. The major unhandled classes are extended ALU and shifts,
special-register operations, exception and interrupt returns, and cache/TLB
control operations.

The straight-line run emits 125.180 bytes per static guest instruction. The
branch-loop run emits 146.915, and the windowed run emits 238.791, including
module state and section overhead. The SRAM kernel JIT ledger is byte-identical
to the measured interpreter ledger.

Run the spike tests:

```sh
TMPDIR=/tmp cargo test --manifest-path wasm-jit-spike/Cargo.toml -- --nocapture
```
