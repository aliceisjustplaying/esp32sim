//! The IEEE 802.15.4 MAC (`IEEE802154`, 0x600A3000), as ESP-IDF's `esp_ieee802154` driver
//! drives it: the register layout of `soc/ieee802154_struct.h`, the command byte at 0x00, the
//! event register pair at 0x60/0x64, the DMA addresses the driver points at its TX and RX
//! buffers, and enough of the TX/RX state machine for a frame to leave and arrive with the
//! timing of the air (32 µs per byte, 6 bytes of PHY overhead around the PSDU).
//!
//! What the driver (`esp_ieee802154_dev.c`, IDF 5.5) actually does, read from its source and
//! confirmed with a register trace of Contiki-NG's radio on this model:
//!
//! - **transmit**: `STOP`, `TIMER0_STOP`, `TIMER1_STOP`, read + clear events (the previous
//!   operation is torn down), then (if the PIB changed) freq/power/ED/conf writes, then
//!   `DMA_TX_ADDR` = the frame buffer (`buf[0]` = PSDU length *including* the 2-byte FCS,
//!   `buf[1..]` the MAC frame without FCS — hardware appends the CRC), and `TX_START`. The ISR
//!   then expects `TX_SFD_DONE` (ignored unless a callback is registered) and `TX_DONE`; a
//!   frame with the AR bit and auto-ack-rx on moves the driver to `RX_ACK` (stage 2).
//! - **receive**: the same teardown, `DMA_RX_ADDR` = a free 129-byte buffer, `RX_START`. On a
//!   frame the hardware raises `RX_SFD_DONE` (the ISR timestamps it with `esp_timer_get_time`),
//!   then `RX_DONE` with the buffer holding `[len incl. FCS][frame][RSSI][LQI]` — the two FCS
//!   positions carry RSSI and LQI, "crc is not written to rx buffer". The ISR reads them, hands
//!   the frame up, and re-arms RX (`set_next_rx_buffer` + `RX_START`) when `rx_when_idle` is set.
//!   Before a TX the driver reads `RX_STATUS.rx_state`: `> 1` means "a frame is coming in", and
//!   it refuses the TX (`ESP_IEEE802154_TX_ERR_ABORT`).
//! - **stop** during a frame raises `RX_ABORT` (reason `RX_STOP` = 1), which the driver clears
//!   synchronously; during a TX, `TX_ABORT` (reason `TX_STOP` = 17).
//!
//! Not modelled (stage 2 of the Cooja-NG lock-step plan): auto-ACK in either direction, CCA
//! (`CCA_TX_START` transmits as if the channel were clear), address filtering (every frame is
//! delivered, promiscuous or not), frame pending bits, security, enhanced ACKs.
//!
//! The DMA side is the bus's: the device cannot see SRAM, so it leaves a `tx_request` (fetch
//! the frame at this address) and an `rx_write` (store this buffer there) for `SocBus` to
//! carry out, which it does right after the register write and right after the device tick.
//!
//! The energy-detect scan (`ED_START`/`ED_DONE`) keeps the synthetic 2.4 GHz scene from before
//! this file existed: a quiet floor with the three non-overlapping WiFi channels on top of it,
//! moving deterministically from one xorshift (the energy-scan golden pins it).
use crate::periph::CPU_HZ;
use emu_core::ClockDomain;
use esp_periph::{Device, RegRam, WriteEffect};
use std::collections::HashSet;

pub const CMD_TX_START: u32 = 0x41;
pub const CMD_RX_START: u32 = 0x42;
pub const CMD_CCA_TX_START: u32 = 0x43;
pub const CMD_ED_START: u32 = 0x44;
pub const CMD_STOP: u32 = 0x45;
pub const CMD_TIMER0_START: u32 = 0x4c;
pub const CMD_TIMER0_STOP: u32 = 0x4d;
pub const CMD_TIMER1_START: u32 = 0x4e;
pub const CMD_TIMER1_STOP: u32 = 0x4f;

