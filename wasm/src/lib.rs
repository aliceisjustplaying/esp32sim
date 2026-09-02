//! The emulator as a WebAssembly module. A C ABI, hand-driven from `web/wasm/worker.js` — no
//! bindgen, no dependencies. The machine produces exactly the messages the WebSocket UI speaks
//! (`docs/web-ui.md`); here they are queued in a `WebServer::queued()` sink and handed to JS.
//!
//! Lifecycle: `esp32sim_new` → `esp32sim_load` (ROM, bootloader, partition table, app, ELF
//! symbols, script) → optional `esp32sim_wifi` → `esp32sim_boot` → repeated `esp32sim_run(cycles,
//! unix_ms)` with `esp32sim_out_*` draining the outbox after each slice and `esp32sim_in_*`
//! feeding the page's inputs.
use esp_soc::observers::{BlockProfile, Coverage, IrqLatency};
use esp_soc::web::{json_escape, WebServer};
use esp_soc::{Machine, Soc, SocBus, Stop};
use std::any::Any;

/// The dozen calls the ABI makes, over whichever chip this instance is.
trait MachineApi {
    fn load(&mut self, kind: u32, d: &[u8], txt: &str) -> Result<(), String>;
    fn write_flash(&mut self, off: usize, d: &[u8]) -> Result<(), String>;
    fn boot(&mut self, app_direct: bool) -> Result<(), String>;
    fn board_name(&self) -> String;
    fn web(&self) -> Option<&WebServer>;
    fn run_slice(&mut self, cycles: u32) -> u32;
    fn cpu_hz(&self) -> f64;
    fn cycles(&self) -> f64;
    fn insns(&self) -> f64;
    fn stub(&mut self, name: &str, value: u32) -> u32;
    fn observer(&mut self, name: &str, arg: &str) -> u32;
    fn reports(&mut self) -> String;
    fn as_any_mut(&mut self) -> &mut dyn Any;
}

