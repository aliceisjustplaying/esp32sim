//! A minimal virtual network behind the virtual access point: DHCP, ARP and ICMP echo, enough for
//! firmware to get an IP address and see the link as usable. Frames in and out are Ethernet II.
//!
//! Default layout (QEMU's user-mode numbering, so it looks familiar):
//!   10.0.2.2  gateway / DHCP server  (MAC 02:53:49:4d:00:02)
//!   10.0.2.3  DNS
//!   10.0.2.15 the station
//! Outbound traffic to the real world needs a NAT backend (docs/networking-plan.md); until then this
//! answers everything on the local subnet.

pub struct VirtualNet {
    pub gw_mac: [u8; 6],
    pub gw_ip: [u8; 4],
    pub dns_ip: [u8; 4],
    pub sta_ip: [u8; 4],
    pub mask: [u8; 4],
    pub log: bool,
    pub dhcp_acks: u64,
    pub arp_replies: u64,
    pub pings: u64,
    pub unhandled: u64,
}

fn be16(b: &[u8]) -> u16 { u16::from_be_bytes([b[0], b[1]]) }
fn checksum(data: &[u8], init: u32) -> u16 {
    let mut sum = init;
    let mut i = 0;
    while i + 1 < data.len() { sum += be16(&data[i..]) as u32; i += 2; }
    if i < data.len() { sum += (data[i] as u32) << 8; }
    while sum >> 16 != 0 { sum = (sum & 0xffff) + (sum >> 16); }
    !(sum as u16)
}

impl VirtualNet {
    pub fn new() -> Self {
        VirtualNet { gw_mac: [0x02, 0x53, 0x49, 0x4d, 0x00, 0x02], gw_ip: [10, 0, 2, 2], dns_ip: [10, 0, 2, 3],
                     sta_ip: [10, 0, 2, 15], mask: [255, 255, 255, 0],
                     log: std::env::var("ESP_EMU_DEBUG_NET").is_ok(), dhcp_acks: 0, arp_replies: 0, pings: 0, unhandled: 0 }
    }

    /// Handle one Ethernet frame from the station; returns frames to send back to it.
    pub fn handle(&mut self, eth: &[u8]) -> Vec<Vec<u8>> {
        if eth.len() < 14 { return Vec::new(); }
        let mut src = [0u8; 6]; src.copy_from_slice(&eth[6..12]);
        match be16(&eth[12..14]) {
            0x0806 => self.arp(&eth[14..], &src),
            0x0800 => self.ipv4(&eth[14..], &src),
            et => { self.unhandled += 1; if self.log { eprintln!("[net] ignoring ethertype {:#06x} ({} bytes)", et, eth.len()); } Vec::new() }
        }
    }

    fn frame(&self, dst: &[u8; 6], ethertype: u16, payload: &[u8]) -> Vec<u8> {
        let mut f = Vec::with_capacity(14 + payload.len());
        f.extend_from_slice(dst); f.extend_from_slice(&self.gw_mac); f.extend_from_slice(&ethertype.to_be_bytes());
        f.extend_from_slice(payload); f
    }

    fn arp(&mut self, p: &[u8], src: &[u8; 6]) -> Vec<Vec<u8>> {
        if p.len() < 28 || be16(&p[6..8]) != 1 { return Vec::new(); }          // only requests
        let target = &p[24..28];
        if target == self.sta_ip { return Vec::new(); }                          // not ours to answer
        let mut r = Vec::with_capacity(28);
        r.extend_from_slice(&[0, 1, 8, 0, 6, 4, 0, 2]);                          // ethernet/ipv4, reply
        r.extend_from_slice(&self.gw_mac); r.extend_from_slice(target);           // sender = the address asked for
        r.extend_from_slice(src); r.extend_from_slice(&p[14..18]);                // target = the station
        self.arp_replies += 1;
        if self.log { eprintln!("[net] ARP who-has {}.{}.{}.{} -> {}", target[0], target[1], target[2], target[3], crate::wifi::mac_str(&self.gw_mac)); }
        vec![self.frame(src, 0x0806, &r)]
    }