pub const EVENT_TX_DONE: u32 = 1 << 0;
pub const EVENT_RX_DONE: u32 = 1 << 1;
pub const EVENT_ACK_TX_DONE: u32 = 1 << 2;
pub const EVENT_ACK_RX_DONE: u32 = 1 << 3;
pub const EVENT_RX_ABORT: u32 = 1 << 4;
pub const EVENT_TX_ABORT: u32 = 1 << 5;
pub const EVENT_ED_DONE: u32 = 1 << 6;
pub const EVENT_TIMER0_OVERFLOW: u32 = 1 << 8;
pub const EVENT_TIMER1_OVERFLOW: u32 = 1 << 9;
pub const EVENT_TX_SFD_DONE: u32 = 1 << 11;
pub const EVENT_RX_SFD_DONE: u32 = 1 << 12;

pub const RX_ABORT_BY_RX_STOP: u32 = 1;
pub const TX_ABORT_BY_TX_STOP: u32 = 17;

/// 802.15.4 O-QPSK at 2.4 GHz: 250 kbit/s, 32 µs per byte.
pub const NS_PER_BYTE: u64 = 32_000;
/// Bytes on the air around the PSDU: 4 preamble + SFD + PHY length.
pub const PHY_OVERHEAD_BYTES: u64 = 6;
/// Preamble + SFD: what precedes the length byte, so where `*_SFD_DONE` lands after a start.
pub const SFD_BYTES: u64 = 5;
/// `IEEE802154_MAC_DATE_VERSION` in the register header.
pub const MAC_DATE: u32 = 0x22_0622;

/// Cycles of the 160 MHz CPU clock that `bytes` occupy on the air.
pub const fn air_cycles(bytes: u64) -> u64 { bytes * NS_PER_BYTE * (CPU_HZ / 1_000_000) / 1000 }

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RadioState {
    /// nothing armed (after `STOP`, after a completed TX/RX, at reset)
    Idle,
    /// `RX_START`: listening
    Rx,
    /// a frame is coming in (SFD seen, `RX_DONE` pending)
    RxFrame,
    /// `TX_START` / `CCA_TX_START`: a frame is going out
    Tx,
    /// `ED_START`: an energy scan is sampling
    Ed,
}

/// A countdown in CPU cycles. One armed from a register write is not decremented by the tick
/// that ends the round the write sat in (the round's cycles precede the write), so it starts
/// counting from the instruction after the write; one armed between rounds (a frame injected
/// by the host at an exact cycle) counts from the very next tick.
#[derive(Clone, Copy, Debug, Default)]
struct Countdown { left: Option<u64>, fresh: bool }
impl Countdown {
    fn arm(&mut self, cycles: u64, from_write: bool) { self.left = Some(cycles); self.fresh = from_write; }
    fn clear(&mut self) { self.left = None; self.fresh = false; }
    fn armed(&self) -> bool { self.left.is_some() }
    /// Advance; true if the countdown expired in this tick.
    fn tick(&mut self, cycles: u64) -> bool {
        let Some(left) = self.left else { return false };
        if self.fresh { self.fresh = false; return false; }
        if cycles >= left { self.left = None; true } else { self.left = Some(left - cycles); false }
    }
    fn remaining(&self) -> u64 { self.left.unwrap_or(u64::MAX) }
}

/// A frame received from the medium, waiting for its `RX_DONE`.
#[derive(Clone, Debug)]
pub struct RxFrame { pub frame: Vec<u8>, pub rssi: i8, pub lqi: u8 }

/// Radio timer 0/1: microsecond counters (the driver programs thresholds in µs) that raise
/// `TIMERn_OVERFLOW` at the threshold.
#[derive(Clone, Copy, Debug, Default)]
struct RadioTimer { running: bool, threshold: u32, cycles: u64 }
impl RadioTimer {
    const CYCLES_PER_US: u64 = CPU_HZ / 1_000_000;
    fn value(&self) -> u32 { (self.cycles / Self::CYCLES_PER_US) as u32 }
    fn start(&mut self) { self.running = true; self.cycles = 0; }
    fn stop(&mut self) { self.running = false; }
    /// Advance; true if the threshold was reached in this tick (the timer then stops).
    fn tick(&mut self, cycles: u64) -> bool {
        if !self.running { return false; }
        self.cycles += cycles;
        if self.value() >= self.threshold { self.running = false; true } else { false }
    }
    fn deadline(&self) -> Option<u64> {
        if !self.running { return None; }
        Some(((self.threshold as u64) * Self::CYCLES_PER_US).saturating_sub(self.cycles).max(1))
    }
}

