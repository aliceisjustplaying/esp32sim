# atech-sim — run Atech ESP32-S3 firmware locally before touching hardware

The firmware the Atech Hardware Platform generated for the "Pocket Synth"
(`firmware/src/main.cpp`, unchanged) runs **natively on the Mac** in `hostsim/`:
a virtual board with a local web UI (TFT, knob + LED ring, buttons, serial console,
audio through the browser) and headless scenario tests. No cloud, no accounts.

The same tree also builds the hardware image (PlatformIO, real SDK drivers) and,
optionally, a Wokwi image (cloud simulator — needs a token; kept for occasional
"does the real binary also boot" checks).

```sh
make run        # local simulator → http://127.0.0.1:8765  (click "enable audio" to hear it)
make test       # all sim/scenarios/*.yaml against the local simulator, TFT screenshots in hostsim/build/screenshots
make flash      # real board
```

## Board (Atech 14-port, read from the device + SDK catalog)

| | |
|---|---|
| MCU | ESP32-S3R2 (2 MB PSRAM), 8 MB flash, native USB-CDC console → `/dev/cu.usbmodem1101` |
| Speaker MAX98357A (I2S) | ports 5+6 → BCLK 12, LRCLK 13, DIN 10 |
| ST7735 TFT 160×80 | ports 9+10 → SCLK 2, CS 41, MOSI 1, DC 40 |
| Rotary encoder + 12-LED ring | ports 1+2 → CLK 5, DT 4, SW 9, ring 8 |
| Button 1 (play) / Button 2 (waveform) | port 3 → GPIO 17 / port 4 → GPIO 16 (active low) |

## Layout

```
hostsim/
  hal/                      Arduino / FreeRTOS / Preferences API implemented for the host
  hal/modules/…             the Atech driver PUBLIC APIs (same headers main.cpp includes)
  drivers/                  host implementations: Speaker → WebAudio + SIM:AUDIO analysis,
                            ST7735_TFT → GFXcanvas16 (identical fonts/rendering) → browser/PNG,
                            RotaryEncoder → UI/scenario driven, ButtonModule → virtual GPIO
  sim/                      VirtualBoard, HTTP+WebSocket server, scenario runner, PNG writer
  web/index.html            the board UI
  third_party/              Adafruit_GFX (BSD), ArduinoJson (MIT)
firmware/
  src/main.cpp.generated    what the Atech Hardware Platform emitted — verbatim, never edited
  src/main.cpp              that file plus the audio work below (SID chip engine, SID jukebox)
  lib/atech_*/              REAL drivers copied from the atech SDK (tools/sync-sdk-modules.sh)
  lib/sid/                  3-voice 6581-style SID chip core (ADSR, filter) driving the synth
  lib/crsid/                cRSID by Hermit (WTFPL): a whole C64 — 6502, SID, CIA, VIC — so the
                            board can play real .sid tunes; same engine the esp32-screen panel uses
  lib/sidtunes/             four HVSC tunes embedded as C arrays (PlatformIO has no EMBED_FILES)
  src/modules/…             glue the hosted platform includes but the SDK doesn't ship:
                            AtechSerial (SDK wire protocol), atech_helpers.h, forwarding headers
  src/sim/                  Wokwi-only: I2S shim (Wokwi has no I2S) → SIM:AUDIO lines
  platformio.ini            env:sim (Wokwi) · env:hw (board)
sim/diagram.json            Wokwi wiring: S3 DevKitC, ILI9341 (stands in for ST7735), KY-040,
                            NeoPixel ring, 2 buttons
sim/scenarios/*.yaml        headless tests · sim/screenshots/ TFT captures
tools/sync-sdk-modules.sh   refresh drivers after `uv pip install -U atech`
examples/idf-minimal/       bare ESP-IDF sample (pre-Atech)
```

## Setup

The Atech SDK hardware modules (`firmware/lib/atech_*`) are not part of this repository; fetch
them from the `atech` package with `make sync-sdk` before building the firmware.

```sh
make sdk                     # .venv with the atech SDK (drivers + `atech` CLI)
make install-wokwi-cli       # once
export WOKWI_CLI_TOKEN=…     # free: https://wokwi.com/dashboard/ci
make build && make test      # build for Wokwi, run all scenarios
make sim                     # interactive: type SDK actions, see serial output
```

