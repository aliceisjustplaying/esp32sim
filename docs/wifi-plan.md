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

Progress (2026-08-25): **authentication now completes with the unmodified blob** (`cnx_auth_done`
fires and the station proceeds to `cnx_do_assoc`). The breakthrough: the blob's RX path only
*indicates* a frame up the stack when the descriptor ring is **shallow** — when several descriptors
are pending at once it switches to a batch block-recycle mode (`wDev_ProcessRxSucData`/
`wDev_IndicateFrame` receive `a3 = pending count`; `a3 = 1` -> indicate -> `ppRxPkt` -> `sta_input`,
`a3 = 0xa` -> recycle -> dropped). The virtual AP was delivering beacons + probe responses in bursts,
so the auth response arrived amid a deep ring and was recycled. `bus.rs::wifi_air_step` now delivers
**one frame at a time, spaced by ~400 us of airtime** (excess requeued), which keeps the ring shallow
and lets the auth response reach the 802.11 stack.

Determinism note: the WDEV RNG (`0x60035000+0x7c`) is fixed-seed, so runs are reproducible for
tracing/tests.

Still to do:
- **Reliability**: auth completes on some attempts, not all — the auth response must still land in a
  shallow-ring window, which the beacon/probe cadence can crowd. Deliver management responses in true
  isolation (pause beacons briefly around a connect), or model the ring-depth gate exactly.
- **Association**: `cnx_do_assoc` runs but the assoc response handling / state to CONNECTED is not yet
  verified end to end.
- Then **DHCP + internet** via a libslirp backend (the 802.11<->Ethernet path already exists).

This is the minimal AP logic that cannot be skipped for unmodified firmware; the hardware oracle
(JTAG on the Atech board) remains available to dump the real ring/bank state when needed.

## Scope: what is and isn't needed

To get *unmodified* firmware to "joined + internet", the blob's state machine must be walked to
CONNECTED by frames a real AP would send. There is no register or flag that shortcuts it. But the
AP logic is **minimal** if the network is **open** (no PSK): just auth-response + assoc-response, no
beacons-for-scan strictly needed, no WPA2 4-way handshake, no crypto. The heavy AP behaviour built
so far can be trimmed to that. Then "internet" is the **libslirp** backend (DHCP + DNS + NAT over the
Mac's network), which is independent of association fidelity and is the larger, reusable half.

Alternative if the auth-accept RE stalls: a small `esp_wifi` shim / OpenCores-Ethernet netif
(docs/networking-plan.md) skips association entirely but requires a one-line firmware config change
(not fully "unmodified"). That's the decision to make.

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

DMA descriptor (esp32-open-mac `dma_list_item`, **confirmed on silicon 2026-08-25** via JTAG on the
Atech board running this specimen): `size:12 length:12 _:6 has_data:1 owner:1`, then `packet` and
`next` pointers. A filled RX descriptor reads `dw0=0xc0..` — the hardware sets has_data (bit 30) and
**leaves owner (bit 31) set**; the register pointers (`0x088/8c/90`) hold the low 20 bits over
`0x3FC0_0000`, while the in-descriptor `packet`/`next` are full addresses. `rx_last` carries a `0x01`
prefix (bit 24). The 48-byte `rx_ctrl` header begins with the signed RSSI in the low byte of word 0. RX buffer = a 48-byte `wifi_pkt_rx_ctrl` header (rssi, rate, channel,
timestamp, `sig_len`, `rx_state`) + the 802.11 frame + 4-byte FCS; word 0 top bits are the
frame-valid / filter-match flags `wDev_ProcessRxSucData` gates on.

## Hardware ground truth

The real Atech board (ESP32-S3, same silicon) is used as the oracle: flash this specimen, let it
receive real beacons off the air, then over the built-in USB-JTAG (`openocd-esp32` + gdb) `halt` and
read the live WiFi MAC registers and RX descriptor ring — `hw/difftest*.sh` show the openocd/gdb
setup. This confirmed the descriptor bit layout (owner stays set), the masked register-pointer format,
and the 48-byte `rx_ctrl` header. Reflashing is reversible: `boards/atech14/` rebuilds the synth,
or `esptool write_flash 0 hw/atech/flash-8M.bin` restores the original dump byte-for-byte.

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