pub struct Ieee802154 {
    ram: RegRam,
    pub state: RadioState,
    pub events: u32, pub event_en: u32,
    pub freq: u32, pub txpower: u32, pub duration: u32,
    pub dma_tx_addr: u32, pub dma_rx_addr: u32,
    pub rx_length: u32,
    rx_abort_reason: u32, tx_abort_reason: u32,
    // ---- TX
    /// `TX_START` was written: the bus fetches the frame at this address and calls `tx_loaded`
    pub tx_request: Option<u32>,
    /// the MAC frame (no length byte, no FCS) going out, kept until `TX_DONE`
    pub tx_frame: Vec<u8>,
    tx_sfd: Countdown, tx_done: Countdown,
    /// a transmission started at the instruction that just executed: the host must see it now
    pub tx_started: bool,
    pub tx_count: u64,
    // ---- RX
    rx_in_flight: Option<RxFrame>,
    rx_done: Countdown,
    /// the completed frame's buffer, for the bus to store at `.0`
    pub rx_write: Option<(u32, Vec<u8>)>,
    pub rx_count: u64,
    /// frames the medium offered while the radio was not listening (or mid-frame)
    pub rx_dropped: u64,
    timer: [RadioTimer; 2],
    // ---- energy detect (the synthetic scene)
    pub ed_rss: i8, pub ed_left: Option<u64>, pub scans: u64,
    /// dBm per 802.15.4 channel 11..26, before noise
    pub level_dbm: [i8; 16],
    /// where each channel is drifting to, 1 dB per 100 ms
    pub target_dbm: [i8; 16],
    noise: u32, scene_acc: u64, scene_ticks: u64,
    // ---- diagnostics
    /// `--debug ieee802154`: narrate commands, events and DMA
    pub dbg: bool,
    /// `--log-periph`: report the first touch of an offset the model does not interpret
    pub log_unknown: bool,
    seen: HashSet<(u32, bool)>,
}
impl Default for Ieee802154 { fn default() -> Self { Self::new() } }

impl Ieee802154 {
    pub fn new() -> Self {
        // WiFi channels 1, 6 and 11 overlap 802.15.4 channels 11-14, 16-19 and 21-24
        let mut level_dbm = [-93i8; 16];
        for (i, l) in [(0, -68), (1, -52), (2, -49), (3, -66), (5, -63), (6, -47), (7, -45), (8, -60), (10, -74), (11, -56), (12, -58), (13, -71), (15, -90)] { level_dbm[i] = l; }
        Ieee802154 {
            ram: RegRam::new(), state: RadioState::Idle, events: 0, event_en: 0, freq: 3, txpower: 0, duration: 0,
            dma_tx_addr: 0, dma_rx_addr: 0, rx_length: 0, rx_abort_reason: 0, tx_abort_reason: 0,
            tx_request: None, tx_frame: Vec::new(), tx_sfd: Countdown::default(), tx_done: Countdown::default(), tx_started: false, tx_count: 0,
            rx_in_flight: None, rx_done: Countdown::default(), rx_write: None, rx_count: 0, rx_dropped: 0,
            timer: [RadioTimer::default(); 2],
            ed_rss: -92, ed_left: None, scans: 0, level_dbm, target_dbm: level_dbm, noise: 0x9e37_79b9, scene_acc: 0, scene_ticks: 0,
            dbg: false, log_unknown: false, seen: HashSet::new(),
        }
    }

    /// the channel the frequency register selects: freq = 3 + 5 * (channel - 11)
    pub fn channel(&self) -> u8 { (11 + ((self.freq.saturating_sub(3)) / 5) as u8).clamp(11, 26) }
    /// `CONF.promiscuous`
    pub fn promiscuous(&self) -> bool { self.ram.read(0x04) & (1 << 7) != 0 }
    /// Listening, or receiving a frame: what a medium wants to know before offering one.
    pub fn listening(&self) -> bool { matches!(self.state, RadioState::Rx | RadioState::RxFrame) }
    pub fn transmitting(&self) -> bool { self.state == RadioState::Tx }