## What the simulator reports

The real `speaker.cpp` is compiled with its I2S calls redirected to
`src/sim/sim_i2s.cpp`, which analyses the sample stream and prints, per ~46 ms
of audio:

```
SIM:AUDIO:note=A4 f=441 rms=0.28
```

Events use the SDK envelope, e.g.
`{"type":"event","payload":{"event_type":"state","key":"note_triggered","value":"C4",…}}`,
and actions are sent as `{"action":"set_note","value":"5"}` — identical to what
`atech send` / `atech monitor` speak to the real board. `take-screenshot` steps
save the TFT to `sim/screenshots/`.

## Hardware

```sh
make flash                   # env:hw — real I2S + ST7735
make monitor
make check                   # atech check: reboot + module health report
make send KEY=set_note VALUE=5
```

"Resource busy" on the port → a browser tab (Web Serial) or monitor holds it:
`lsof /dev/cu.usbmodem1101`, or `.venv/bin/atech free-port`.

## SID jukebox (real .sid tunes)

The Pocket Synth drives a SID *chip* model (`lib/sid`). The jukebox goes a step further and runs
**cRSID** (`lib/crsid`) — a complete emulated C64: the 6502 executes the tune's own machine code,
which writes the SID registers 50 times a second, exactly as it did in 1985. Four tunes are
embedded (Commando · Rob Hubbard, Wizball · Martin Galway, Irish Dream and On the Edge), and the
titles and authors on screen are read from each file's PSID header at run time.

| Control | Action |
| --- | --- |
| hold **button 1** ~0.7 s | start / stop the jukebox |
| **encoder** | previous / next tune |
| **button 2** | next tune |
| **knob press** | stop |
| serial JSON | `{"action":"play_sid","value":"0"}`, `{"action":"next_sid"}`, `{"action":"stop_sid"}` |

Try it in the emulator (the script drives the serial protocol, so no clicking):

```sh
printf '3.0 serial {"action":"play_sid","value":"0"}\n16.0 stop\n' > /tmp/sid.txt
./target/release/esp32sim --boot rom --bootloader $B/bootloader.bin --ptable $B/partitions.bin \
    --app $B/firmware.bin --elf $B/firmware.elf --script /tmp/sid.txt --wav commando.wav --max-seconds 16
```

Two things this board forced that the panel did not:

- **No PSRAM.** The emulated C64 is ~270 KB (64 KB RAM, both 64 KB IO banks, 64 KB ROM banks) and
  this board has 360 KB of internal heap and no PSRAM at all, so `cRSID_init` now falls back to
  internal RAM — and the firmware allocates the C64 when playback starts and frees it on stop
  (`cRSID_free`), so the synth and the WiFi stack get the memory back. Free heap goes 349 KB →
  37 KB while a tune plays and straight back afterwards.
- **44.1 kHz, not 22.05.** It feeds the same `Speaker` the synth uses, so no resampling. Rendering
  costs about a quarter of one core, and `Speaker::writeSamples` blocking on the I2S DMA is what
  paces playback.

## Scenarios

`sim/scenarios/*.yaml` use the Wokwi scenario format (`wait-serial`, `delay`,
`set-control`, `write-serial`, `take-screenshot`) so they run on both simulators.
The local runner adds knob control, which Wokwi lacks:

```yaml
- set-control: { part-id: knob, control: rotate, value: 2 }    # detents, negative = CCW
- set-control: { part-id: knob, control: pressed, value: 1 }
```

## What each simulator is for

| | hostsim (local) | Wokwi (cloud, optional) |
|---|---|---|
| Runs | `main.cpp` + driver logic compiled for the Mac | the actual ESP32-S3 binary |
| Catches | behaviour bugs: synth engine, UI, protocol, state | driver/register/timing/stack/watchdog bugs |
| Audio | heard in the browser + analysed | analysed only (no I2S in Wokwi) |
| Needs | nothing | free account token |

## Known limits (hostsim)

- Timing is host timing (`millis()` is real time, tasks are threads); no watchdog, no stack limits.
- The host `Speaker` reproduces the real driver's behaviour (background task, volume applied in
  `writeSamples`, 85/15 note gap, RTTTL) but not its exact I2S buffering.
- The rotary encoder is driven at detent level (no quadrature edges).
