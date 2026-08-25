//! The virtual air interface: 802.11 frame helpers and a minimal access point the emulated MAC
//! "hears". The AP beacons, answers probe requests, and completes open-system authentication and
//! association; data frames are handed to the network backend (docs/networking-plan.md).

pub fn mac_str(m: &[u8]) -> String { m.iter().map(|b| format!("{:02x}", b)).collect::<Vec<_>>().join(":") }

fn ies(f: &[u8], body: usize) -> Vec<(u8, &[u8])> {
    let mut v = Vec::new(); let mut i = body;
    while i + 2 <= f.len() { let (id, l) = (f[i], f[i + 1] as usize); if i + 2 + l > f.len() { break; } v.push((id, &f[i + 2..i + 2 + l])); i += 2 + l; }
    v
}
fn mgmt_body_offset(subtype: u16) -> usize { match subtype { 8 | 5 => 24 + 12, 0 => 24 + 4, 1 => 24 + 6, 11 => 24 + 6, _ => 24 } }

/// One-line description of an 802.11 frame (frame control, addresses, SSID for management frames).
pub fn describe(f: &[u8]) -> String {
    if f.len() < 24 { return format!("{} bytes (short): {:02x?}", f.len(), f); }
    let fc = u16::from_le_bytes([f[0], f[1]]);
    let (ty, st) = ((fc >> 2) & 3, (fc >> 4) & 0xf);
    let kind = match (ty, st) {
        (0, 0) => "assoc-req", (0, 1) => "assoc-resp", (0, 4) => "probe-req", (0, 5) => "probe-resp", (0, 8) => "beacon",
        (0, 10) => "disassoc", (0, 11) => "auth", (0, 12) => "deauth", (0, 13) => "action",
        (1, 11) => "rts", (1, 12) => "cts", (1, 13) => "ack", (1, 10) => "ps-poll",
        (2, 0) => "data", (2, 4) => "null", (2, 8) => "qos-data", (2, 12) => "qos-null", _ => "?",
    };
    let mut s = format!("{} bytes {} ({}/{}) a1={} a2={} a3={}", f.len(), kind, ty, st, mac_str(&f[4..10]), mac_str(&f[10..16]), mac_str(&f[16..22]));
    if ty == 0 { for (id, d) in ies(f, mgmt_body_offset(st)) { if id == 0 { s += &format!(" ssid='{}'", String::from_utf8_lossy(d)); } } }
    if ty == 2 && f.len() >= 32 { let off = if st & 8 != 0 { 26 } else { 24 }; if f.len() >= off + 8 && f[off] == 0xaa { let et = u16::from_be_bytes([f[off + 6], f[off + 7]]); s += &format!(" ethertype={:#06x}", et); } }
    s
}

/// True for a beacon frame (management subtype 8).
pub fn is_beacon(f: &[u8]) -> bool { f.len() >= 2 && f[0] & 0x0c == 0 && (f[0] >> 4) & 0xf == 8 }

#[derive(Clone, Debug)]
pub struct ApConfig { pub ssid: String, pub bssid: [u8; 6], pub channel: u8, pub psk: Option<String> }

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum StaState { Idle, Authenticated, Associated }

/// A frame the AP puts on the air, with the emulated time (µs) it should reach the station.
pub struct AirFrame { pub at_us: u64, pub frame: Vec<u8> }

pub struct VirtualAp {
    pub cfg: ApConfig,
    pub state: StaState,
    pub sta: [u8; 6],
    pub aid: u16,
    pub next_beacon_us: u64,
    pub beacon_interval_us: u64,
    pub seq: u16,
    pub queue: Vec<AirFrame>,
    pub log: bool,
    pub stats: (u64, u64, u64),   // beacons, probe responses, data frames from the station
}

