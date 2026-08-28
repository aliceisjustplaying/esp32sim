//! The emulator as a WebAssembly module. A C ABI, hand-driven from `web/wasm/worker.js` — no
//! bindgen, no dependencies. The machine produces exactly the messages the WebSocket UI speaks
//! (`docs/web-ui.md`); here they are queued in a `WebServer::queued()` sink and handed to JS.
//!
//! Lifecycle: `esp32sim_new` → `esp32sim_load` (ROM, bootloader, partition table, app, ELF
//! symbols, script) → optional `esp32sim_wifi` → `esp32sim_boot` → repeated `esp32sim_run(cycles,
//! unix_ms)` with `esp32sim_out_*` draining the outbox after each slice and `esp32sim_in_*`
//! feeding the page's inputs.
use esp32s3::machine::{Machine, Stop};

#[cfg(target_arch = "wasm32")]
#[link(wasm_import_module = "env")]
extern "C" { fn host_log(ptr: *const u8, len: usize); }
#[cfg(not(target_arch = "wasm32"))]
unsafe fn host_log(ptr: *const u8, len: usize) { eprintln!("{}", String::from_utf8_lossy(std::slice::from_raw_parts(ptr, len))); }

fn log(s: &str) { unsafe { host_log(s.as_ptr(), s.len()); } }

pub struct Emu {
    m: Machine,
    /// the last drained outbox: (1 text | 2 binary, payload), addressed by index from JS
    out: Vec<(u8, Vec<u8>)>,
    booted: bool,
}

unsafe fn bytes<'a>(ptr: *const u8, len: usize) -> &'a [u8] { if len == 0 { &[] } else { std::slice::from_raw_parts(ptr, len) } }
unsafe fn text<'a>(ptr: *const u8, len: usize) -> &'a str { std::str::from_utf8(bytes(ptr, len)).unwrap_or("") }

/// Buffers the page fills before handing them to `esp32sim_load` / `esp32sim_in_*`.
#[no_mangle] pub extern "C" fn esp32sim_alloc(len: usize) -> *mut u8 { let mut v = vec![0u8; len.max(1)]; let p = v.as_mut_ptr(); std::mem::forget(v); p }
#[no_mangle] pub unsafe extern "C" fn esp32sim_free(ptr: *mut u8, len: usize) { drop(Vec::from_raw_parts(ptr, len.max(1), len.max(1))); }

/// `board` is one of the CLI names (atech14, waveshare-cam, waveshare-lcd4b, none). Null on an unknown board.
#[no_mangle]
pub unsafe extern "C" fn esp32sim_new(board: *const u8, board_len: usize, flash_mb: u32, psram_mb: u32) -> *mut Emu {
    std::panic::set_hook(Box::new(|info| log(&format!("[emu] panic: {}", info))));
    let board = text(board, board_len).to_string();
    let mut m = Machine::new([0x44, 0x1b, 0xf6, 0x75, 0xdc, 0xe0]);
    let Some(b) = esp32s3::board::make_board(&board) else { log(&format!("[emu] unknown board '{}'", board)); return std::ptr::null_mut() };
    m.bus.board = b;
    for (addr, dev) in m.bus.board.i2c_devices() { m.bus.periph.i2c[0].attach(addr, dev); }
    let (flash_mb, psram_mb) = (flash_mb.max(1) as usize, psram_mb as usize);
    m.bus.flash = vec![0xff; flash_mb << 20];
    let cap = (flash_mb << 20).trailing_zeros() as u8; m.bus.periph.spi1.jedec[2] = cap; m.bus.periph.spi0.jedec[2] = cap;
    m.bus.psram = vec![0; psram_mb << 20];
    m.bus.rebuild_page_table();
    m.bus.periph.lcd_cam.frame_cycles = esp32s3::periph::CPU_HZ / 10;
    m.web = Some(esp32s3::web::WebServer::queued());
    m.realtime = false;                                  // the worker paces; std::time does not exist here
    Box::into_raw(Box::new(Emu { m, out: Vec::new(), booted: false }))
}

#[no_mangle]
pub unsafe extern "C" fn esp32sim_delete(e: *mut Emu) { if !e.is_null() { drop(Box::from_raw(e)); } }

/// kind: 0 mask-ROM ELF, 1 bootloader (flash 0x0), 2 partition table (0x8000), 3 app (0x10000),
/// 4 ELF for symbols, 5 whole flash image (0x0), 6 script text, 7 camera picture (BMP/PPM).
/// Returns 0, or 1 with the reason logged.
#[no_mangle]
pub unsafe extern "C" fn esp32sim_load(e: *mut Emu, kind: u32, ptr: *const u8, len: usize) -> u32 {
    let e = &mut *e; let d = bytes(ptr, len);
    let r = match kind {
        0 => e.m.load_rom(d),
        1 => e.m.write_flash(0, d), 2 => e.m.write_flash(0x8000, d), 3 => e.m.write_flash(0x10000, d),
        4 => e.m.add_symbols(d),
        5 => e.m.write_flash(0, d),
        6 => e.m.load_script(text(ptr, len)),
        7 => esp32s3::picture::parse(d).map(|p| e.m.bus.board.set_camera_picture(p)),
        _ => Err(format!("unknown load kind {}", kind)),
    };
    match r { Ok(()) => 0, Err(msg) => { log(&format!("[emu] load kind {}: {}", kind, msg)); 1 } }
}

