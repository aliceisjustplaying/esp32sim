# Provenance

The SPI2 bus and device setup is copied from TinyDraw commit
`7a157d44a9da3312b1ecda2b45b116af2de28e63`, file
`calibration/esp32s3-tier-b/main/tier_b_probe.cpp` (SHA-256
`a79f3ffc177f9a9c94eee654825acf6562c197537f5b89138b18e79c3a84307a`). The copied fields are SPI2 host, GPIO 11 clock,
GPIO 4 through 7 data lines, 32,768-byte maximum transfer, automatic DMA
channel selection, 40 MHz mode 0 device, queue depth one, and no chip select.

This image sets `SPI_TRANS_MODE_QIO` on each transaction and
`SPI_DEVICE_HALFDUPLEX` on the device to exercise all four product data lines.
The panel initialization and panel-I/O device are intentionally absent, so the
no-chip-select traffic cannot address the panel.
