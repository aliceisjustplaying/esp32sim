# WiFi hardware emulation (full, unmodified firmware)

Goal (chosen 2026-08-25): run the **unmodified** Espressif WiFi stack — `esp_wifi` + the closed
`libpp`/`libnet80211`/`libphy` blobs — so firmware associates with an access point and moves IP
traffic, with **no application changes and no shim**. The PHY math is *not* emulated; we fake the
radio/analog registers so the blob's calibration passes, and model the **MAC** (the descriptor
rings and interrupt events) plus a **virtual access point** the MAC "hears".

This is reverse-engineering, done against a specimen: `examples/wifi-station` (the IDF station
example, open network, SSID `esp32sim`). Espressif never documented these peripherals; the register
map matches the classic ESP32's as reverse-engineered by **esp32-open-mac** (their `0x3ff73000` is
our `0x60033000`). Everything below was learned by tracing the blob's own accesses with
`--regstat`, `--trace-fn`, `ESP_EMU_DEBUG_WIFI` and disassembly of the ROM/library functions.

## Status

Working today (`--wifi ssid=…[,chan=,psk=,bssid=]`, no firmware changes):
- The blob boots, calibrates and reaches `wifi:mode : sta` — the PHY calibration loops
  (`rom_pkdet_vol_start`, `txdc_cal_v70`, `ram_iq_est_enable`, the analog I2C-master handshakes,
  the temperature sensor) are satisfied by faked done-bits.
- **Scan**: the station transmits probe requests; the virtual AP answers with probe responses and
  beacons; the blob receives, parses and lists the BSS (`scan_parse_beacon`, `sta_recv_mgmt`).
- **MAC model** (`esp32s3/src/periph.rs` `WifiMac`): TX via the PLCP0 queue registers → frame
  fetched from its DMA descriptor and completed (`txq_complete`/event bit 7, `hal_mac_get_txq_pmd`
  result word); RX via the descriptor ring at `0x088` → `rx_ctrl` header + frame + FCS written into
  the next descriptor, RX event bits 14/24 (`wDev_ProcessFiq` → `lmacProcessRxSucData`).
- **Virtual AP** (`esp32s3/src/wifi.rs`): beacons, probe responses, open-system auth, association,
  and 802.11 ↔ Ethernet translation for data frames.
- Reaches **`state: init -> auth`** and exchanges authentication frames with the AP.

Not yet working: **association does not complete** — the station authenticates, the AP replies, but
the station falls back `auth -> init` (status 0x200, timeout) instead of sending the association
request. Next debugging step: the `rx_ctrl` status/`filter_match` bits or the auth-response timing
that `sta_recv_mgmt` accepts (the auth reply is delivered but not consumed). After that: WPA2 4-way
handshake (needs the crypto the blob does in software — should "just work" once assoc completes),
then a network backend (Ethernet frames ↔ libslirp, per docs/networking-plan.md).

## What was reverse-engineered

MAC register file at **`0x60033000`** (block 0x33) / `0x60034000` (0x34), WDEV at `0x60035000`:

| Register | Meaning |
| --- | --- |
| `0x088` `WIFI_BASE_RX_DSCR` | RX descriptor ring base (hardware fills from here) |
| `0x08c/0x090` | next / last RX descriptor |
| `0x084` bit 0 | RX descriptor reload (restart at base) |
| `0xc3c` / `0xc40` | MAC interrupt events / clear (bit 7 TX done, bits 14/24 RX data) |
| `0xcb0` / `0xcac` | per-queue TX complete / clear |
| `0xca8` / `0xca4` | per-queue TX error / clear |
| `0xd08 − 8·q` `MAC_TX_PLCP0[q]` | queue q descriptor addr; bit 31 triggers TX |
| `0xd14` | `hal_init` handshake (write bit 1, poll bit 0) |
| `0x040/0x060` (per slot) | MAC address / BSSID filters |
| `0x0d8` | RX policy |
| WDEV `0x0c/0x10/0x14/0x18/0x1c` | TSF counter: latch/load and the 64-bit value |
| WDEV `0x118/0x11c` | power interrupt events / clear |

DMA descriptor (esp32-open-mac `dma_list_item`): `size:12 length:12 _:6 has_data:1 owner:1`, then
`packet` and `next` pointers. RX buffer = a 48-byte `wifi_pkt_rx_ctrl` header (rssi, rate, channel,
timestamp, `sig_len`, `rx_state`) + the 802.11 frame + 4-byte FCS; word 0 top bits are the
frame-valid / filter-match flags `wDev_ProcessRxSucData` gates on.

## Tools added for this work

- `--regstat FILE` — per (address, pc, r/w) access counts with the resolved symbol.
- `--trace-fn PREFIX` — log each entry to functions matching a prefix, with args and caller.
- `--stub SYMBOL[=val]` — synthetic return at a function entry.
- `--wifi SPEC` — attach a virtual AP.
- `--poke` script action; `ESP_EMU_FAKE_READ=addr:or[:and],…` runtime register overrides.
- `ESP_EMU_DEBUG_WIFI` (register trace) / `ESP_EMU_DEBUG_WIFI_FRAMES` (frame decode).

References: esp32-open-mac (github.com/esp32-open-mac/esp32-open-mac, `main/hardware.c`,
`main/mac.c`) and its blog (zeus.ugent.be/blog/23-24/open-source-esp32-wifi-mac/); Ebiroll's and
esp32-open-mac's QEMU forks for the classic ESP32 (Apache-2.0, consulted for behaviour only).
