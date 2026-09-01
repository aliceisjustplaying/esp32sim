//! The emulator as a WebAssembly module. A C ABI, hand-driven from `web/wasm/worker.js` — no
//! bindgen, no dependencies. The machine produces exactly the messages the WebSocket UI speaks
//! (`docs/web-ui.md`); here they are queued in a `WebServer::queued()` sink and handed to JS.
//!
//! Lifecycle: `esp32sim_new` → `esp32sim_load` (ROM, bootloader, partition table, app, ELF
//! symbols, script) → optional `esp32sim_wifi` → `esp32sim_boot` → repeated `esp32sim_run(cycles,
//! unix_ms)` with `esp32sim_out_*` draining the outbox after each slice and `esp32sim_in_*`
//! feeding the page's inputs.
use esp32s3::machine::{Machine, Stop};

/// Which chip this instance emulates. The two SoCs have separate `Machine` types — different
/// cores, different peripherals — so the C ABI dispatches over this rather than pretending they
/// share an interface they do not.
enum Chip {
    S3(Machine),
    C3(esp32c3::Machine),
}

#[cfg(target_arch = "wasm32")]
#[link(wasm_import_module = "env")]
extern "C" {
    fn host_log(ptr: *const u8, len: usize);
}
#[cfg(not(target_arch = "wasm32"))]
unsafe fn host_log(ptr: *const u8, len: usize) {
    eprintln!(
        "{}",
        String::from_utf8_lossy(std::slice::from_raw_parts(ptr, len))
    );
}

fn log(s: &str) {
    unsafe {
        host_log(s.as_ptr(), s.len());
    }
}

pub struct Emu {
    chip: Chip,
    /// the last drained outbox: (1 text | 2 binary, payload), addressed by index from JS
    out: Vec<(u8, Vec<u8>)>,
    /// messages produced for a chip with no `WebServer` of its own (the C3 is console-only)
    queue: Vec<(u8, Vec<u8>)>,
    booted: bool,
}

impl Emu {
    /// The S3 machine, or None on a C3 instance — for the calls that only make sense there.
    fn s3(&mut self) -> Option<&mut Machine> {
        match &mut self.chip {
            Chip::S3(m) => Some(m),
            _ => None,
        }
    }
    fn text_msg(&mut self, s: &str) {
        self.queue.push((1, s.as_bytes().to_vec()));
    }
}

unsafe fn bytes<'a>(ptr: *const u8, len: usize) -> &'a [u8] {
    if len == 0 {
        &[]
    } else {
        std::slice::from_raw_parts(ptr, len)
    }
}
unsafe fn text<'a>(ptr: *const u8, len: usize) -> &'a str {
    std::str::from_utf8(bytes(ptr, len)).unwrap_or("")
}

/// Buffers the page fills before handing them to `esp32sim_load` / `esp32sim_in_*`.
#[no_mangle]
pub extern "C" fn esp32sim_alloc(len: usize) -> *mut u8 {
    let mut v = vec![0u8; len.max(1)];
    let p = v.as_mut_ptr();
    std::mem::forget(v);
    p
}
#[no_mangle]
pub unsafe extern "C" fn esp32sim_free(ptr: *mut u8, len: usize) {
    drop(Vec::from_raw_parts(ptr, len.max(1), len.max(1)));
}