    // ------------------------------------------------------------------ energy scan scene
    pub fn set_channel_dbm(&mut self, channel: u8, dbm: i8) { if (11..=26).contains(&channel) { let i = (channel - 11) as usize; self.level_dbm[i] = dbm; self.target_dbm[i] = dbm; } }
    fn rand(&mut self) -> u32 { self.noise ^= self.noise << 13; self.noise ^= self.noise >> 17; self.noise ^= self.noise << 5; self.noise }
    /// One 100 ms step of the scene: levels drift toward their targets; every 2.5 s something
    /// happens — a WiFi network moves (channel 1, 6 or 11: 802.15.4 channels 11-14, 16-19, 21-24)
    /// or a burst lands on one channel and is left to fade back to the floor.
    fn scene_step(&mut self) {
        self.scene_ticks += 1;
        if self.scene_ticks.is_multiple_of(25) {
            let r = self.rand();
            if r.is_multiple_of(4) {
                let ch = (self.rand() % 16) as usize; self.target_dbm[ch] = self.target_dbm[ch].max(-50 - (self.rand() % 12) as i8); self.level_dbm[ch] = self.target_dbm[ch];
            } else {
                let base = [0usize, 5, 10][(r / 4 % 3) as usize];
                let peak = -44 - (self.rand() % 28) as i8;
                for (k, drop) in [(0, 6), (1, 0), (2, 2), (3, 9)] { self.target_dbm[base + k] = peak - drop; }
            }
        }
        for i in 0..16 {
            let wifi = matches!(i, 0..=3 | 5..=8 | 10..=13);
            if !wifi && self.target_dbm[i] > -93 && self.scene_ticks.is_multiple_of(5) { self.target_dbm[i] -= 1; }   // a burst fades
            let (l, t) = (self.level_dbm[i], self.target_dbm[i]);
            self.level_dbm[i] = if l < t { l + 1 } else if l > t { l - 1 } else { l };
        }
    }
    fn sample(&mut self) -> i8 {
        let jitter = (self.rand() % 7) as i8 - 3;
        (self.level_dbm[(self.channel() - 11) as usize] as i16 + jitter as i16).clamp(-127, 0) as i8
    }

    // ------------------------------------------------------------------ the medium's side
    /// A frame (MAC header + payload, no FCS) from the medium. With `on_air` its SFD is seen now
    /// and `RX_DONE` follows after the air time of the PSDU (the medium reported the start of
    /// the frame); without, the frame is complete now — SFD and `RX_DONE` together, the buffer
    /// written at once (the medium reported its end, as Cooja-NG's does). Dropped, and counted,
    /// unless the radio is listening and between frames. Returns whether it was taken.
    pub fn receive(&mut self, frame: &[u8], rssi: i8, lqi: u8, on_air: bool) -> bool {
        if self.state != RadioState::Rx || frame.is_empty() || frame.len() > 125 {
            self.rx_dropped += 1;
            if self.dbg { eprintln!("[802.15.4] rx of {} bytes dropped: state {:?}", frame.len(), self.state); }
            return false;
        }
        self.state = RadioState::RxFrame;
        self.rx_in_flight = Some(RxFrame { frame: frame.to_vec(), rssi, lqi });
        self.events |= EVENT_RX_SFD_DONE;
        if on_air {
            // the PHY header and the SFD are before this point on the air: what is left is the length byte, the PSDU and the FCS
            self.rx_done.arm(air_cycles(1 + frame.len() as u64 + 2), false);
            if self.dbg { eprintln!("[802.15.4] rx {} bytes rssi {} lqi {}: SFD now, RX_DONE in {} cycles", frame.len(), rssi, lqi, self.rx_done.remaining()); }
        } else {
            if self.dbg { eprintln!("[802.15.4] rx {} bytes rssi {} lqi {}: complete now", frame.len(), rssi, lqi); }
            self.finish_rx();
        }
        true
    }

