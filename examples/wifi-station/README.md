# IDF WiFi station — specimen for WiFi emulation

The ESP-IDF `wifi/getting_started/station` example, open network, SSID `esp32sim`, built for the S3
(`idf.py set-target esp32s3 build`). Used to reverse-engineer and drive the WiFi MAC model.

    ../../target/release/esp32sim --board none --boot rom --console usb --max-seconds 15 \
      --bootloader build/bootloader/bootloader.bin --ptable build/partition_table/partition-table.bin \
      --app build/wifi_station.bin --elf build/wifi_station.elf --wifi ssid=esp32sim

The unmodified blob boots, scans, authenticates, associates and takes a DHCP lease:

    wifi:connected with esp32sim, aid = 1, channel 6, BW20, bssid = 02:53:49:4d:00:01
    esp_netif_handlers: sta ip: 10.0.2.15, mask: 255.255.255.0, gw: 10.0.2.2

`ESP_EMU_DEBUG_WIFI_FRAMES=1` decodes every 802.11 frame on the air, `ESP_EMU_DEBUG_NET=1` the
DHCP/ARP/ICMP exchanges. The example is built for an **open** network (WPA2 is not implemented yet).