impl<S: Soc> MachineApi for Machine<S> {
    fn load(&mut self, kind: u32, d: &[u8], txt: &str) -> Result<(), String> {
        match kind {
            0 => self.load_rom(d),
            1 => self.write_flash(0, d),
            2 => self.write_flash(0x8000, d),
            3 => self.write_flash(0x10000, d),
            4 => self.add_symbols(d),
            5 => self.write_flash(0, d),
            6 => self.load_script(txt),
            7 => esp_soc::picture::parse(d).map(|p| self.bus.board().set_camera_picture(p)),
            _ => Err(format!("unknown load kind {}", kind)),
        }
    }
    fn write_flash(&mut self, off: usize, d: &[u8]) -> Result<(), String> {
        Machine::write_flash(self, off, d)
    }
    fn boot(&mut self, app_direct: bool) -> Result<(), String> {
        if app_direct {
            self.boot_app(0x10000).map(|_| ())
        } else {
            self.boot_rom();
            Ok(())
        }
    }
    fn board_name(&self) -> String {
        self.bus.board_ref().name().to_string()
    }
    fn web(&self) -> Option<&WebServer> {
        self.web.as_ref()
    }
    fn run_slice(&mut self, cycles: u32) -> u32 {
        self.max_cycles = self.bus.cycles() + cycles as u64;
        loop {
            match self.run(u64::MAX) {
                Stop::Halted | Stop::MaxInsns => return 0,
                Stop::SwReset => {
                    let cause = self.bus.reset_cause();
                    let note = format!(
                        "[emu] chip reset at t={:.3}s: cause {:#x} ({})",
                        self.seconds(),
                        cause,
                        esp_periph::reset_cause_name(cause)
                    );
                    log(&note);
                    if let Some(w) = &self.web {
                        w.send_text(&format!(
                            "{{\"t\":\"emu\",\"msg\":\"{}\"}}",
                            json_escape(&note)
                        ));
                    }
                    self.reboot();
                }
                Stop::Unimplemented(pc, raw) => {
                    log(&format!(
                        "[emu] unimplemented instruction at {:08x} {} (raw {:#x})",
                        pc,
                        self.sym(pc),
                        raw
                    ));
                    return 2;
                }
                Stop::Ebreak(pc) => {
                    log(&format!("[emu] ebreak at {:08x} {}", pc, self.sym(pc)));
                    return 3;
                }
                Stop::Breakpoint(_) => return 3,
                Stop::Exceptions(_) => return 4,
                Stop::Simcall(_) => return 5,
                Stop::Watch(..) => return 6,
            }
        }
    }
    fn cpu_hz(&self) -> f64 {
        S::CPU_HZ as f64
    }
    fn cycles(&self) -> f64 {
        self.bus.cycles() as f64
    }
    fn insns(&self) -> f64 {
        Machine::insns(self) as f64
    }
    fn stub(&mut self, name: &str, value: u32) -> u32 {
        let by_addr = name
            .strip_prefix("0x")
            .and_then(|h| u32::from_str_radix(h, 16).ok());
        match by_addr.or_else(|| self.sym_addr(name)) {
            Some(addr) => {
                self.stubs.insert(addr, value);
                log(&format!(
                    "[emu] stub {} @ {:#x} -> returns {:#x}",
                    name, addr, value
                ));
                0
            }
            None => {
                log(&format!(
                    "[emu] stub: no symbol '{}' (load the app ELF first)",
                    name
                ));
                1
            }
        }
    }
    fn observer(&mut self, name: &str, arg: &str) -> u32 {
        match name {
            "profile-blocks" => {
                self.add_observer(Box::new(BlockProfile::new(20)));
                0
            }
            "coverage" => {
                self.add_observer(Box::new(Coverage::new(None)));
                0
            }
            "irq-latency" => {
                self.add_observer(Box::new(IrqLatency::new(S::CORES)));
                0
            }
            "trace-fn" => {
                let n: Vec<(u32, String)> = self
                    .symbols
                    .iter()
                    .filter(|(_, s)| s.starts_with(arg))
                    .map(|(a, s)| (*a, s.clone()))
                    .collect();
                for (a, s) in n {
                    self.fn_probes.insert(a, s);
                }
                0
            }
            _ => {
                log(&format!("[emu] unknown observer '{}'", name));
                1
            }
        }
    }
    fn reports(&mut self) -> String {
        Machine::reports(self)
    }
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

#[cfg(target_arch = "wasm32")]
#[link(wasm_import_module = "env")]
extern "C" {
    fn host_log(ptr: *const u8, len: usize);
}
#[cfg(not(target_arch = "wasm32"))]
#[expect(
    unsafe_code,
    reason = "the native logging shim receives a raw ABI string"
)]
unsafe fn host_log(ptr: *const u8, len: usize) {
    // SAFETY: The caller supplies a readable string pointer for exactly `len` bytes and retains it
    // for this call.
    let message = unsafe { std::slice::from_raw_parts(ptr, len) };
    eprintln!("{}", String::from_utf8_lossy(message));
}

#[expect(unsafe_code, reason = "host logging crosses the imported native ABI")]
fn log(s: &str) {
    // SAFETY: `s` supplies a valid pointer and length for the call, and the host does not retain it.
    unsafe { host_log(s.as_ptr(), s.len()) };
}

pub struct Emu {
    m: Box<dyn MachineApi>,
    /// the last drained outbox: (1 text | 2 binary, payload), addressed by index from JS
    out: Vec<(u8, Vec<u8>)>,
    booted: bool,
}

