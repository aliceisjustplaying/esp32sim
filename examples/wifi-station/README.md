# IDF WiFi station — specimen for WiFi emulation

The ESP-IDF `wifi/getting_started/station` example, open network, SSID `esp32sim`, built for the S3
(`idf.py set-target esp32s3 build`). Used to reverse-engineer and drive the WiFi MAC model.

    ../../target/release/esp32sim --board none --boot rom --console usb --max-seconds 15 \
      --bootloader build/bootloader/bootloader.bin --ptable build/partition_table/partition-table.bin \
      --app build/wifi_station.bin --elf build/wifi_station.elf --wifi ssid=esp32sim

The unmodified blob boots, scans, and finds the virtual AP; association is still being brought up
(docs/wifi-plan.md). `ESP_EMU_DEBUG_WIFI_FRAMES=1` decodes every 802.11 frame on the air.
