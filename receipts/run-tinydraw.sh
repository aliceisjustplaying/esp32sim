#!/bin/sh
set -eu
assets=/Users/alice/src/a/esp32sim/target/performance-review-latest-main-2026-09-06/esp32sim-performance-review-latest-main-2026-09-06/assets
exec target/release/esp32sim --approximate-timing --boot rom --board waveshare-amoled18-v2 --psram-mb 8 --flash-mb 16 --rom "$assets/rom.elf" --bootloader "$assets/bootloader.bin" --ptable "$assets/ptable.bin" --app "$assets/app.bin" --elf "$assets/elf.elf" --no-dump --no-reboot "$@"
