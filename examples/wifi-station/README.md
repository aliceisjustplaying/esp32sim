# IDF WiFi station — specimen for WiFi emulation

The ESP-IDF `wifi/getting_started/station` example, WPA2-PSK, SSID `esp32sim`, password
`esp32sim-pass`, built for the S3
(`idf.py set-target esp32s3 build`). Used to reverse-engineer and drive the WiFi MAC model.

    ../../target/release/esp32sim --board none --boot rom --console usb --max-seconds 15 \
      --bootloader build/bootloader/bootloader.bin --ptable build/partition_table/partition-table.bin \
      --app build/wifi_station.bin --elf build/wifi_station.elf --wifi ssid=esp32sim,psk=esp32sim-pass

The unmodified blob boots, scans, authenticates, associates and takes a DHCP lease:

    wifi:connected with esp32sim, aid = 1, channel 6, BW20, bssid = 02:53:49:4d:00:01
    esp_netif_handlers: sta ip: 10.0.2.15, mask: 255.255.255.0, gw: 10.0.2.2

    wpa: WPA: Key negotiation completed with 02:53:49:4d:00:01 [PTK=CCMP GTK=CCMP]

`ESP_EMU_DEBUG_WIFI_FRAMES=1` decodes every 802.11 frame on the air, `ESP_EMU_DEBUG_NET=1` the
DHCP/ARP/ICMP exchanges. Drop `psk=` (and rebuild the example without a password) for an open
network. The build enables `CONFIG_WPA_DEBUG_PRINT`, which is what made the handshake debuggable.
