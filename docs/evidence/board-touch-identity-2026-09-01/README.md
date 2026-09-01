# BOARD touch identity receipt

Date: 2026-09-01
Owner: lane BOARD
Board: Waveshare ESP32-S3-Touch-AMOLED-1.8 V2
Port: `/dev/cu.usbmodem101`

## Result

The on-device probe at TinyDraw commit
`4db22a6ba73f8e087722f27164e34ccd38dbdd8f` reset the board devices through
the TCA9554, then read the touch device at I2C address `0x15`. The three
identity registers returned successfully:

```text
0xA7 = 0xB7
0xA8 = 0x41
0xA9 = 0x02
```

The raw boot log reports `TINYDRAW_TOUCH_IDENTITY_DONE pass=1`. Waveshare's
V2 product documentation identifies the controller fitted to this board as
CST820. The controller is therefore adopted as CST820 for the exact V2 board
in decision 0014. This receipt does not apply to the V1 board or any other
revision.

Vendor board reference:
<https://docs.waveshare.com/ESP32-S3-Touch-AMOLED-1.8>

## Provenance

- UTC capture completed: `2026-09-01T00:35:13Z`
- TinyDraw repository: `aliceisjustplaying/tinydraw`
- TinyDraw branch: `board/touch-identity-probe`
- TinyDraw commit: `4db22a6ba73f8e087722f27164e34ccd38dbdd8f`
- ESP-IDF: `v6.1.0`
- EIM: `0.18.0`
- Compiler: `xtensa-esp-elf 15.2.0`
- esptool: `5.3.1`
- chip: ESP32-S3 QFN56 revision v0.2
- board MAC: `1c:db:d4:7b:85:c8`

The port was enumerated immediately before the flash and immediately before
the serial capture. The flash command was:

```text
eim run "idf.py -B '$PWD/../out/build/esp32-touch-identity-probe' -p /dev/cu.usbmodem101 flash" v6.1
```

The capture command was:

```text
uv run --script tools/esp32-capture.py /dev/cu.usbmodem101 work/board-touch-identity-2026-09-01/serial.log 30 --end-marker TINYDRAW_TOUCH_IDENTITY_DONE
```

## Artifact hashes

```text
f4782c51fe4b67302dcb97281f5d999ef6bcedecd5a35aa7db55f6585a63a5a2  bootloader.bin
f53268312c8caffe6c7f4e6c66d4092aeca3435c142db3116466f84a6a608d2d  partition-table.bin
e4f8fd51f7265112be58a86d62777d0dc6adac014c029cae83700c7a02fbf328  tinydraw_esp32.elf
0a2a696ef4884f02589e24b812e267edf8702757af3e6746ded5e640fac50e44  tinydraw_esp32.bin
9d7af17178759879b55aa8ad2e4802eb44cc5833d7bc9a4fd1d5f25dfe79484e  sdkconfig
7ae7d6f7c674b9dd86256c56f9de2fae37d7d1c06f8b2aa575058a8edb3f32c7  flash.log
25792e9e58d2fcf845bf4f7c189fe53d64da801ce90d21b4bc0f6655177a0d0b  serial.log
```

The generated firmware artifacts remain machine-local. `flash.log` and
`serial.log` are committed beside this receipt.