#[expect(
    unsafe_code,
    reason = "WASM callers pass buffers as pointer-length pairs"
)]
unsafe fn bytes<'a>(ptr: *const u8, len: usize) -> &'a [u8] {
    if len == 0 {
        &[]
    } else {
        // SAFETY: The caller guarantees that `ptr` is readable for `len` bytes and that the
        // returned borrow does not outlive the allocation.
        unsafe { std::slice::from_raw_parts(ptr, len) }
    }
}
#[expect(unsafe_code, reason = "WASM callers pass text as pointer-length pairs")]
unsafe fn text<'a>(ptr: *const u8, len: usize) -> &'a str {
    // SAFETY: This function forwards its pointer and lifetime contract to `bytes`.
    let data = unsafe { bytes(ptr, len) };
    std::str::from_utf8(data).unwrap_or("")
}

/// Buffers the page fills before handing them to `esp32sim_load` / `esp32sim_in_*`.
#[expect(unsafe_code, reason = "the WASM interface exports a stable ABI symbol")]
#[no_mangle]
pub extern "C" fn esp32sim_alloc(len: usize) -> *mut u8 {
    Vec::leak(vec![0u8; len.max(1)]).as_mut_ptr()
}
/// Release a buffer returned by `esp32sim_alloc`.
///
/// # Safety
/// `ptr` must come from `esp32sim_alloc(len)` and must not be used after this call.
#[expect(
    unsafe_code,
    reason = "the WASM interface reclaims an ABI-owned allocation"
)]
#[no_mangle]
pub unsafe extern "C" fn esp32sim_free(ptr: *mut u8, len: usize) {
    // SAFETY: The caller guarantees this pointer and length came from `esp32sim_alloc` and still
    // have unique ownership.
    drop(unsafe { Vec::from_raw_parts(ptr, len.max(1), len.max(1)) });
}

/// `board` is a CLI board name (atech14, waveshare-cam, waveshare-lcd4b, none) for the ESP32-S3,
/// or `esp32c3` for the RISC-V chip, which is console-only and takes no board. Null on failure.
///
/// # Safety
/// `board` must be readable for `board_len` bytes and remain valid for this call.
#[expect(
    unsafe_code,
    reason = "the WASM constructor reads an ABI pointer-length pair"
)]
#[no_mangle]
pub unsafe extern "C" fn esp32sim_new(
    board: *const u8,
    board_len: usize,
    flash_mb: u32,
    psram_mb: u32,
) -> *mut Emu {
    std::panic::set_hook(Box::new(|info| log(&format!("[emu] panic: {}", info))));
    // SAFETY: The caller guarantees that the board name is readable for this call.
    let board = unsafe { text(board, board_len) }.to_string();
    let (flash_mb, psram_mb) = (flash_mb.max(1) as usize, psram_mb as usize);
    let m: Box<dyn MachineApi> = if board == "esp32c3" || board == "c3" {
        let mut m = esp32c3::machine([0x3c, 0x84, 0x27, 0xb6, 0xa7, 0x1c], flash_mb << 20);
        m.bus.set_flash_size(flash_mb << 20);
        m.console.mask = 2; // the ROM mirrors its console to UART0 and USB-Serial/JTAG
        prepare(&mut m);
        Box::new(m)
    } else {
        let mut m = esp32s3::machine([0x44, 0x1b, 0xf6, 0x75, 0xdc, 0xe0]);
        let Some(b) = esp32s3::board::make_board(&board) else {
            log(&format!("[emu] unknown board '{}'", board));
            return std::ptr::null_mut();
        };
        m.bus.board = b;
        for (bus, addr, dev) in m.bus.board.i2c_devices() {
            m.bus.periph.i2c[bus as usize].attach(addr, dev);
        }
        m.bus.set_flash_size(flash_mb << 20);
        let _ = m.bus.set_psram_size(psram_mb << 20);
        m.bus.periph.lcd_cam.frame_cycles = esp32s3::periph::CPU_HZ / 10;
        prepare(&mut m);
        Box::new(m)
    };
    Box::into_raw(Box::new(Emu {
        m,
        out: Vec::new(),
        booted: false,
    }))
}

