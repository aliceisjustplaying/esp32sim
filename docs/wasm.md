# Running the emulator in the browser (WebAssembly)

The whole emulator — both Xtensa cores, the SoC, the boards, the virtual WiFi and subnet —
compiles to a single WebAssembly module and runs inside the page, in a Web Worker. Nothing is
uploaded anywhere: firmware is read from the visitor's disk (or fetched from files you host next
to the page) and executed in the tab.

## Build and try it

```sh
tools/wasm-build.sh                      # -> web/wasm/esp32sim.wasm (needs the wasm32-unknown-unknown target)
python3 -m http.server -d web 8790       # any static server; file:// will not do (workers, fetch)
open http://127.0.0.1:8790/?wasm
```

`?wasm` switches `web/index.html` from its WebSocket transport to the worker; the page gains a
firmware panel: board, flash/PSRAM size, an optional WiFi spec, function stubs, and file inputs
for the mask-ROM ELF, `bootloader.bin`, `partition-table.bin`, the app image, its ELF (symbols
— needed for stubs) and a script. **Boot** starts it; the rest of the page — console tabs,
display, touch, buttons, knob, audio, camera — is the same UI the native emulator serves.

For your own demos, `?wasm&fw=<name>` loads `web/wasm/fw/<name>.json` and boots it without
clicking (format in `web/wasm/fw/README.md`). Everything in that directory is git-ignored: the
mask ROM is Espressif's and the firmware is whoever built it; host them only where you may.

## On GitHub Pages

`.github/workflows/pages.yml` builds the module on every push to `main`, fetches the mask-ROM ELF
from the Apache-2.0 `espressif/esp-rom-elfs` release, and publishes `web/` — so the page at
**https://joakimeriksson.github.io/esp32sim/** is the emulator, with the demos in
`web/wasm/fw/demos.json` (hello_world and the Touch-LCD-4B energy panel with its SID player; the Atech Pocket Synth once Atech confirms its driver-module license) one click away and the file
inputs for anyone's own firmware. On a `github.io` host the page starts in wasm mode without
`?wasm`. Only firmware whose code is ours is committed under `web/wasm/fw/public/`; the panel is a
separate build with placeholder `secrets.h` values (checked with `strings` against the real ones).

**Demo data without a rebuild.** The panel firmware has a `demo` data partition (0x610000,
64 KB); when it holds a JSON document the firmware renders that — prices for today and
tomorrow, hourly kWh, tile states, header power, a fixed clock — and never starts WiFi. The
manifest writes `public/energydata.json` there (`flash_at`), so changing the demo is editing a
JSON file; real boards have the partition erased and behave as before. Natively:
`--flash-at 0x610000=web/wasm/fw/public/energydata.json`.

## What it is

```
web/index.html   the UI, unchanged; `link` is either a WebSocket or the worker
web/emu.js       page side: firmware panel, manifest loading, window.EmuLink
web/wasm/worker.js   owns the wasm instance, paces it to wall time, relays the UI protocol
wasm/            the crate: a C ABI over esp32s3::Machine (esp32sim_new / load / wifi / stub /
                 boot / run / out_* / in_*); no bindgen, no dependencies
```

Inside the module the machine talks to the page through the same `WebServer` the native build
uses, in **queue mode**: every `send_text`/`send_binary` lands in an outbox the worker drains
after each run slice (`docs/web-ui.md` is the protocol on both sides). The worker keeps the
machine at wall time: it computes the cycle count the clock has earned, runs in ≤2 M-cycle
slices, yields every 25 ms so frames and audio flow, and resynchronises instead of bursting if
the tab falls half a second behind. `Date.now()` is passed in for the emulated SNTP server.

## What works, measured (M-series Mac, Chrome)

| firmware | in the tab | notes |
| --- | --- | --- |
| IDF hello_world | real time | ROM → bootloader → app, `esp_restart` reboots through the ROM |
| Waveshare Touch-LCD-4B energy panel + SID player | **real time**, ~62 Minsn/s | LVGL at 60 fps, touch, the tune plays through WebAudio |
| Atech 14-port synth | real time | ST7735 and WS2812 decoded, buttons/knob, scripted scenario |

The interpreter in the browser is the basic-block interpreter — V8 executes the module natively
but the AArch64 JIT is not available (wasm has no executable memory) — so throughput is about
half the native block-interpreter figure and a quarter of the native JIT's. That is enough for
everything above at real time on this machine; PIE-heavy code (the autopling detector) will fall
behind.

## Limits

- **No NAT.** The browser has no sockets. With a `wifi=` spec the firmware still associates,
  gets a DHCP lease, resolves names and syncs time against the emulated subnet, but connections
  past the gateway are refused (the `--net none` behaviour). A WebSocket relay to a small host
  helper is the planned way out (`wasm-plan.md`).
- **No file outputs**: `--wav`, `--tft-png`, register traces — the page is the output.
- **Emulator log lines** (`[emu] …`) that the native build prints to stderr do not exist here,
  except the ones the wasm glue forwards (stubs, resets, load errors) to the console tab and the
  browser console.
- **Memory**: flash + PSRAM + SRAM + ROM plus the block caches; the panel configuration takes
  ~45 MB of wasm memory. The block tables are sized smaller than natively (`block.rs`).
- Audio needs one click on **enable audio** — browsers will not start WebAudio otherwise.