    /// The frame in flight is complete: lay it out as the driver reads it, raise `RX_DONE`.
    fn finish_rx(&mut self) {
        if let Some(rx) = self.rx_in_flight.take() {
            let mut buf = Vec::with_capacity(rx.frame.len() + 3);
            buf.push((rx.frame.len() + 2) as u8);
            buf.extend_from_slice(&rx.frame);
            buf.push(rx.rssi as u8);
            buf.push(rx.lqi);
            self.rx_length = (rx.frame.len() + 2) as u32;
            self.rx_write = Some((self.dma_rx_addr, buf));
            self.rx_count += 1;
            self.events |= EVENT_RX_DONE;
            if self.dbg { eprintln!("[802.15.4] RX_DONE: {} bytes to {:#010x}", rx.frame.len(), self.dma_rx_addr); }
        }
        if self.state == RadioState::RxFrame { self.state = RadioState::Idle; }
    }

    /// The bus fetched the frame `TX_START` pointed at: `psdu` is the MAC frame without its
    /// length byte and without the FCS (the hardware appends the CRC). Starts the air-time
    /// clock and flags the start for the host.
    pub fn tx_loaded(&mut self, psdu: Vec<u8>) {
        self.tx_sfd.arm(air_cycles(SFD_BYTES), true);
        self.tx_done.arm(air_cycles(PHY_OVERHEAD_BYTES + psdu.len() as u64 + 2), true);
        if self.dbg { eprintln!("[802.15.4] tx {} bytes on channel {}: TX_DONE in {} cycles", psdu.len(), self.channel(), self.tx_done.remaining()); }
        self.tx_frame = psdu;
        self.tx_started = true;
        self.tx_count += 1;
    }

    /// The host's yield flag: a transmission began at the instruction that just ran.
    pub fn take_tx_started(&mut self) -> bool { std::mem::take(&mut self.tx_started) }

    // ------------------------------------------------------------------ commands
    fn command(&mut self, cmd: u32) {
        if self.dbg { eprintln!("[802.15.4] cmd {:#04x} in state {:?}", cmd, self.state); }
        match cmd {
            CMD_TX_START | CMD_CCA_TX_START => {
                self.abort_rx_silently();
                self.state = RadioState::Tx;
                self.tx_request = Some(self.dma_tx_addr);
            }
            CMD_RX_START => { self.abort_rx_silently(); self.state = RadioState::Rx; }
            CMD_ED_START => {
                self.state = RadioState::Ed;
                self.ed_left = Some((self.duration.max(1) as u64) * 16 * (CPU_HZ / 1_000_000));   // symbols of 16 µs
            }
            CMD_STOP => {
                if self.state == RadioState::RxFrame { self.rx_abort_reason = RX_ABORT_BY_RX_STOP; self.events |= EVENT_RX_ABORT; }
                if self.state == RadioState::Tx && self.tx_done.armed() { self.tx_abort_reason = TX_ABORT_BY_TX_STOP; self.events |= EVENT_TX_ABORT; }
                self.abort_rx_silently();
                self.tx_sfd.clear(); self.tx_done.clear();
                self.ed_left = None;
                self.state = RadioState::Idle;
            }
            CMD_TIMER0_START => { self.timer[0].start(); }
            CMD_TIMER0_STOP => { self.timer[0].stop(); }
            CMD_TIMER1_START => { self.timer[1].start(); }
            CMD_TIMER1_STOP => { self.timer[1].stop(); }
            _ => { if self.dbg || self.log_unknown { eprintln!("[802.15.4] unsupported command {:#04x}", cmd); } }
        }
    }
    fn abort_rx_silently(&mut self) {
        if self.rx_in_flight.take().is_some() { self.rx_dropped += 1; }
        self.rx_done.clear();
    }

    fn note_unknown(&mut self, off: u32, write: bool, v: u32) {
        if !(self.dbg || self.log_unknown) { return; }
        if self.seen.insert((off, write)) {
            eprintln!("[802.15.4] {} unmodelled register +0x{:03x}{}", if write { "W" } else { "R" }, off, if write { format!(" = {:#x}", v) } else { String::new() });
        }
    }