impl VirtualAp {
    pub fn new(cfg: ApConfig) -> Self {
        VirtualAp { cfg, state: StaState::Idle, sta: [0; 6], aid: 1, next_beacon_us: 100_000, beacon_interval_us: 102_400, seq: 0,
                    queue: Vec::new(), log: std::env::var("ESP_EMU_DEBUG_WIFI_FRAMES").is_ok(), stats: (0, 0, 0) }
    }
    fn hdr(&mut self, fc: u16, a1: &[u8; 6], a3: &[u8; 6]) -> Vec<u8> {
        let mut f = Vec::with_capacity(128);
        f.extend_from_slice(&fc.to_le_bytes()); f.extend_from_slice(&[0, 0]);   // fc, duration
        f.extend_from_slice(a1); f.extend_from_slice(&self.cfg.bssid); f.extend_from_slice(a3);
        f.extend_from_slice(&(self.seq << 4).to_le_bytes()); self.seq = self.seq.wrapping_add(1);
        f
    }
    fn capability(&self) -> u16 { 0x0001 | if self.cfg.psk.is_some() { 0x0010 } else { 0 } | 0x0400 }   // ESS, privacy, short slot
    fn common_ies(&self, f: &mut Vec<u8>) {
        f.push(0); f.push(self.cfg.ssid.len() as u8); f.extend_from_slice(self.cfg.ssid.as_bytes());
        f.extend_from_slice(&[1, 8, 0x82, 0x84, 0x8b, 0x96, 0x0c, 0x12, 0x18, 0x24]);      // supported rates 1 2 5.5 11 (basic) 6 9 12 18
        f.extend_from_slice(&[3, 1, self.cfg.channel]);                                     // DS parameter set
        f.extend_from_slice(&[5, 4, 0, 1, 0, 0]);                                           // TIM
        f.extend_from_slice(&[7, 6, b'S', b'E', b' ', 1, 13, 20]);                          // country
        f.extend_from_slice(&[50, 4, 0x30, 0x48, 0x60, 0x6c]);                              // extended rates 24 36 48 54
        if self.cfg.psk.is_some() {                                                         // RSN: WPA2-PSK, CCMP
            f.extend_from_slice(&[48, 20, 1, 0, 0x00, 0x0f, 0xac, 4, 1, 0, 0x00, 0x0f, 0xac, 4, 1, 0, 0x00, 0x0f, 0xac, 2, 0, 0]);
        }
    }
    fn beacon_like(&mut self, subtype: u16, dst: &[u8; 6], now_us: u64) -> Vec<u8> {
        let bssid = self.cfg.bssid;
        let mut f = self.hdr(subtype << 4, dst, &bssid);
        f.extend_from_slice(&now_us.to_le_bytes());                                         // timestamp
        f.extend_from_slice(&100u16.to_le_bytes());                                          // beacon interval (TU)
        f.extend_from_slice(&self.capability().to_le_bytes());
        self.common_ies(&mut f);
        f
    }
    fn send(&mut self, at_us: u64, frame: Vec<u8>) {
        if self.log { let d = describe(&frame); if d.contains("auth") || d.contains("assoc") { eprintln!("[wifi] AP -> {}  hex={:02x?}", d, frame); } else { eprintln!("[wifi] AP -> {} (t+{} us)", d, at_us); } }
        self.queue.push(AirFrame { at_us, frame });
    }
    /// Time-driven behaviour (beacons). Returns frames due at or before `now_us`.
    pub fn step(&mut self, now_us: u64) -> Vec<AirFrame> {
        let mgmt_pending = self.queue.iter().any(|a| !is_beacon(&a.frame));
        if now_us >= self.next_beacon_us && !mgmt_pending {
            self.next_beacon_us += self.beacon_interval_us;
            if self.next_beacon_us <= now_us { self.next_beacon_us = now_us + self.beacon_interval_us; }
            let b = self.beacon_like(8, &[0xff; 6], now_us); self.stats.0 += 1;
            self.queue.push(AirFrame { at_us: now_us, frame: b });
        }
        let (due, later): (Vec<_>, Vec<_>) = std::mem::take(&mut self.queue).into_iter().partition(|a| a.at_us <= now_us);
        self.queue = later;
        due
    }
    /// The station transmitted `f` at `now_us`. Returns data frames (as 802.11) for the network backend.
    pub fn on_station_tx(&mut self, f: &[u8], now_us: u64) -> Option<Vec<u8>> {
        if f.len() < 24 { return None; }
        let fc = u16::from_le_bytes([f[0], f[1]]);
        let (ty, st) = ((fc >> 2) & 3, (fc >> 4) & 0xf);
        let mut a2 = [0u8; 6]; a2.copy_from_slice(&f[10..16]);
        let to_us = |a1: &[u8]| a1 == &[0xff; 6] || a1 == &self.cfg.bssid;
        match (ty, st) {
            (0, 4) => {                                                                      // probe request: for us or wildcard?
                let ssid_ok = ies(f, 24).iter().any(|(id, d)| *id == 0 && (d.is_empty() || *d == self.cfg.ssid.as_bytes()));
                if ssid_ok && to_us(&f[4..10]) { let r = self.beacon_like(5, &a2, now_us); self.stats.1 += 1; self.send(now_us + 1500, r); }
            }
            (0, 11) if to_us(&f[4..10]) => {                                                 // authentication (open system)
                let (alg, seq) = (u16::from_le_bytes([f[24], f[25]]), u16::from_le_bytes([f[26], f[27]]));
                if self.log { eprintln!("[wifi] station AUTH req alg={} seq={} status={} hex={:02x?}", alg, seq, u16::from_le_bytes([f[28],f[29]]), f); }
                if alg == 0 && seq == 1 {
                    self.sta = a2; self.state = StaState::Authenticated;
                    let mut r = self.hdr(11 << 4, &a2, &self.cfg.bssid.clone());
                    r.extend_from_slice(&[0, 0, 2, 0, 0, 0]);                                // open, seq 2, status success
                    self.send(now_us + 300, r);
                }
            }
            (0, 0) | (0, 2) if to_us(&f[4..10]) && self.state != StaState::Idle => {         // (re)association request
                self.state = StaState::Associated;
                let mut r = self.hdr(1 << 4, &a2, &self.cfg.bssid.clone());
                r.extend_from_slice(&self.capability().to_le_bytes()); r.extend_from_slice(&[0, 0]); r.extend_from_slice(&(0xc000 | self.aid).to_le_bytes());
                r.extend_from_slice(&[1, 8, 0x82, 0x84, 0x8b, 0x96, 0x0c, 0x12, 0x18, 0x24]); r.extend_from_slice(&[50, 4, 0x30, 0x48, 0x60, 0x6c]);
                self.send(now_us + 300, r);
            }
            (0, 12) | (0, 10) if to_us(&f[4..10]) => { self.state = StaState::Idle; }
            (2, _) if self.state == StaState::Associated && f[4..10] == self.cfg.bssid => {  // data to the DS
                if st == 4 || st == 12 { return None; }                                       // null frames (power save)
                self.stats.2 += 1; return Some(f.to_vec());
            }
            _ => {}
        }
        None
    }
    /// Wrap an Ethernet frame from the network backend into an 802.11 data frame from the DS.
    pub fn data_from_ds(&mut self, eth: &[u8]) -> Option<Vec<u8>> {
        if self.state != StaState::Associated || eth.len() < 14 { return None; }
        let mut dst = [0u8; 6]; dst.copy_from_slice(&eth[0..6]); let mut src = [0u8; 6]; src.copy_from_slice(&eth[6..12]);
        let bssid = self.cfg.bssid;
        let mut f = self.hdr((2 << 2) | 0x0200, &dst, &src);                                    // data, from-DS
        f[16..22].copy_from_slice(&src); f[10..16].copy_from_slice(&bssid);
        f.extend_from_slice(&[0xaa, 0xaa, 0x03, 0, 0, 0]); f.extend_from_slice(&eth[12..14]);  // LLC/SNAP
        f.extend_from_slice(&eth[14..]);
        Some(f)
    }
}

/// 802.11 data frame (to the DS) -> Ethernet frame.
pub fn data_to_eth(f: &[u8]) -> Option<Vec<u8>> {
    let fc = u16::from_le_bytes([f[0], f[1]]); let st = (fc >> 4) & 0xf;
    let hdr = if st & 8 != 0 { 26 } else { 24 };
    if f.len() < hdr + 8 || f[hdr] != 0xaa { return None; }
    let mut e = Vec::with_capacity(f.len());
    e.extend_from_slice(&f[16..22]);   // dst = addr3
    e.extend_from_slice(&f[10..16]);   // src = addr2
    e.extend_from_slice(&f[hdr + 6..hdr + 8]);
    e.extend_from_slice(&f[hdr + 8..]);
    Some(e)
}

/// FCS (CRC-32) as the MAC appends it.
pub fn fcs(data: &[u8]) -> u32 {
    let mut crc = 0xffff_ffffu32;
    for &b in data { crc ^= b as u32; for _ in 0..8 { crc = if crc & 1 != 0 { (crc >> 1) ^ 0xedb8_8320 } else { crc >> 1 }; } }
    !crc
}