/// `board` is one of the CLI names (atech14, waveshare-cam, waveshare-lcd4b, none). Null on an unknown board.
/// `board` is a CLI board name (atech14, waveshare-cam, waveshare-lcd4b, none) for the ESP32-S3,
/// or `esp32c3` for the RISC-V chip, which is console-only and takes no board. Null on failure.
#[no_mangle]
pub unsafe extern "C" fn esp32sim_new(
    board: *const u8,
    board_len: usize,
    flash_mb: u32,
    psram_mb: u32,
) -> *mut Emu {
    std::panic::set_hook(Box::new(|info| log(&format!("[emu] panic: {}", info))));
    let board = text(board, board_len).to_string();
    let (flash_mb, psram_mb) = (flash_mb.max(1) as usize, psram_mb as usize);

    if board == "esp32c3" || board == "c3" {
        let mut m = esp32c3::Machine::new([0x3c, 0x84, 0x27, 0xb6, 0xa7, 0x1c], flash_mb << 20);
        let cap = (flash_mb << 20).trailing_zeros() as u8;
        m.bus.periph.spi1.jedec[2] = cap;
        m.bus.periph.spi0.jedec[2] = cap;
        // the ROM mirrors its console to UART0 and USB-Serial/JTAG; take one or it prints twice
        m.console_mask = 2;
        m.capture_console = true;
        return Box::into_raw(Box::new(Emu {
            chip: Chip::C3(m),
            out: Vec::new(),
            queue: Vec::new(),
            booted: false,
        }));
    }

    let mut m = Machine::new([0x44, 0x1b, 0xf6, 0x75, 0xdc, 0xe0]);
    let Some(b) = esp32s3::board::make_board(&board) else {
        log(&format!("[emu] unknown board '{}'", board));
        return std::ptr::null_mut();
    };
    m.bus.board = b;
    for (addr, dev) in m.bus.board.i2c_devices() {
        m.bus.periph.i2c[0].attach(addr, dev);
    }
    m.bus.flash = vec![0xff; flash_mb << 20];
    let cap = (flash_mb << 20).trailing_zeros() as u8;
    m.bus.periph.spi1.jedec[2] = cap;
    m.bus.periph.spi0.jedec[2] = cap;
    m.bus.psram = vec![0; psram_mb << 20];
    m.bus.rebuild_page_table();
    m.bus.periph.lcd_cam.frame_cycles = esp32s3::periph::CPU_HZ / 10;
    m.web = Some(esp32s3::web::WebServer::queued());
    m.realtime = false; // the worker paces; std::time does not exist here
    Box::into_raw(Box::new(Emu {
        chip: Chip::S3(m),
        out: Vec::new(),
        queue: Vec::new(),
        booted: false,
    }))
}

#[no_mangle]
pub unsafe extern "C" fn esp32sim_delete(e: *mut Emu) {
    if !e.is_null() {
        drop(Box::from_raw(e));
    }
}

/// kind: 0 mask-ROM ELF, 1 bootloader (flash 0x0), 2 partition table (0x8000), 3 app (0x10000),
/// 4 ELF for symbols, 5 whole flash image (0x0), 6 script text, 7 camera picture (BMP/PPM).
/// Returns 0, or 1 with the reason logged.
#[no_mangle]
pub unsafe extern "C" fn esp32sim_load(e: *mut Emu, kind: u32, ptr: *const u8, len: usize) -> u32 {
    let e = &mut *e;
    let d = bytes(ptr, len);
    let r = match &mut e.chip {
        Chip::S3(m) => match kind {
            0 => m.load_rom(d),
            1 => m.write_flash(0, d),
            2 => m.write_flash(0x8000, d),
            3 => m.write_flash(0x10000, d),
            4 => m.add_symbols(d),
            5 => m.write_flash(0, d),
            6 => m.load_script(text(ptr, len)),
            7 => esp32s3::picture::parse(d).map(|p| m.bus.board.set_camera_picture(p)),
            _ => Err(format!("unknown load kind {}", kind)),
        },
        Chip::C3(m) => match kind {
            0 => m.load_rom(d),
            1 => m.write_flash(0, d),
            2 => m.write_flash(0x8000, d),
            3 => m.write_flash(0x10000, d),
            4 => m.add_symbols(d),
            5 => m.write_flash(0, d),
            _ => Err(format!(
                "load kind {} is not supported on the C3 (no scripts, no camera)",
                kind
            )),
        },
    };
    match r {
        Ok(()) => 0,
        Err(msg) => {
            log(&format!("[emu] load kind {}: {}", kind, msg));
            1
        }
    }
}