    fn rx_status(&self) -> u32 {
        let rx_state = match self.state { RadioState::Rx => 1, RadioState::RxFrame => 2, _ => 0 };
        let sync = if self.state == RadioState::RxFrame { (1 << 20) | (1 << 21) } else { 0 };
        (self.rx_abort_reason & 0x1f) << 4 | rx_state << 16 | sync
    }
    fn tx_status(&self) -> u32 {
        let tx_state = if self.state == RadioState::Tx { 1 } else { 0 };
        tx_state | (self.tx_abort_reason & 0x1f) << 4
    }
    fn txrx_status(&self) -> u32 {
        let (tx, rx, ed) = (self.state == RadioState::Tx, self.listening(), self.state == RadioState::Ed);
        (tx as u32) << 8 | (rx as u32) << 9 | (ed as u32) << 10
    }
}

impl Device for Ieee802154 {
    fn read(&mut self, off: u32) -> u32 {
        match off {
            0x00 => 0,
            0x48 => self.freq, 0x4c => self.txpower, 0x50 => self.duration,
            0x54 => (self.ram.read(off) & !0x00ff_0000) | ((self.ed_rss as u8 as u32) << 16),   // ED_SCAN_CFG.ED_RSS; CCA_BUSY stays clear
            0x60 => self.event_en, 0x64 => self.events,
            0x80 => self.rx_status(), 0x84 => self.tx_status(), 0x88 => self.txrx_status(),
            0xa4 => self.rx_length,
            0xa8 => self.timer[0].threshold, 0xac => self.timer[0].value(),
            0xb0 => self.timer[1].threshold, 0xb4 => self.timer[1].value(),
            0xd0 => self.dma_tx_addr, 0xe0 => self.dma_rx_addr,
            0x184 => MAC_DATE,
            // configuration the driver writes and reads back: CONF, the multipan addresses, IFS,
            // ACK timeout, abort enables, pending, PTI, DMA config, the delay table, security
            0x04 | 0x08..=0x44 | 0x58 | 0x5c | 0x68 | 0x6c | 0x70 | 0x78 | 0x7c | 0x90 | 0xb8 | 0xbc | 0xc0..=0xcc | 0xd4 | 0xd8 | 0xe4 | 0xe8 | 0xf0 | 0xf4 | 0x100..=0x120 | 0x128..=0x140 => self.ram.read(off),
            0x144..=0x180 => 0,                                                                  // debug counters
            _ => { self.note_unknown(off, false, 0); self.ram.read(off) }
        }
    }
    fn write(&mut self, off: u32, v: u32) -> WriteEffect {
        match off {
            0x00 => self.command(v & 0xff),
            0x48 => self.freq = v & 0x7f, 0x4c => self.txpower = v & 0x1f, 0x50 => self.duration = v & 0xff_ffff,
            0x54 => self.ram.write(off, v & 0x0000_ffff),                                         // ED_RSS / CCA_BUSY are read-only
            0x60 => self.event_en = v & 0x1fff, 0x64 => self.events &= !v,                          // EVENT_STATUS: write 1 to clear
            0xa8 => self.timer[0].threshold = v, 0xb0 => self.timer[1].threshold = v,
            0xd0 => self.dma_tx_addr = v, 0xe0 => self.dma_rx_addr = v,
            0x04 | 0x08..=0x44 | 0x58 | 0x5c | 0x68 | 0x6c | 0x70 | 0x78 | 0x7c | 0x90 | 0xb8 | 0xbc | 0xc0..=0xcc | 0xd4 | 0xd8 | 0xe4 | 0xe8 | 0xf0 | 0xf4 | 0x100..=0x120 | 0x128..=0x140 | 0x180 => self.ram.write(off, v),
            _ => { self.note_unknown(off, true, v); self.ram.write(off, v) }
        }
        WriteEffect::NONE
    }
    fn irq_sources(&self) -> u64 { (self.events & self.event_en != 0) as u64 }
    fn clock(&self) -> Option<ClockDomain> { Some(ClockDomain::Cpu) }
    fn tick(&mut self, cycles: u64) {
        self.scene_acc += cycles;
        while self.scene_acc >= CPU_HZ / 10 { self.scene_acc -= CPU_HZ / 10; self.scene_step(); }
        if let Some(left) = self.ed_left {
            if cycles >= left { self.ed_left = None; self.ed_rss = self.sample(); self.scans += 1; self.events |= EVENT_ED_DONE; if self.state == RadioState::Ed { self.state = RadioState::Idle; } } else { self.ed_left = Some(left - cycles); }
        }
        if self.tx_sfd.tick(cycles) { self.events |= EVENT_TX_SFD_DONE; }
        if self.tx_done.tick(cycles) {
            self.events |= EVENT_TX_DONE;
            if self.state == RadioState::Tx { self.state = RadioState::Idle; }
            if self.dbg { eprintln!("[802.15.4] TX_DONE"); }
        }
        if self.rx_done.tick(cycles) { self.finish_rx(); }
        if self.timer[0].tick(cycles) { self.events |= EVENT_TIMER0_OVERFLOW; }
        if self.timer[1].tick(cycles) { self.events |= EVENT_TIMER1_OVERFLOW; }
    }
    fn has_deadline(&self) -> bool { true }
    /// The scene only matters to a scan in progress, so it is not a deadline on its own: an idle
    /// radio must not wake the machine every 100 ms.
    fn next_deadline(&self) -> Option<u64> {
        let mut best = u64::MAX;
        if let Some(l) = self.ed_left { best = best.min(l).min(CPU_HZ / 10 - self.scene_acc); }
        for c in [&self.tx_sfd, &self.tx_done, &self.rx_done] { best = best.min(c.remaining()); }
        for t in &self.timer { if let Some(d) = t.deadline() { best = best.min(d); } }
        Some(best)
    }
    fn debug(&mut self, on: bool) { self.dbg = on; }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn armed_rx() -> Ieee802154 {
        let mut r = Ieee802154::new();
        r.write(0x60, 0x1fff); r.write(0xe0, 0x4081_0000); r.write(0x00, CMD_RX_START);
        r
    }

