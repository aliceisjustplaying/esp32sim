//! The emulator as a WebAssembly module. A C ABI, hand-driven from `web/wasm/worker.js` — no
//! bindgen, no dependencies. The machine produces exactly the messages the WebSocket UI speaks
//! (`docs/web-ui.md`); here they are queued in a `WebServer::queued()` sink and handed to JS.
//!
//! Lifecycle: `esp32sim_new` → `esp32sim_load` (ROM, bootloader, partition table, app, ELF
//! symbols, script) → optional `esp32sim_wifi` → `esp32sim_boot` → repeated `esp32sim_run(cycles,
//! unix_ms)` with `esp32sim_out_*` draining the outbox after each slice and `esp32sim_in_*`
//! feeding the page's inputs.
use esp_soc::Stop;

/// Which chip this instance emulates: the two `Machine<S>` types differ in their cores and
/// peripherals, so the C ABI dispatches over this.
enum Chip {
    S3(esp32s3::Machine),
    C3(esp32c3::Machine),
}

/// Run `$body` with `$m` bound to whichever machine this is.
macro_rules! either { ($e:expr, $m:ident => $body:expr) => { match &mut $e.chip { Chip::S3($m) => $body, Chip::C3($m) => $body } } }

#[cfg(target_arch = "wasm32")]
#[link(wasm_import_module = "env")]
extern "C" { fn host_log(ptr: *const u8, len: usize); }
#[cfg(not(target_arch = "wasm32"))]
unsafe fn host_log(ptr: *const u8, len: usize) { eprintln!("{}", String::from_utf8_lossy(std::slice::from_raw_parts(ptr, len))); }

fn log(s: &str) { unsafe { host_log(s.as_ptr(), s.len()); } }

pub struct Emu {
    chip: Chip,
    /// the last drained outbox: (1 text | 2 binary, payload), addressed by index from JS
    out: Vec<(u8, Vec<u8>)>,
    booted: bool,
}

unsafe fn bytes<'a>(ptr: *const u8, len: usize) -> &'a [u8] { if len == 0 { &[] } else { std::slice::from_raw_parts(ptr, len) } }
unsafe fn text<'a>(ptr: *const u8, len: usize) -> &'a str { std::str::from_utf8(bytes(ptr, len)).unwrap_or("") }

/// Buffers the page fills before handing them to `esp32sim_load` / `esp32sim_in_*`.
#[no_mangle] pub extern "C" fn esp32sim_alloc(len: usize) -> *mut u8 { let mut v = vec![0u8; len.max(1)]; let p = v.as_mut_ptr(); std::mem::forget(v); p }
#[no_mangle] pub unsafe extern "C" fn esp32sim_free(ptr: *mut u8, len: usize) { drop(Vec::from_raw_parts(ptr, len.max(1), len.max(1))); }

/// `board` is a CLI board name (atech14, waveshare-cam, waveshare-lcd4b, none) for the ESP32-S3,
/// or `esp32c3` for the RISC-V chip, which is console-only and takes no board. Null on failure.
#[no_mangle]
pub unsafe extern "C" fn esp32sim_new(board: *const u8, board_len: usize, flash_mb: u32, psram_mb: u32) -> *mut Emu {
    std::panic::set_hook(Box::new(|info| log(&format!("[emu] panic: {}", info))));
    let board = text(board, board_len).to_string();
    let (flash_mb, psram_mb) = (flash_mb.max(1) as usize, psram_mb as usize);

    if board == "esp32c3" || board == "c3" {
        let mut m = esp32c3::machine([0x3c, 0x84, 0x27, 0xb6, 0xa7, 0x1c], flash_mb << 20);
        let cap = (flash_mb << 20).trailing_zeros() as u8;
        m.bus.periph.spi1.jedec[2] = cap;
        m.bus.periph.spi0.jedec[2] = cap;
        // the ROM mirrors its console to UART0 and USB-Serial/JTAG; take one or it prints twice
        m.console.mask = 2;
        m.console.capture = true;
        m.web = Some(esp_soc::web::WebServer::queued());
        m.rt.enabled = false;
        return Box::into_raw(Box::new(Emu { chip: Chip::C3(m), out: Vec::new(), booted: false }));
    }

    let mut m = esp32s3::machine([0x44, 0x1b, 0xf6, 0x75, 0xdc, 0xe0]);
    let Some(b) = esp32s3::board::make_board(&board) else { log(&format!("[emu] unknown board '{}'", board)); return std::ptr::null_mut() };
    m.bus.board = b;
    for (bus, addr, dev) in m.bus.board.i2c_devices() { m.bus.periph.i2c[bus as usize].attach(addr, dev); }
    m.bus.flash = vec![0xff; flash_mb << 20];
    let cap = (flash_mb << 20).trailing_zeros() as u8; m.bus.periph.spi1.jedec[2] = cap; m.bus.periph.spi0.jedec[2] = cap;
    m.bus.psram = vec![0; psram_mb << 20];
    m.bus.rebuild_page_table();
    m.bus.periph.lcd_cam.frame_cycles = esp32s3::periph::CPU_HZ / 10;
    m.web = Some(esp_soc::web::WebServer::queued());
    m.rt.enabled = false;                                // the worker paces; std::time does not exist here
    m.console.capture = true;
    Box::into_raw(Box::new(Emu { chip: Chip::S3(m), out: Vec::new(), booted: false }))
}

