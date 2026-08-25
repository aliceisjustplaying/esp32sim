# Command line

```
esp32sim --boot rom --bootloader B.bin --ptable P.bin --app A.bin [--elf X.elf ...] [options]
esp32sim --flash-image flash.bin --boot rom ...
```

## Images and boot
| Flag | Meaning |
| --- | --- |
| `--boot rom\|app` | `rom`: start at the mask ROM reset vector (real boot chain). `app`: load the app image segments and jump to its entry |
| `--bootloader F`, `--ptable F`, `--app F` | written to flash at 0x0 / 0x8000 / 0x10000 |
| `--flash-image F` | whole flash dump written at 0 |
| `--rom F` | mask ROM ELF (default: `~/.espressif/tools/esp-rom-elfs/*/esp32s3_rev0_rom.elf`) |
| `--elf F` (repeatable) | symbols for logs/profiles (app ELF, bootloader ELF) |
| `--flash-mb N`, `--psram-mb N` | flash size (JEDEC follows it) and octal PSRAM size (default 8 / 2) |
| `--board atech14\|waveshare-cam\|none` | board model (default atech14) |
| `--strap HEX`, `--reset-cause HEX`, `--efuse-regs F`, `--regs-init F` | reproduce a real chip's boot state (used by the differential tests) |
| `--no-reboot` | stop at the first chip reset instead of rebooting from ROM |
| `--stub SYMBOL[=value]` (repeatable) | return `value` (default 0) immediately when execution reaches the function's entry — e.g. `--stub esp_wifi_start=0` keeps the WiFi blob from spinning in PHY calibration until the network backend exists |

## Running
| Flag | Meaning |
| --- | --- |
| `--max-seconds S`, `--max-insns N` | stop after emulated time / instructions |
| `--script F` | host actions at emulated times (below) |
| `--console usb\|uart0\|both\|all\|none`, `--console-prefix` | which consoles to print |
| `--realtime` | pace to wall time without the UI |
| `--web PORT [--web-dir DIR]` | browser UI (implies real time) |
| `--cam-image F`, `--cam-fps N` | camera source for boards with a camera |

## Outputs
| Flag | Meaning |
| --- | --- |
| `--wav F` | audio captured from I2S (whichever controller played) |
| `--tft-png F`, `--gram-png F` | display frame (visible, scaled) / raw GRAM |
| `--no-dump` | skip the register dump at exit |

## Debugging
| Flag | Meaning |
| --- | --- |
| `--trace`, `--trace-from N` | per-instruction trace (from instruction N) |
| `--break PC` (repeatable) | stop at PC |
| `--watch ADDR` | stop when a word changes |
| `--peek ADDR,N`, `--disasm ADDR,N` | dump memory / disassemble at exit |
| `--profile` | top PCs by instruction count |
| `--log-periph` | log the first access to every unknown peripheral register |
| `--stop-after-exceptions N` | stop after N exceptions |
| `--regtrace F`, `--regtrace-from-pc PC`, `--regtrace-max N` | register trace file for `hw/compare.py` |

Environment: `ESP_EMU_DEBUG` (misc), `ESP_EMU_DEBUG_SPI`, `ESP_EMU_DEBUG_USB`,
`ESP_EMU_DEBUG_I2C` (bus traces), `ESP_EMU_LOG_ALL` (every peripheral access),
`ESP_EMU_RT_LOG` (20 ms windows that took > 40 ms wall, with PCs), `ESP_EMU_DEBUG_LCD` (LCD engine
start/reset, DMA link restarts, descriptor completions), `ESP_EMU_DEBUG_SPI2`.
`XTENSA_DIS_FILES=a.dis:b.dis` feeds the decoder equivalence test.

## Action scripts

One action per line, `<seconds> <cmd> [args]`; buttons/encoder are active low.

```
1.5  press btn1 150        # press for 150 ms (btn1, btn2, knob/sw, or a GPIO number)
2.0  release 16
2.5  gpio 17 0
3.0  knob cw 3             # 3 detents clockwise (ccw for the other way)
4.0  serial {"action":"set_note","value":"5"}
4.5  touch 450 30 1        # touch panel press at (450,30); `touch x y 0` releases
5.5  stop
```

`hw/wsdrive.py [port] [seconds]` drives the same inputs over the UI's WebSocket and reports
real-time keep-up (push gaps, lag, audio delivered); `hw/wsaudio.py [port] [seconds]` listens to the
UI's audio stream and reports sample counts/peak (how to check sound without listening).