    /// A frame offered while listening: SFD at once, RX_DONE exactly one PSDU air time later,
    /// the buffer laid out as the driver reads it (length incl. FCS, frame, RSSI, LQI).
    #[test]
    fn rx_lands_after_its_air_time() {
        let mut r = armed_rx();
        assert!(r.receive(&[0x41, 0xc8, 0x05, 0xcd, 0xab], -60, 200, true));
        assert_eq!(r.events & EVENT_RX_SFD_DONE, EVENT_RX_SFD_DONE);
        assert_eq!(r.read(0x80) >> 16 & 7, 2, "rx_state says a frame is coming in");
        let air = air_cycles(1 + 5 + 2);
        r.tick(air - 1);
        assert_eq!(r.events & EVENT_RX_DONE, 0);
        r.tick(1);
        assert_eq!(r.events & EVENT_RX_DONE, EVENT_RX_DONE);
        let (addr, buf) = r.rx_write.take().unwrap();
        assert_eq!(addr, 0x4081_0000);
        assert_eq!(buf, vec![7, 0x41, 0xc8, 0x05, 0xcd, 0xab, (-60i8) as u8, 200]);
        assert_eq!(r.read(0xa4), 7);
        assert_eq!(r.state, RadioState::Idle, "the driver re-arms RX itself");
    }

    /// Not listening: the frame is dropped and counted, nothing is raised.
    #[test]
    fn rx_while_idle_is_dropped() {
        let mut r = Ieee802154::new();
        assert!(!r.receive(&[1, 2, 3], -60, 200, true));
        assert_eq!((r.rx_dropped, r.events), (1, 0));
        let mut r = armed_rx();
        assert!(r.receive(&[1, 2, 3], -60, 200, true));
        assert!(!r.receive(&[4, 5, 6], -60, 200, true), "a second frame collides with the one in flight");
        assert_eq!(r.rx_dropped, 1);
    }