#[no_mangle]
pub unsafe extern "C" fn esp32sim_delete(e: *mut Emu) { if !e.is_null() { drop(Box::from_raw(e)); } }

/// kind: 0 mask-ROM ELF, 1 bootloader (flash 0x0), 2 partition table (0x8000), 3 app (0x10000),
/// 4 ELF for symbols, 5 whole flash image (0x0), 6 script text, 7 camera picture (BMP/PPM).
/// Returns 0, or 1 with the reason logged.
#[no_mangle]
pub unsafe extern "C" fn esp32sim_load(e: *mut Emu, kind: u32, ptr: *const u8, len: usize) -> u32 {
    let e = &mut *e; let d = bytes(ptr, len);
    let r = either!(e, m => match kind {
        0 => m.load_rom(d),
        1 => m.write_flash(0, d), 2 => m.write_flash(0x8000, d), 3 => m.write_flash(0x10000, d),
        4 => m.add_symbols(d),
        5 => m.write_flash(0, d),
        6 => m.load_script(text(ptr, len)),
        7 => esp_soc::picture::parse(d).map(|p| m.bus.board.set_camera_picture(p)),
        _ => Err(format!("unknown load kind {}", kind)),
    });
    match r { Ok(()) => 0, Err(msg) => { log(&format!("[emu] load kind {}: {}", kind, msg)); 1 } }
}

/// Write bytes into flash at an arbitrary offset (a data partition's contents).
#[no_mangle]
pub unsafe extern "C" fn esp32sim_load_at(e: *mut Emu, offset: u32, ptr: *const u8, len: usize) -> u32 {
    let e = &mut *e; let d = bytes(ptr, len);
    let r = either!(e, m => m.write_flash(offset as usize, d));
    match r { Ok(()) => 0, Err(msg) => { log(&format!("[emu] flash {:#x}: {}", offset, msg)); 1 } }
}

/// Attach the virtual access point and subnet: `ssid=NAME,psk=PASS,chan=N`. No NAT — the browser
/// has no sockets — so DHCP, DNS, SNTP and ICMP answer, and connections past the gateway are refused.
#[no_mangle]
pub unsafe extern "C" fn esp32sim_wifi(e: *mut Emu, spec: *const u8, len: usize) {
    let e = &mut *e;
    let Chip::S3(m) = &mut e.chip else { log("[emu] wifi: the C3 radio is not modelled"); return };
    let mut cfg = esp32s3::wifi::ApConfig { ssid: "esp32sim".into(), bssid: [0x02, 0x53, 0x49, 0x4d, 0x00, 0x01], channel: 6, psk: None };
    for kv in text(spec, len).split(',') {
        match kv.split_once('=') {
            Some(("ssid", v)) => cfg.ssid = v.to_string(),
            Some(("chan", v)) | Some(("channel", v)) => cfg.channel = v.parse().unwrap_or(6),
            Some(("psk", v)) | Some(("password", v)) => cfg.psk = Some(v.to_string()),
            _ => {}
        }
    }
    log(&format!("[emu] virtual AP '{}' ({}), subnet 10.0.2.0/24, no NAT in the browser", cfg.ssid, if cfg.psk.is_some() { "WPA2-PSK" } else { "open" }));
    m.bus.periph.wifi.ap = Some(esp32s3::wifi::VirtualAp::new(cfg, m.bus.debug.has("wifi-frames")));
    m.bus.periph.wifi.net = Some(esp32s3::net::VirtualNet::new(m.bus.debug.has("net")));
}

