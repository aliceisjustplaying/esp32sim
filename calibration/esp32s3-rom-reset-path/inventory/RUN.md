# Inventory B run record (2026-09-06)

Command (from the worktree root, unmodeled emulator, real ROM, untimed):

    /Users/alice/src/a/esp32sim/target/release/esp32sim --boot rom \
      --bootloader out/rom-fetch-ladder/bootloader/bootloader.bin \
      --ptable out/rom-fetch-ladder/partition_table/partition-table.bin \
      --app out/rom-fetch-ladder/esp32s3_rom_fetch_ladder_calibration.bin \
      --elf out/rom-fetch-ladder/esp32s3_rom_fetch_ladder_calibration.elf \
      --board waveshare-amoled18-v2 --flash-mb 16 --psram-mb 8 --console usb --no-dump \
      --trace --break 0x403c88a4 --max-insns 50000000 > reset-trace.log

Inputs (SHA-256):
6760d090cd294f64595668c7388b732fbef36d903cb6fd3c9bb78c0111426f72  out/rom-fetch-ladder/bootloader/bootloader.bin
7f00b6c042a89b15b0cac534f82ed988caf29278ff5700b0c511eb1b5bb7c820  out/rom-fetch-ladder/partition_table/partition-table.bin
9dd0fd9f86367192a8e5d94d0a40d590cbfea4e01175985c4a3e0b342a5ffc71  out/rom-fetch-ladder/esp32s3_rom_fetch_ladder_calibration.bin
b622cde9269e6ba0f82832285097c71ab701d5d1c7a96d16b56af35e32487e8f  out/rom-fetch-ladder/esp32s3_rom_fetch_ladder_calibration.elf
c0ce0f338d1de1bdc6efbef1591779a2a42c1ab7d759d3c6ae8ae63a7dd34cfd  /Users/alice/.espressif/tools/esp-rom-elfs/20241011/esp32s3_rev0_rom.elf
98f60b2180b04f2dacf41ddc122460d4168201049300b8fc8166cd8194654c9e  /Users/alice/src/a/esp32sim/target/release/esp32sim

Outputs: reset-trace.log.gz and rom.dis are archived at ~/Archives/esp32s3/rom-reset-inventory-2026-09-06/ (SHA256SUMS there; too large for the source tree per docs/evidence/README.md); reset-trace.log.gz (raw trace, 18438318 bytes uncompressed), tally.py, reset-inventory.txt, rom.dis (objdump -d of the ROM ELF).
Break address 0x403c88a4 is the bootloader entry printed by the ROM for this image.
Limitation: the trace prints a0..a3 only; load/store effective addresses are not recoverable, so no MMIO/SRAM/flash access-target counts are claimed.

Note: the app ELF hash above (b622cde9...) is the rebuilt v1 image whose code is identical to the hardware-verified 476116ea... (see the integrity note in the 191331 archive); only the bootloader entry address matters for this inventory, and it is the same for both.