/// The page is the one client: messages queue in a `WebServer` sink; the worker paces the run.
fn prepare<S: Soc>(m: &mut Machine<S>) {
    m.web = Some(WebServer::queued());
    m.rt.enabled = false; // std::time does not exist here
    m.console.capture = true;
}

/// Destroy an emulator returned by `esp32sim_new`.
///
/// # Safety
/// A non-null `e` must come from `esp32sim_new` and must not be used after this call.
#[expect(
    unsafe_code,
    reason = "the WASM destructor reclaims an ABI-owned allocation"
)]
#[no_mangle]
pub unsafe extern "C" fn esp32sim_delete(e: *mut Emu) {
    if !e.is_null() {
        // SAFETY: The caller guarantees unique ownership of a pointer returned by `esp32sim_new`.
        drop(unsafe { Box::from_raw(e) });
    }
}

/// kind: 0 mask-ROM ELF, 1 bootloader (flash 0x0), 2 partition table (0x8000), 3 app (0x10000),
/// 4 ELF for symbols, 5 whole flash image (0x0), 6 script text, 7 camera picture (BMP/PPM).
/// Returns 0, or 1 with the reason logged.
///
/// # Safety
/// `e` must identify a live emulator with exclusive access. `ptr` must be readable for `len`
/// bytes and remain valid for this call.
#[expect(
    unsafe_code,
    reason = "the WASM loader receives emulator and buffer ABI pointers"
)]
#[no_mangle]
pub unsafe extern "C" fn esp32sim_load(e: *mut Emu, kind: u32, ptr: *const u8, len: usize) -> u32 {
    // SAFETY: The caller guarantees a live, exclusively accessible emulator.
    let e = unsafe { &mut *e };
    // SAFETY: The caller guarantees a readable input buffer for this call.
    let data = unsafe { bytes(ptr, len) };
    let input_text = std::str::from_utf8(data).unwrap_or("");
    match e.m.load(kind, data, input_text) {
        Ok(()) => 0,
        Err(msg) => {
            log(&format!("[emu] load kind {}: {}", kind, msg));
            1
        }
    }
}

/// Write bytes into flash at an arbitrary offset (a data partition's contents).
///
/// # Safety
/// `e` must identify a live emulator with exclusive access. `ptr` must be readable for `len`
/// bytes and remain valid for this call.
#[expect(
    unsafe_code,
    reason = "the WASM flash loader receives emulator and buffer ABI pointers"
)]
#[no_mangle]
pub unsafe extern "C" fn esp32sim_load_at(
    e: *mut Emu,
    offset: u32,
    ptr: *const u8,
    len: usize,
) -> u32 {
    // SAFETY: The caller guarantees a live, exclusively accessible emulator.
    let e = unsafe { &mut *e };
    // SAFETY: The caller guarantees a readable input buffer for this call.
    let data = unsafe { bytes(ptr, len) };
    match e.m.write_flash(offset as usize, data) {
        Ok(()) => 0,
        Err(msg) => {
            log(&format!("[emu] flash {:#x}: {}", offset, msg));
            1
        }
    }
}

