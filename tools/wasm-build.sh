#!/bin/sh
# Build the WebAssembly emulator and put it where web/index.html?wasm expects it.
#   tools/wasm-build.sh            -> web/wasm/esp32sim.wasm
#   python3 -m http.server -d web 8790 ; open http://127.0.0.1:8790/?wasm
set -e
cd "$(dirname "$0")/.."
rustup target list --installed | grep -q wasm32-unknown-unknown || rustup target add wasm32-unknown-unknown
cargo build --release --target wasm32-unknown-unknown -p esp32sim-wasm
cp target/wasm32-unknown-unknown/release/esp32sim_wasm.wasm web/wasm/esp32sim.wasm
if command -v wasm-opt >/dev/null 2>&1; then wasm-opt -O3 -o web/wasm/esp32sim.wasm web/wasm/esp32sim.wasm; fi
ls -la web/wasm/esp32sim.wasm
