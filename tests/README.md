# Tests

`cargo test --workspace` is hermetic: decoder tests, unit tests, and nothing that needs a
toolchain, a download or hardware.

The **golden-output tests** (`cli/tests/goldens.rs`) are the
regression bar for everything else: they run the committed demo firmware from the mask ROM and
compare the guest console, the captured audio (SHA-256) and the instruction count against the
files in `tests/golden/`. Bit-identical is the requirement — a timing change that shifts one
audio sample is a failure, not noise (see `docs/decisions.md`, "Performance").

They need the mask ROM ELFs, which ship with ESP-IDF (`~/.espressif/tools/esp-rom-elfs/`) or
can be pointed at with `ESP32SIM_ROM_DIR`, so they are `#[ignore]`d by default and never skip
silently: without a ROM they fail with the path they looked for.

```sh
cargo test --release --workspace -- --include-ignored      # ~3 s for the whole set
UPDATE_GOLDENS=1 cargo test --release --workspace -- --include-ignored   # after an intentional change
```

Use `--release`: the debug build runs the same scenarios ~30x slower. On a mismatch the actual
output is left next to the golden as `*.actual` for diffing.

| golden | what it covers |
| --- | --- |
| `atech-script1.*` | Pocket Synth: buttons, encoder, serial command, ST7735 over bit-banged SPI, WS2812 via RMT, SID voice on I2S/GDMA; also asserted equal to `boards/atech14/regression.wav`, and re-run with `--no-jit` (the JIT's oracle) |
| `atech-sid.*` | the cRSID C64 jukebox: a 6502 + SID inside the emulated S3 |
| `panel-sid.*` | Touch-LCD-4B energy panel: PSRAM, LCD_CAM RGB frames, GT911 touch and TCA9554 over I2C, ES8311 on I2S, a demo partition via `--flash-at` |
| `hello-s3.*` | stock ESP-IDF hello_world on UART0, ROM → bootloader → app_main |
| `hello-c3.*` | the same on the ESP32-C3 (RISC-V), with the MAC/reset cause/straps of the real module in `hw/c3-hello-world-real.txt`, through `esp32sim-c3` and `esp32sim --chip c3` |
| `atech-script1` with observers | the same run with `--profile-blocks --coverage --irq-latency --vcd` attached must be byte-identical and produce every report |

CI (`.github/workflows/ci.yml`) downloads the ROM ELFs from espressif/esp-rom-elfs and runs the
full set on every push and pull request.