/// Attach the virtual access point and subnet: `ssid=NAME,psk=PASS,chan=N`. No NAT — the browser
/// has no sockets — so DHCP, DNS, SNTP and ICMP answer, and connections past the gateway are refused.
#[no_mangle]
pub unsafe extern "C" fn esp32sim_wifi(e: *mut Emu, spec: *const u8, len: usize) {
    let e = &mut *e;
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
    e.m.bus.periph.wifi.ap = Some(esp32s3::wifi::VirtualAp::new(cfg));
    e.m.bus.periph.wifi.net = Some(esp32s3::net::VirtualNet::new());
}

/// `--stub NAME[=value]`: return `value` immediately when execution reaches the function's entry.
/// Needs the ELF symbols loaded first. Returns 1 if the symbol is unknown.
#[no_mangle]
pub unsafe extern "C" fn esp32sim_stub(e: *mut Emu, name: *const u8, len: usize, value: u32) -> u32 {
    let e = &mut *e; let name = text(name, len);
    match e.m.symbols.iter().find(|(_, n)| n.as_str() == name).map(|(a, _)| *a) {
        Some(addr) => { e.m.stubs.insert(addr, value); log(&format!("[emu] stub {} @ {:#x} -> returns {:#x}", name, addr, value)); 0 }
        None => { log(&format!("[emu] stub: no symbol '{}' (load the app ELF first)", name)); 1 }
    }
}

/// Start from the mask ROM (the normal path) or, with `app_direct` set, straight into the app image.
#[no_mangle]
pub unsafe extern "C" fn esp32sim_boot(e: *mut Emu, app_direct: u32) -> u32 {
    let e = &mut *e;
    if app_direct != 0 { if let Err(msg) = e.m.boot_app(0x10000) { log(&format!("[emu] boot: {}", msg)); return 1; } } else { e.m.boot_rom(); }
    // the WebSocket server announces the board in its per-client hello; here there is one client
    if let Some(w) = &e.m.web { w.send_text(&format!("{{\"t\":\"board\",\"name\":\"{}\"}}", e.m.bus.board.name())); }
    e.booted = true; 0
}

/// Run for `cycles` more emulated cycles. Returns 0 while the machine can go on; otherwise a stop
/// code: 2 unimplemented instruction, 3 breakpoint, 4 exception limit, 5 semihosting call.
/// A chip reset (esp_restart, watchdog) reboots through the ROM and keeps going, like the CLI.
#[no_mangle]
pub unsafe extern "C" fn esp32sim_run(e: *mut Emu, cycles: u32, unix_ms: f64) -> u32 {
    let e = &mut *e;
    if !e.booted { return 9; }
    #[cfg(target_arch = "wasm32")] esp32s3::host::set_unix_time_ms(unix_ms as u64);
    let _ = unix_ms;
    e.m.max_cycles = e.m.bus.cycles + cycles as u64;
    loop {
        match e.m.run(u64::MAX) {
            Stop::Halted | Stop::MaxInsns => return 0,
            Stop::SwReset => {
                let cause = e.m.bus.periph.rtc.reset_cause;
                let note = format!("[emu] chip reset at t={:.3}s: cause {:#x} ({})", e.m.bus.cycles as f64 / esp32s3::periph::CPU_HZ as f64, cause, esp32s3::periph::reset_cause_name(cause));
                log(&note); if let Some(w) = &e.m.web { w.send_text(&format!("{{\"t\":\"emu\",\"msg\":\"{}\"}}", esp32s3::web::json_escape(&note))); }
                e.m.reboot();
            }
            Stop::Unimplemented(pc, raw) => { log(&format!("[emu] unimplemented instruction at {:08x} {} (raw {:#x})", pc, e.m.sym(pc), raw)); return 2; }
            Stop::Breakpoint(_) => return 3,
            Stop::Exceptions(_) => return 4,
            Stop::Simcall(_) => return 5,
            Stop::Watch(..) => return 6,
        }
    }
}

#[no_mangle] pub unsafe extern "C" fn esp32sim_cycles(e: *mut Emu) -> f64 { (&*e).m.bus.cycles as f64 }
#[no_mangle] pub unsafe extern "C" fn esp32sim_insns(e: *mut Emu) -> f64 { ((&*e).m.cpu.insn_count + (&*e).m.cpu1.insn_count) as f64 }

/// Drain what the machine sent since the last call; then index it with the accessors below.
#[no_mangle]
pub unsafe extern "C" fn esp32sim_out_take(e: *mut Emu) -> u32 {
    let e = &mut *e;
    e.out = e.m.web.as_ref().map(|w| w.take_outbox()).unwrap_or_default();
    e.out.len() as u32
}
#[no_mangle] pub unsafe extern "C" fn esp32sim_out_kind(e: *mut Emu, i: u32) -> u32 { (&*e).out.get(i as usize).map(|m| m.0 as u32).unwrap_or(0) }
#[no_mangle] pub unsafe extern "C" fn esp32sim_out_ptr(e: *mut Emu, i: u32) -> *const u8 { (&*e).out.get(i as usize).map(|m| m.1.as_ptr()).unwrap_or(std::ptr::null()) }
#[no_mangle] pub unsafe extern "C" fn esp32sim_out_len(e: *mut Emu, i: u32) -> usize { (&*e).out.get(i as usize).map(|m| m.1.len()).unwrap_or(0) }

/// Page inputs, in the WebSocket protocol: JSON text (buttons, knob, touch, serial) or binary (camera).
#[no_mangle] pub unsafe extern "C" fn esp32sim_in_text(e: *mut Emu, ptr: *const u8, len: usize) { if let Some(w) = &(&*e).m.web { w.push_incoming(text(ptr, len).to_string()); } }
#[no_mangle] pub unsafe extern "C" fn esp32sim_in_bin(e: *mut Emu, ptr: *const u8, len: usize) { if let Some(w) = &(&*e).m.web { w.push_incoming_bin(bytes(ptr, len).to_vec()); } }