/// Attach the virtual access point and subnet: `ssid=NAME,psk=PASS,chan=N`. No NAT — the browser
/// has no sockets — so DHCP, DNS, SNTP and ICMP answer, and connections past the gateway are refused.
///
/// # Safety
/// `e` must identify a live emulator with exclusive access. `spec` must be readable for `len`
/// bytes and remain valid for this call.
#[expect(
    unsafe_code,
    reason = "the WASM WiFi setup receives emulator and text ABI pointers"
)]
#[no_mangle]
pub unsafe extern "C" fn esp32sim_wifi(e: *mut Emu, spec: *const u8, len: usize) {
    // SAFETY: The caller guarantees a live, exclusively accessible emulator.
    let e = unsafe { &mut *e };
    let Some(m) = e.m.as_any_mut().downcast_mut::<esp32s3::Machine>() else {
        log("[emu] wifi: the C3 radio is not modelled");
        return;
    };
    let mut cfg = esp32s3::wifi::ApConfig {
        ssid: "esp32sim".into(),
        bssid: [0x02, 0x53, 0x49, 0x4d, 0x00, 0x01],
        channel: 6,
        psk: None,
    };
    // SAFETY: The caller guarantees a readable setup string for this call.
    let spec = unsafe { text(spec, len) };
    for kv in spec.split(',') {
        match kv.split_once('=') {
            Some(("ssid", v)) => cfg.ssid = v.to_string(),
            Some(("chan", v)) | Some(("channel", v)) => cfg.channel = v.parse().unwrap_or(6),
            Some(("psk", v)) | Some(("password", v)) => cfg.psk = Some(v.to_string()),
            _ => {}
        }
    }
    log(&format!(
        "[emu] virtual AP '{}' ({}), subnet 10.0.2.0/24, no NAT in the browser",
        cfg.ssid,
        if cfg.psk.is_some() {
            "WPA2-PSK"
        } else {
            "open"
        }
    ));
    m.bus.periph.wifi.ap = Some(esp32s3::wifi::VirtualAp::new(
        cfg,
        m.bus.debug.has("wifi-frames"),
    ));
    m.bus.periph.wifi.net = Some(esp32s3::net::VirtualNet::new(m.bus.debug.has("net")));
}

/// `--stub NAME[=value]`: return `value` immediately when execution reaches the function's entry.
/// NAME is a symbol (needs the ELF loaded) or a hex address. Returns 1 if it cannot be resolved.
///
/// # Safety
/// `e` must identify a live emulator with exclusive access. `name` must be readable for `len`
/// bytes and remain valid for this call.
#[expect(
    unsafe_code,
    reason = "the WASM stub setup receives emulator and text ABI pointers"
)]
#[no_mangle]
pub unsafe extern "C" fn esp32sim_stub(
    e: *mut Emu,
    name: *const u8,
    len: usize,
    value: u32,
) -> u32 {
    // SAFETY: The caller guarantees a live, exclusively accessible emulator.
    let e = unsafe { &mut *e };
    // SAFETY: The caller guarantees a readable symbol name for this call.
    let name = unsafe { text(name, len) };
    e.m.stub(name, value)
}

/// Attach an analysis: `profile-blocks`, `coverage`, `irq-latency` (no argument), `trace-fn`
/// (arg = symbol prefix as text). Returns 1 for an unknown name.
///
/// # Safety
/// `e` must identify a live emulator with exclusive access. `name` and `arg` must be readable for
/// their respective lengths and remain valid for this call.
#[expect(
    unsafe_code,
    reason = "the WASM observer setup receives emulator and text ABI pointers"
)]
#[no_mangle]
pub unsafe extern "C" fn esp32sim_observer(
    e: *mut Emu,
    name: *const u8,
    len: usize,
    arg: *const u8,
    arg_len: usize,
) -> u32 {
    // SAFETY: The caller guarantees a live, exclusively accessible emulator.
    let e = unsafe { &mut *e };
    // SAFETY: The caller guarantees a readable observer name for this call.
    let name = unsafe { text(name, len) };
    // SAFETY: The caller guarantees a readable observer argument for this call.
    let arg = unsafe { text(arg, arg_len) };
    e.m.observer(name, arg)
}

/// Every observer's report so far, as `emu` messages in the outbox.
///
/// # Safety
/// `e` must identify a live emulator with exclusive access for this call.
#[expect(
    unsafe_code,
    reason = "the WASM report function receives an emulator ABI pointer"
)]
#[no_mangle]
pub unsafe extern "C" fn esp32sim_reports(e: *mut Emu) {
    // SAFETY: The caller guarantees a live, exclusively accessible emulator.
    let e = unsafe { &mut *e };
    let r = e.m.reports();
    if let Some(w) = e.m.web() {
        for line in r.lines() {
            w.send_text(&format!(
                "{{\"t\":\"emu\",\"msg\":\"{}\"}}",
                json_escape(line)
            ));
        }
    }
}