    /// TX_START asks the bus for the frame; once loaded, SFD and DONE follow the air. The tick
    /// that closes the round of the write does not count (it precedes the write).
    #[test]
    fn tx_start_requests_the_frame_then_times_the_air() {
        let mut r = Ieee802154::new();
        r.write(0x60, 0x1fff); r.write(0xd0, 0x4081_6150); r.write(0x00, CMD_TX_START);
        assert_eq!(r.tx_request.take(), Some(0x4081_6150));
        assert_eq!(r.state, RadioState::Tx);
        r.tx_loaded(vec![0u8; 17]);
        assert!(r.take_tx_started() && !r.take_tx_started());
        r.tick(40);                                   // the round the write sat in
        assert_eq!(r.tx_done.remaining(), air_cycles(6 + 17 + 2));
        r.tick(air_cycles(SFD_BYTES));
        assert_eq!(r.events & EVENT_TX_SFD_DONE, EVENT_TX_SFD_DONE);
        r.tick(air_cycles(6 + 17 + 2) - air_cycles(SFD_BYTES) - 1);
        assert_eq!(r.events & EVENT_TX_DONE, 0);
        r.tick(1);
        assert_eq!(r.events & EVENT_TX_DONE, EVENT_TX_DONE);
        assert_eq!(r.state, RadioState::Idle);
        assert_eq!(r.read(0x64), EVENT_TX_SFD_DONE | EVENT_TX_DONE);
        r.write(0x64, EVENT_TX_DONE);
        assert_eq!(r.read(0x64), EVENT_TX_SFD_DONE);
    }

    /// STOP mid-frame raises RX_ABORT with the RX_STOP reason, as the driver clears it.
    #[test]
    fn stop_aborts_a_frame_in_flight() {
        let mut r = armed_rx();
        r.receive(&[1, 2, 3], -60, 200, true);
        r.write(0x00, CMD_STOP);
        assert_eq!(r.events & EVENT_RX_ABORT, EVENT_RX_ABORT);
        assert_eq!(r.read(0x80) >> 4 & 0x1f, RX_ABORT_BY_RX_STOP);
        assert_eq!(r.state, RadioState::Idle);
        r.tick(air_cycles(100));
        assert_eq!(r.events & EVENT_RX_DONE, 0, "no RX_DONE after a stop");
    }

    /// The deadline is the nearest of what is armed, and nothing when the radio is quiet.
    #[test]
    fn deadline_follows_the_armed_countdowns() {
        let mut r = Ieee802154::new();
        assert_eq!(r.next_deadline(), Some(u64::MAX));
        r.write(0xa8, 100); r.write(0x00, CMD_TIMER0_START);
        assert_eq!(r.next_deadline(), Some(100 * 160));
        r.tick(50 * 160);
        assert_eq!(r.read(0xac), 50);
        r.tick(50 * 160);
        assert_eq!(r.events & EVENT_TIMER0_OVERFLOW, EVENT_TIMER0_OVERFLOW);
        assert_eq!(r.next_deadline(), Some(u64::MAX));
    }

    /// A frame the medium reports at its end (Cooja-NG's delivery): SFD and RX_DONE together,
    /// the buffer ready at once, nothing left to count down.
    #[test]
    fn rx_reported_at_its_end_completes_at_once() {
        let mut r = armed_rx();
        assert!(r.receive(&[0x41, 0xc8, 0x05], -50, 255, false));
        assert_eq!(r.events & (EVENT_RX_SFD_DONE | EVENT_RX_DONE), EVENT_RX_SFD_DONE | EVENT_RX_DONE);
        assert_eq!(r.rx_write.take().unwrap().1, vec![5, 0x41, 0xc8, 0x05, (-50i8) as u8, 255]);
        assert_eq!(r.state, RadioState::Idle);
        assert_eq!(r.next_deadline(), Some(u64::MAX));
    }

    #[test]
    fn channel_from_frequency() {
        let mut r = Ieee802154::new();
        r.write(0x48, 78);
        assert_eq!(r.channel(), 26);
        r.write(0x48, 3);
        assert_eq!(r.channel(), 11);
    }
}