/// `--stub NAME[=value]`: return `value` immediately when execution reaches the function's entry.
/// NAME is a symbol (needs the ELF loaded) or a hex address. Returns 1 if it cannot be resolved.
#[no_mangle]
pub unsafe extern "C" fn esp32sim_stub(e: *mut Emu, name: *const u8, len: usize, value: u32) -> u32 {
    let e = &mut *e; let name = text(name, len).to_string();
    let by_addr = name.strip_prefix("0x").and_then(|h| u32::from_str_radix(h, 16).ok());
    either!(e, m => match by_addr.or_else(|| m.sym_addr(&name)) {
        Some(addr) => { m.stubs.insert(addr, value); log(&format!("[emu] stub {} @ {:#x} -> returns {:#x}", name, addr, value)); 0 }
        None => { log(&format!("[emu] stub: no symbol '{}' (load the app ELF first)", name)); 1 }
    })
}

/// Attach an analysis: `profile-blocks`, `coverage`, `irq-latency` (no argument), `trace-fn`
/// (arg = symbol prefix as text). Returns 1 for an unknown name.
#[no_mangle]
pub unsafe extern "C" fn esp32sim_observer(e: *mut Emu, name: *const u8, len: usize, arg: *const u8, arg_len: usize) -> u32 {
    use esp_soc::observers::{BlockProfile, Coverage, IrqLatency};
    let e = &mut *e; let (name, arg) = (text(name, len).to_string(), text(arg, arg_len).to_string());
    either!(e, m => match name.as_str() {
        "profile-blocks" => { m.add_observer(Box::new(BlockProfile::new(20))); 0 }
        "coverage" => { m.add_observer(Box::new(Coverage::new(None))); 0 }
        "irq-latency" => { let n = m.cores.len(); m.add_observer(Box::new(IrqLatency::new(n))); 0 }
        "trace-fn" => { let n: Vec<(u32, String)> = m.symbols.iter().filter(|(_, s)| s.starts_with(&arg)).map(|(a, s)| (*a, s.clone())).collect(); for (a, s) in n { m.fn_probes.insert(a, s); } 0 }
        _ => { log(&format!("[emu] unknown observer '{}'", name)); 1 }
    })
}

/// Every observer's report so far, as `emu` messages in the outbox.
#[no_mangle]
pub unsafe extern "C" fn esp32sim_reports(e: *mut Emu) {
    let e = &mut *e;
    let r = either!(e, m => m.reports());
    either!(e, m => if let Some(w) = &m.web { for line in r.lines() { w.send_text(&format!("{{\"t\":\"emu\",\"msg\":\"{}\"}}", esp_soc::web::json_escape(line))); } });
}

/// Start from the mask ROM (the normal path) or, with `app_direct` set, straight into the app image.
#[no_mangle]
pub unsafe extern "C" fn esp32sim_boot(e: *mut Emu, app_direct: u32) -> u32 {
    let e = &mut *e;
    // the WebSocket server announces the board in its per-client hello; here there is one client
    let ok = either!(e, m => {
        if app_direct != 0 { if let Err(msg) = m.boot_app(0x10000) { log(&format!("[emu] boot: {}", msg)); false } else { true } } else { m.boot_rom(); true }
    });
    if !ok { return 1; }
    let name = either!(e, m => m.bus.board.name().to_string());
    either!(e, m => if let Some(w) = &m.web { w.send_text(&format!("{{\"t\":\"board\",\"name\":\"{}\"}}", name)); });
    e.booted = true; 0
}