/// Start from the mask ROM (the normal path) or, with `app_direct` set, straight into the app image.
///
/// # Safety
/// `e` must identify a live emulator with exclusive access for this call.
#[expect(
    unsafe_code,
    reason = "the WASM boot function receives an emulator ABI pointer"
)]
#[no_mangle]
pub unsafe extern "C" fn esp32sim_boot(e: *mut Emu, app_direct: u32) -> u32 {
    // SAFETY: The caller guarantees a live, exclusively accessible emulator.
    let e = unsafe { &mut *e };
    if let Err(msg) = e.m.boot(app_direct != 0) {
        log(&format!("[emu] boot: {}", msg));
        return 1;
    }
    // the WebSocket server announces the board in its per-client hello; here there is one client
    let name = e.m.board_name();
    if let Some(w) = e.m.web() {
        w.send_text(&format!("{{\"t\":\"board\",\"name\":\"{}\"}}", name));
    }
    e.booted = true;
    0
}

/// Run for `cycles` more emulated cycles. Returns 0 while the machine can go on; otherwise a stop
/// code: 2 unimplemented instruction, 3 breakpoint/ebreak, 4 exception limit, 5 semihosting call.
/// A chip reset (esp_restart, watchdog) reboots through the ROM and keeps going, like the CLI.
///
/// # Safety
/// `e` must identify a live emulator with exclusive access for this call.
#[expect(
    unsafe_code,
    reason = "the WASM run function receives an emulator ABI pointer"
)]
#[no_mangle]
pub unsafe extern "C" fn esp32sim_run(e: *mut Emu, cycles: u32, unix_ms: f64) -> u32 {
    // SAFETY: The caller guarantees a live, exclusively accessible emulator.
    let e = unsafe { &mut *e };
    if !e.booted {
        return 9;
    }
    #[cfg(target_arch = "wasm32")]
    esp_soc::host::set_unix_time_ms(unix_ms as u64);
    let _ = unix_ms;
    e.m.run_slice(cycles)
}

/// The emulated CPU clock, so the driver paces the right chip: 240 MHz on the S3, 160 on the C3.
///
/// # Safety
/// `e` must identify a live emulator that is not being mutated concurrently.
#[expect(
    unsafe_code,
    reason = "the WASM clock query receives an emulator ABI pointer"
)]
#[no_mangle]
pub unsafe extern "C" fn esp32sim_cpu_hz(e: *mut Emu) -> f64 {
    // SAFETY: The caller guarantees a live emulator without concurrent mutation.
    let e = unsafe { &*e };
    e.m.cpu_hz()
}
/// Return the current emulated cycle count.
///
/// # Safety
/// `e` must identify a live emulator that is not being mutated concurrently.
#[expect(
    unsafe_code,
    reason = "the WASM cycle query receives an emulator ABI pointer"
)]
#[no_mangle]
pub unsafe extern "C" fn esp32sim_cycles(e: *mut Emu) -> f64 {
    // SAFETY: The caller guarantees a live emulator without concurrent mutation.
    let e = unsafe { &*e };
    e.m.cycles()
}
/// Return the current emulated instruction count.
///
/// # Safety
/// `e` must identify a live emulator that is not being mutated concurrently.
#[expect(
    unsafe_code,
    reason = "the WASM instruction query receives an emulator ABI pointer"
)]
#[no_mangle]
pub unsafe extern "C" fn esp32sim_insns(e: *mut Emu) -> f64 {
    // SAFETY: The caller guarantees a live emulator without concurrent mutation.
    let e = unsafe { &*e };
    e.m.insns()
}

