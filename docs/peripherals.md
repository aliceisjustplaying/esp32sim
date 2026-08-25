# Peripheral coverage

Legend: **full** = everything the IDF/Arduino drivers use; **partial** = the paths exercised
so far; **stub** = accepts writes, returns plausible reads; **—** = not modelled (unknown
registers are logged with `--log-periph`).

| Block | Base | Status | Modelled |
| --- | --- | --- | --- |
| Interrupt matrix (per core) | 0x600C2000 | full | source→line mapping for both cores, level lookup |
| System / sensitive / APB_CTRL | 0x600C0000… | partial | core-1 release/reset, cache enables, clock regs as stubs |
| Cache MMU | 0x600C5000 | full | 512 entries, flash and PSRAM pages, invalid entries fault |
| SPI0/SPI1 (flash controller) | 0x60002000/3000 | full | user commands, JEDEC (size follows `--flash-mb`), read/program/erase, status/QE |
| Octal PSRAM (on SPI1 CS1) | — | full | mode registers MR0–MR8, sync read/write, `--psram-mb` |
| efuse | 0x60007000 | partial | MAC, chip revision, defaults; `--efuse-regs` loads a dump |
| RTC_CNTL | 0x60008000 | partial | reset cause, slow-clock time, SW resets, RTC watchdog (stages, feed, wprotect) |
| systimer | 0x60023000 | full | 2 units, 3 targets, one-shot/periodic |
| Timer groups 0/1 | 0x6001F000/20000 | partial | timer 0 with alarm/auto-reload; WDT registers as stubs |
| GPIO / IO_MUX | 0x60004000/9000 | full | out/enable/input, pin matrix in/out selects, edge/level interrupts, strap |
| UART0/1/2 | 0x60000000… | partial | TX FIFO to console, RX from scripts, TX-done/empty interrupts |
| USB Serial/JTAG | 0x60038000 | full | TX/RX FIFOs, interrupts (IDF console and Arduino `Serial`) |
| I2C0/I2C1 | 0x60013000/27000 | full | IDF `i2c_master` command list, FIFOs, NACK/END/COMPLETE interrupts |
| GDMA | 0x6003F000 | partial | out-channels (I2S0/I2S1) and in-channels (CAM); descriptor walk, DONE/EOF/TOTAL_EOF |
| I2S0 / I2S1 | 0x6000F000/2D000 | partial | TX: clock config, sample rate, 16-bit stereo capture to PCM; RX — |
| RMT | 0x60016000 | partial | TX channels: symbol RAM, clock divider, end marker, done interrupt; RX — |
| LCD_CAM | 0x60041000 | partial | camera engine: start/reset, VSYNC interrupt, frame pump to GDMA; LCD side — |
| SHA | 0x6003B000 | full | SHA-1/224/256 in block mode (bootloader image verification) |
| RNG | 0x6003B000 | full | random words |
| regi2c (PLL/BBPLL) | 0x60021000 | stub | reads back what was written |
| LEDC, PCNT, ADC, SPI2/3, TWAI, SDMMC, USB-OTG | — | — | |
| WiFi MAC/BB/PHY, BT | — | — | see networking-plan.md |

CPU-side: full base ISA, FPU (single precision), MAC16, booleans, PIE (all esp-dl/esp-dsp
ops; FFT/GPIO/s32 corners decode but are not executed).