/// Run for `cycles` more emulated cycles. Returns 0 while the machine can go on; otherwise a stop
/// code: 2 unimplemented instruction, 3 breakpoint, 4 exception limit, 5 semihosting call.
/// A chip reset (esp_restart, watchdog) reboots through the ROM and keeps going, like the CLI.
#[no_mangle]
pub unsafe extern "C" fn esp32sim_run(e: *mut Emu, cycles: u32, unix_ms: f64) -> u32 {
    let e = &mut *e;
    if !e.booted { return 9; }
    #[cfg(target_arch = "wasm32")] esp_soc::host::set_unix_time_ms(unix_ms as u64);
    let _ = unix_ms;
    either!(e, m => {
        m.max_cycles = m.bus.cycles + cycles as u64;
        loop {
            match m.run(u64::MAX) {
                Stop::Halted | Stop::MaxInsns => return 0,
                Stop::SwReset => {
                    let cause = m.bus.periph.rtc.reset_cause;
                    let note = format!("[emu] chip reset at t={:.3}s: cause {:#x} ({})", m.seconds(), cause, esp_periph::reset_cause_name(cause));
                    log(&note);
                    if let Some(w) = &m.web { w.send_text(&format!("{{\"t\":\"emu\",\"msg\":\"{}\"}}", esp_soc::web::json_escape(&note))); }
                    m.reboot();
                }
                Stop::Unimplemented(pc, raw) => { log(&format!("[emu] unimplemented instruction at {:08x} {} (raw {:#x})", pc, m.sym(pc), raw)); return 2; }
                Stop::Ebreak(pc) => { log(&format!("[emu] ebreak at {:08x} {}", pc, m.sym(pc))); return 3; }
                Stop::Breakpoint(_) => return 3,
                Stop::Exceptions(_) => return 4,
                Stop::Simcall(_) => return 5,
                Stop::Watch(..) => return 6,
            }
        }
    })
}

/// The emulated CPU clock, so the driver paces the right chip: 240 MHz on the S3, 160 on the C3.
#[no_mangle] pub unsafe extern "C" fn esp32sim_cpu_hz(e: *mut Emu) -> f64 { match &(&*e).chip { Chip::S3(_) => esp32s3::periph::CPU_HZ as f64, Chip::C3(_) => esp32c3::periph::CPU_HZ as f64 } }
#[no_mangle] pub unsafe extern "C" fn esp32sim_cycles(e: *mut Emu) -> f64 { either!(&mut *e, m => m.bus.cycles as f64) }
#[no_mangle] pub unsafe extern "C" fn esp32sim_insns(e: *mut Emu) -> f64 { either!(&mut *e, m => m.insns() as f64) }

/// Drain what the machine sent since the last call; then index it with the accessors below.
#[no_mangle]
pub unsafe extern "C" fn esp32sim_out_take(e: *mut Emu) -> u32 {
    let e = &mut *e;
    e.out = either!(e, m => m.web.as_ref().map(|w| w.take_outbox()).unwrap_or_default());
    e.out.len() as u32
}
#[no_mangle] pub unsafe extern "C" fn esp32sim_out_kind(e: *mut Emu, i: u32) -> u32 { (&*e).out.get(i as usize).map(|m| m.0 as u32).unwrap_or(0) }
#[no_mangle] pub unsafe extern "C" fn esp32sim_out_ptr(e: *mut Emu, i: u32) -> *const u8 { (&*e).out.get(i as usize).map(|m| m.1.as_ptr()).unwrap_or(std::ptr::null()) }
#[no_mangle] pub unsafe extern "C" fn esp32sim_out_len(e: *mut Emu, i: u32) -> usize { (&*e).out.get(i as usize).map(|m| m.1.len()).unwrap_or(0) }

/// Page inputs, in the WebSocket protocol: JSON text (buttons, knob, touch, serial) or binary (camera).
#[no_mangle]
pub unsafe extern "C" fn esp32sim_in_text(e: *mut Emu, ptr: *const u8, len: usize) {
    let e = &mut *e; let msg = text(ptr, len).to_string();
    either!(e, m => if let Some(w) = &m.web { w.push_incoming(msg); });
}
#[no_mangle]
pub unsafe extern "C" fn esp32sim_in_bin(e: *mut Emu, ptr: *const u8, len: usize) {
    let e = &mut *e;
    either!(e, m => if let Some(w) = &m.web { w.push_incoming_bin(bytes(ptr, len).to_vec()); });
}