    fn ipv4(&mut self, p: &[u8], src: &[u8; 6]) -> Vec<Vec<u8>> {
        if p.len() < 20 { return Vec::new(); }
        let ihl = ((p[0] & 0xf) as usize) * 4;
        if p.len() < ihl { return Vec::new(); }
        let (proto, body) = (p[9], &p[ihl..]);
        match proto {
            17 if body.len() >= 8 && be16(&body[2..4]) == 67 => self.dhcp(&body[8..], src),
            1 if !body.is_empty() && body[0] == 8 => {                              // ICMP echo request
                let mut icmp = body.to_vec(); icmp[0] = 0; icmp[2] = 0; icmp[3] = 0;
                let c = checksum(&icmp, 0).to_be_bytes(); icmp[2] = c[0]; icmp[3] = c[1];
                self.pings += 1;
                let mut dst_ip = [0u8; 4]; dst_ip.copy_from_slice(&p[12..16]);
                let mut src_ip = [0u8; 4]; src_ip.copy_from_slice(&p[16..20]);
                if self.log { eprintln!("[net] ICMP echo request -> reply ({} bytes)", icmp.len()); }
                vec![self.frame(src, 0x0800, &ip_packet(1, &src_ip, &dst_ip, &icmp))]
            }
            _ => { self.unhandled += 1; if self.log { eprintln!("[net] ignoring IPv4 proto {} ({} bytes)", proto, p.len()); } Vec::new() }
        }
    }

    /// BOOTP/DHCP: answer DISCOVER with OFFER and REQUEST with ACK.
    fn dhcp(&mut self, d: &[u8], src: &[u8; 6]) -> Vec<Vec<u8>> {
        if d.len() < 240 || d[0] != 1 || &d[236..240] != [0x63, 0x82, 0x53, 0x63] { return Vec::new(); }
        let mut msg_type = 0u8;
        let mut i = 240;
        while i + 1 < d.len() {
            let (opt, len) = (d[i], d[i + 1] as usize);
            if opt == 255 { break; }
            if opt == 0 { i += 1; continue; }
            if i + 2 + len > d.len() { break; }
            if opt == 53 && len == 1 { msg_type = d[i + 2]; }
            i += 2 + len;
        }
        let reply_type = match msg_type { 1 => 2, 3 => 5, _ => return Vec::new() };   // DISCOVER->OFFER, REQUEST->ACK

        let mut b = vec![0u8; 240];
        b[0] = 2; b[1] = 1; b[2] = 6;                                   // BOOTREPLY, ethernet, 6-byte MAC
        b[4..8].copy_from_slice(&d[4..8]);                              // xid
        b[10..12].copy_from_slice(&d[10..12]);                          // flags
        b[16..20].copy_from_slice(&self.sta_ip);                        // yiaddr
        b[20..24].copy_from_slice(&self.gw_ip);                         // siaddr
        b[28..34].copy_from_slice(src);                                 // chaddr
        b[236..240].copy_from_slice(&[0x63, 0x82, 0x53, 0x63]);
        b.extend_from_slice(&[53, 1, reply_type]);
        b.extend_from_slice(&[54, 4]); b.extend_from_slice(&self.gw_ip);         // server id
        b.extend_from_slice(&[51, 4, 0, 1, 0x51, 0x80]);                          // lease 86400 s
        b.extend_from_slice(&[1, 4]); b.extend_from_slice(&self.mask);
        b.extend_from_slice(&[3, 4]); b.extend_from_slice(&self.gw_ip);           // router
        b.extend_from_slice(&[6, 4]); b.extend_from_slice(&self.dns_ip);          // DNS
        b.push(255);
        while b.len() < 300 { b.push(0); }

        let mut udp = Vec::with_capacity(8 + b.len());
        udp.extend_from_slice(&67u16.to_be_bytes()); udp.extend_from_slice(&68u16.to_be_bytes());
        udp.extend_from_slice(&((8 + b.len()) as u16).to_be_bytes()); udp.extend_from_slice(&[0, 0]);   // checksum optional in IPv4
        udp.extend_from_slice(&b);

        if reply_type == 5 { self.dhcp_acks += 1; }
        if self.log { eprintln!("[net] DHCP {} -> {} for {}", if msg_type == 1 { "DISCOVER" } else { "REQUEST" },
                                if reply_type == 2 { "OFFER" } else { "ACK" },
                                format!("{}.{}.{}.{}", self.sta_ip[0], self.sta_ip[1], self.sta_ip[2], self.sta_ip[3])); }
        let ip = ip_packet(17, &self.gw_ip, &[255, 255, 255, 255], &udp);
        vec![self.frame(&[0xff; 6], 0x0800, &ip)]
    }
}

/// Build an IPv4 packet (no options) with a correct header checksum.
fn ip_packet(proto: u8, src: &[u8; 4], dst: &[u8; 4], payload: &[u8]) -> Vec<u8> {
    let total = 20 + payload.len();
    let mut h = Vec::with_capacity(total);
    h.extend_from_slice(&[0x45, 0x00]); h.extend_from_slice(&(total as u16).to_be_bytes());
    h.extend_from_slice(&[0, 0, 0x40, 0x00, 64, proto, 0, 0]);
    h.extend_from_slice(src); h.extend_from_slice(dst);
    let c = checksum(&h, 0).to_be_bytes(); h[10] = c[0]; h[11] = c[1];
    h.extend_from_slice(payload); h
}