/// Write bytes into flash at an arbitrary offset (a data partition's contents).
#[no_mangle]
pub unsafe extern "C" fn esp32sim_load_at(
    e: *mut Emu,
    offset: u32,
    ptr: *const u8,
    len: usize,
) -> u32 {
    let e = &mut *e;
    let d = bytes(ptr, len);
    let r = match &mut e.chip {
        Chip::S3(m) => m.write_flash(offset as usize, d),
        Chip::C3(m) => m.write_flash(offset as usize, d),
    };
    match r {
        Ok(()) => 0,
        Err(msg) => {
            log(&format!("[emu] flash {:#x}: {}", offset, msg));
            1
        }
    }
}

/// Attach the virtual access point and subnet: `ssid=NAME,psk=PASS,chan=N`. No NAT — the browser
/// has no sockets — so DHCP, DNS, SNTP and ICMP answer, and connections past the gateway are refused.
#[no_mangle]
pub unsafe extern "C" fn esp32sim_wifi(e: *mut Emu, spec: *const u8, len: usize) {
    let e = &mut *e;
    if e.s3().is_none() {
        log("[emu] wifi: the C3 radio is not modelled");
        return;
    }
    let mut cfg = esp32s3::wifi::ApConfig {
        ssid: "esp32sim".into(),
        bssid: [0x02, 0x53, 0x49, 0x4d, 0x00, 0x01],
        channel: 6,
        psk: None,
    };
    for kv in text(spec, len).split(',') {
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
    let m = e.s3().unwrap();
    m.bus.periph.wifi.ap = Some(esp32s3::wifi::VirtualAp::new(cfg));
    m.bus.periph.wifi.net = Some(esp32s3::net::VirtualNet::new());
}

/// `--stub NAME[=value]`: return `value` immediately when execution reaches the function's entry.
/// NAME is a symbol (needs the ELF loaded) or a hex address. Returns 1 if it cannot be resolved.
#[no_mangle]
pub unsafe extern "C" fn esp32sim_stub(
    e: *mut Emu,
    name: *const u8,
    len: usize,
    value: u32,
) -> u32 {
    let e = &mut *e;
    let name = text(name, len).to_string();
    let by_addr = name
        .strip_prefix("0x")
        .and_then(|h| u32::from_str_radix(h, 16).ok());
    let Some(m) = e.s3() else {
        log("[emu] stub: not supported on the C3");
        return 1;
    };
    match by_addr.or_else(|| {
        m.symbols
            .iter()
            .find(|(_, n)| n.as_str() == name.as_str())
            .map(|(a, _)| *a)
    }) {
        Some(addr) => {
            m.stubs.insert(addr, value);
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

/// Start from the mask ROM (the normal path) or, with `app_direct` set, straight into the app image.
#[no_mangle]
pub unsafe extern "C" fn esp32sim_boot(e: *mut Emu, app_direct: u32) -> u32 {
    let e = &mut *e;
    // the WebSocket server announces the board in its per-client hello; here there is one client
    let name = match &mut e.chip {
        Chip::S3(m) => {
            if app_direct != 0 {
                if let Err(msg) = m.boot_app(0x10000) {
                    log(&format!("[emu] boot: {}", msg));
                    return 1;
                }
            } else {
                m.boot_rom();
            }
            let n = m.bus.board.name().to_string();
            if let Some(w) = &m.web {
                w.send_text(&format!("{{\"t\":\"board\",\"name\":\"{}\"}}", n));
            }
            n
        }
        Chip::C3(m) => {
            if app_direct != 0 {
                if let Err(msg) = m.boot_app(0x10000) {
                    log(&format!("[emu] boot: {}", msg));
                    return 1;
                }
            } else {
                m.boot_rom();
            }
            "esp32c3".to_string()
        }
    };
    if matches!(e.chip, Chip::C3(_)) {
        e.text_msg(&format!("{{\"t\":\"board\",\"name\":\"{}\"}}", name));
    }
    e.booted = true;
    0
}

/// Run for `cycles` more emulated cycles. Returns 0 while the machine can go on; otherwise a stop
/// code: 2 unimplemented instruction, 3 breakpoint, 4 exception limit, 5 semihosting call.
/// A chip reset (esp_restart, watchdog) reboots through the ROM and keeps going, like the CLI.
#[no_mangle]
pub unsafe extern "C" fn esp32sim_run(e: *mut Emu, cycles: u32, unix_ms: f64) -> u32 {
    let e = &mut *e;
    if !e.booted {
        return 9;
    }
    #[cfg(target_arch = "wasm32")]
    esp32s3::host::set_unix_time_ms(unix_ms as u64);
    let _ = unix_ms;
    match &mut e.chip {
        Chip::S3(m) => {
            m.max_cycles = m.bus.cycles + cycles as u64;
            loop {
                match m.run(u64::MAX) {
                    Stop::Halted | Stop::MaxInsns => return 0,
                    Stop::SwReset => {
                        let cause = m.bus.periph.rtc.reset_cause;
                        let note = format!(
                            "[emu] chip reset at t={:.3}s: cause {:#x} ({})",
                            m.bus.cycles as f64 / esp32s3::periph::CPU_HZ as f64,
                            cause,
                            esp32s3::periph::reset_cause_name(cause)
                        );
                        log(&note);
                        if let Some(w) = &m.web {
                            w.send_text(&format!(
                                "{{\"t\":\"emu\",\"msg\":\"{}\"}}",
                                esp32s3::web::json_escape(&note)
                            ));
                        }
                        m.reboot();
                    }
                    Stop::Unimplemented(pc, raw) => {
                        log(&format!(
                            "[emu] unimplemented instruction at {:08x} {} (raw {:#x})",
                            pc,
                            m.sym(pc),
                            raw
                        ));
                        return 2;
                    }
                    Stop::Breakpoint(_) => return 3,
                    Stop::Exceptions(_) => return 4,
                    Stop::Simcall(_) => return 5,
                    Stop::Watch(..) => return 6,
                }
            }
        }
        Chip::C3(m) => {
            use esp32c3::Stop as S;
            m.max_cycles = m.bus.cycles + cycles as u64;
            let mut note = None;
            let rc = loop {
                match m.run(u64::MAX) {
                    S::Halted | S::MaxInsns => break 0,
                    S::SwReset => {
                        let cause = m.bus.periph.rtc.reset_cause;
                        note = Some(format!(
                            "[emu] chip reset at t={:.3}s: cause {:#x} ({})",
                            m.seconds(),
                            cause,
                            esp32s3::periph::reset_cause_name(cause)
                        ));
                        m.reboot();
                    }
                    S::Ebreak(pc) => {
                        log(&format!("[emu] ebreak at {:08x} {}", pc, m.sym(pc)));
                        break 3;
                    }
                    S::Breakpoint(_) => break 3,
                    S::Exceptions(_) => break 4,
                    S::Watch(..) => break 6,
                }
            };
            // the C3 machine has no WebServer: turn its console into the same protocol by hand
            m.drain_console();
            let out = std::mem::take(&mut m.console);
            if !out.is_empty() {
                let txt = String::from_utf8_lossy(&out).to_string();
                e.text_msg(&format!(
                    "{{\"t\":\"serial\",\"src\":\"usb\",\"data\":\"{}\"}}",
                    esp32s3::web::json_escape(&txt)
                ));
            }
            if let Some(n) = note {
                log(&n);
                e.text_msg(&format!(
                    "{{\"t\":\"emu\",\"msg\":\"{}\"}}",
                    esp32s3::web::json_escape(&n)
                ));
            }
            let (t, insns) = match &e.chip {
                Chip::C3(m) => (m.seconds(), m.cpu.insn_count),
                _ => (0.0, 0),
            };
            e.text_msg(&format!("{{\"t\":\"stat\",\"time\":{:.2},\"insns\":{},\"frames\":0,\"behind\":0,\"resyncs\":0,\"cam\":0,\"gpio_in\":\"0\"}}", t, insns));
            rc
        }
    }
}

/// The emulated CPU clock, so the driver paces the right chip: 240 MHz on the S3, 160 on the C3.
#[no_mangle]
pub unsafe extern "C" fn esp32sim_cpu_hz(e: *mut Emu) -> f64 {
    match &(&*e).chip {
        Chip::S3(_) => esp32s3::periph::CPU_HZ as f64,
        Chip::C3(_) => esp32c3::periph::CPU_HZ as f64,
    }
}
#[no_mangle]
pub unsafe extern "C" fn esp32sim_cycles(e: *mut Emu) -> f64 {
    match &(&*e).chip {
        Chip::S3(m) => m.bus.cycles as f64,
        Chip::C3(m) => m.bus.cycles as f64,
    }
}
#[no_mangle]
pub unsafe extern "C" fn esp32sim_insns(e: *mut Emu) -> f64 {
    match &(&*e).chip {
        Chip::S3(m) => (m.cpu.insn_count + m.cpu1.insn_count) as f64,
        Chip::C3(m) => m.cpu.insn_count as f64,
    }
}

/// Drain what the machine sent since the last call; then index it with the accessors below.
#[no_mangle]
pub unsafe extern "C" fn esp32sim_out_take(e: *mut Emu) -> u32 {
    let e = &mut *e;
    e.out = std::mem::take(&mut e.queue);
    if let Chip::S3(m) = &e.chip {
        if let Some(w) = &m.web {
            e.out.extend(w.take_outbox());
        }
    }
    e.out.len() as u32
}
#[no_mangle]
pub unsafe extern "C" fn esp32sim_out_kind(e: *mut Emu, i: u32) -> u32 {
    (&*e).out.get(i as usize).map(|m| m.0 as u32).unwrap_or(0)
}
#[no_mangle]
pub unsafe extern "C" fn esp32sim_out_ptr(e: *mut Emu, i: u32) -> *const u8 {
    (&*e)
        .out
        .get(i as usize)
        .map(|m| m.1.as_ptr())
        .unwrap_or(std::ptr::null())
}
#[no_mangle]
pub unsafe extern "C" fn esp32sim_out_len(e: *mut Emu, i: u32) -> usize {
    (&*e).out.get(i as usize).map(|m| m.1.len()).unwrap_or(0)
}

/// Page inputs, in the WebSocket protocol: JSON text (buttons, knob, touch, serial) or binary (camera).
#[no_mangle]
pub unsafe extern "C" fn esp32sim_in_text(e: *mut Emu, ptr: *const u8, len: usize) {
    let e = &mut *e;
    let msg = text(ptr, len).to_string();
    match &mut e.chip {
        Chip::S3(m) => {
            if let Some(w) = &m.web {
                w.push_incoming(msg);
            }
        }
        // the C3 has no board and no WebServer: the console's serial input is the one thing to wire
        Chip::C3(m) => {
            if esp32s3::web::json_str(&msg, "t").as_deref() == Some("serial") {
                let line = esp32s3::web::json_str(&msg, "line").unwrap_or_default();
                m.bus
                    .periph
                    .usb
                    .host_input(format!("{}\n", line).as_bytes());
            }
        }
    }
}
#[no_mangle]
pub unsafe extern "C" fn esp32sim_in_bin(e: *mut Emu, ptr: *const u8, len: usize) {
    let e = &mut *e;
    if let Chip::S3(m) = &e.chip {
        if let Some(w) = &m.web {
            w.push_incoming_bin(bytes(ptr, len).to_vec());
        }
    }
}
