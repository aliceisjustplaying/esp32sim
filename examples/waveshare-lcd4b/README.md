# Waveshare ESP32-S3-Touch-LCD-4B · esp32-screen energy panel in the emulator

    ./run-energy-panel.sh --web 8768      # http://127.0.0.1:8768/ — 480x480 panel, click/drag to touch
    ./run-energy-panel.sh --script touch.txt --tft-png panel.png --max-seconds 6

What works: ST7701S init through the TCA9554, LVGL UI on the RGB bus at 60 fps, GT911 touch (taps,
swipes between the tileview pages), the SID player (libcRSID → ES8311 → I2S0, audible in the UI or
`--wav`). `esp_wifi_start` is stubbed (no network backend yet, see docs/networking-plan.md), so
prices and Home Assistant tiles stay empty and NTP never arrives. Check audio without listening:
`hw/wsaudio.py 8768 8`.
