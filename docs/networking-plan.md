# Networking (WiFi) plan for esp32sim

Goal: firmware that needs the network — the Waveshare autopling web UI / `/api/pling`, the
esp32-screen Home Assistant panel, Atech cloud events — runs in the emulator and talks to
real hosts (Home Assistant, price APIs, a browser on the Mac), with no root privileges.

## What we can and cannot emulate

The ESP32-S3 WiFi stack is `esp_wifi` (open glue) on top of `libpp`/`libnet80211`/`libphy`
(closed blobs) driving undocumented MAC/baseband/RF registers (the `BB`, `NRX`, `FE`, `FE2`,
`WIFI MAC` blocks that `--log-periph` shows). Emulating that register interface well enough
for the blob to associate and pass frames is a Wokwi-scale reverse-engineering job (months) —
that is **phase 4, research only**. Everything useful is reachable earlier by substituting the
driver *below the public API*, which is what Espressif's own QEMU does.

Two facts make the substitute cheap:

- ESP-IDF ships an **OpenCores Ethernet MAC driver** for QEMU (`CONFIG_ETH_USE_OPENETH`,
  `components/esp_eth/src/openeth/`), registers at `0x600CD000`, interrupt source
  `ETS_WIFI_MAC_INTR_SOURCE` (0), available for esp32s3. The MAC is tiny (MODER, INT_SOURCE,
  INT_MASK, MAC_ADDR0/1, TX_BD_NUM, 128 buffer descriptors at +0x400); QEMU's
  `hw/net/opencores_eth.c` is the semantic reference.
- `protocol_examples_common` (used by autopling's `example_connect()`) already offers
  `EXAMPLE_CONNECT_ETHERNET` + `EXAMPLE_USE_OPENETH` — a menuconfig change, no source change.

Host side: **libslirp** (QEMU's user-mode NAT; `brew install libslirp`, crate `libslirp-sys`)
gives the guest 10.0.2.15, DHCP, DNS, outbound TCP/UDP through the Mac's own connections, and
`hostfwd` port forwards for inbound (guest HTTP server → `http://127.0.0.1:8080`). No root,
no vmnet entitlement. Limitation: no multicast, so mDNS (`esp-web.local`, HA discovery) does
not cross; use IP addresses or add an emulator-side mDNS responder later.

## Phases

### Phase 1 — virtual NIC + user-mode network (2–3 days)
- `esp32s3/src/openeth.rs`: OpenCores MAC model; RX/TX through the BD ring in the MMIO
  window, IRQ on source 0, MAC address from the efuse/`--mac`.
- `esp32s3/src/net.rs`: `--net none|user[,hostfwd=tcp:127.0.0.1:8080-:80,...]` backed by
  libslirp (FFI, own thread; frames exchanged through a channel; `--pcap file` for Wireshark).
- UI: a **Network** card — guest IP, TX/RX counters, active forwards, link up/down toggle.
- Free core 0: done as `--stub esp_wifi_start=0` (a synthetic return at the function entry);
  the real fix is the shim in phase 2.
- Validate: IDF `examples/protocols/http_request` built with ETHERNET+OPENETH fetches a real
  URL; then **autopling** rebuilt with the same two menuconfig options → its web UI on
  `http://127.0.0.1:8080`, `curl -X POST …/api/pling` plings the emulated speaker,
  detector + camera + pling all live in one page.

### Phase 2 — `esp32sim_wifi` shim component (1–2 days)
Projects that call `esp_wifi_*` directly (esp32-screen: `esp_wifi_init/set_mode/set_config/
start/connect/disconnect/scan_start/scan_get_ap_records/sta_get_ap_info`) get a drop-in
component with the same public headers that implements them over the openeth netif: posts
`WIFI_EVENT_STA_START/CONNECTED` (and `DISCONNECTED` on link-down from the UI), lets the
default `esp_netif` handlers deliver `IP_EVENT_STA_GOT_IP`, answers scans with a virtual AP
list (`--net user,ssid=Home,rssi=-55`). Integration is `EXTRA_COMPONENT_DIRS` / a component
override in `idf_component.yml`; no application source changes. Target: the HA panel polling
elprisetjustnu.se and Home Assistant from the emulator.

### Phase 3 — Arduino / PlatformIO (≈1 day, some uncertainty)
Arduino's `WiFi.begin()` is the same `esp_wifi_*` API inside `libesp_wifi.a`; the shim must
win at link time (`lib_extra_dirs` + `-Wl,--allow-multiple-definition`, or a patched
`libesp_wifi.a` in a PlatformIO `board_build` override). Needed for the Atech firmware's cloud
events (`wifi.postStateEvent`), which would then reach a local mock of the Atech dev server.

### Phase 4 — real 802.11 emulation (research)
Only if unmodified binaries must associate. Steps if ever: log the blob's register traffic
(`--log-periph` already names the blocks), find the MAC RX/TX descriptor path in `libpp`,
model beacons/association from a virtual AP. Not planned.

## Alternatives considered
- **TAP/vmnet** instead of slirp: real LAN presence (mDNS, HA discovery work) but needs root
  on Linux and the vmnet entitlement (or `sudo`) on macOS. Worth adding as `--net tap` later;
  slirp first because it needs nothing.
- **Pure-Rust NAT** instead of libslirp: no C dependency, but a TCP proxy state machine is a
  week of work for no functional gain. Revisit if packaging (`cargo install`) matters.
- **Custom "esp32sim NIC"** instead of OpenCores: would need our own guest driver for IDF *and*
  Arduino; openeth's driver already exists in IDF and is target-independent.