/// Drain what the machine sent since the last call; then index it with the accessors below.
///
/// # Safety
/// `e` must identify a live emulator with exclusive access for this call.
#[expect(
    unsafe_code,
    reason = "the WASM output drain receives an emulator ABI pointer"
)]
#[no_mangle]
pub unsafe extern "C" fn esp32sim_out_take(e: *mut Emu) -> u32 {
    // SAFETY: The caller guarantees a live, exclusively accessible emulator.
    let e = unsafe { &mut *e };
    e.out = e.m.web().map(|w| w.take_outbox()).unwrap_or_default();
    e.out.len() as u32
}
/// Return the kind of one message from the last output drain.
///
/// # Safety
/// `e` must identify a live emulator that is not being mutated concurrently.
#[expect(
    unsafe_code,
    reason = "the WASM output query receives an emulator ABI pointer"
)]
#[no_mangle]
pub unsafe extern "C" fn esp32sim_out_kind(e: *mut Emu, i: u32) -> u32 {
    // SAFETY: The caller guarantees a live emulator without concurrent mutation.
    let e = unsafe { &*e };
    e.out.get(i as usize).map(|m| m.0 as u32).unwrap_or(0)
}
/// Return the data pointer for one message from the last output drain.
///
/// # Safety
/// `e` must identify a live emulator that is not being mutated concurrently. The returned pointer
/// is valid only until the next mutable operation on `e`.
#[expect(
    unsafe_code,
    reason = "the WASM output query receives an emulator ABI pointer"
)]
#[no_mangle]
pub unsafe extern "C" fn esp32sim_out_ptr(e: *mut Emu, i: u32) -> *const u8 {
    // SAFETY: The caller guarantees a live emulator without concurrent mutation.
    let e = unsafe { &*e };
    e.out
        .get(i as usize)
        .map(|m| m.1.as_ptr())
        .unwrap_or(std::ptr::null())
}
/// Return the data length for one message from the last output drain.
///
/// # Safety
/// `e` must identify a live emulator that is not being mutated concurrently.
#[expect(
    unsafe_code,
    reason = "the WASM output query receives an emulator ABI pointer"
)]
#[no_mangle]
pub unsafe extern "C" fn esp32sim_out_len(e: *mut Emu, i: u32) -> usize {
    // SAFETY: The caller guarantees a live emulator without concurrent mutation.
    let e = unsafe { &*e };
    e.out.get(i as usize).map(|m| m.1.len()).unwrap_or(0)
}

/// Page input in the WebSocket JSON protocol.
///
/// # Safety
/// `e` must identify a live emulator with exclusive access. `ptr` must be readable for `len`
/// bytes and remain valid for this call.
#[expect(
    unsafe_code,
    reason = "the WASM text input receives emulator and buffer ABI pointers"
)]
#[no_mangle]
pub unsafe extern "C" fn esp32sim_in_text(e: *mut Emu, ptr: *const u8, len: usize) {
    // SAFETY: The caller guarantees a live, exclusively accessible emulator.
    let e = unsafe { &mut *e };
    // SAFETY: The caller guarantees a readable input buffer for this call.
    let input = unsafe { text(ptr, len) };
    if let Some(w) = e.m.web() {
        w.push_incoming(input.to_string());
    }
}
/// Page input in the WebSocket binary protocol.
///
/// # Safety
/// `e` must identify a live emulator with exclusive access. `ptr` must be readable for `len`
/// bytes and remain valid for this call.
#[expect(
    unsafe_code,
    reason = "the WASM binary input receives emulator and buffer ABI pointers"
)]
#[no_mangle]
pub unsafe extern "C" fn esp32sim_in_bin(e: *mut Emu, ptr: *const u8, len: usize) {
    // SAFETY: The caller guarantees a live, exclusively accessible emulator.
    let e = unsafe { &mut *e };
    // SAFETY: The caller guarantees a readable input buffer for this call.
    let input = unsafe { bytes(ptr, len) };
    if let Some(w) = e.m.web() {
        w.push_incoming_bin(input.to_vec());
    }
}
