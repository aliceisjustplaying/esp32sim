//! esp32sim — command-line front end.
//!
//!   esp32sim --rom ROM.elf --bootloader b.bin --ptable p.bin --app firmware.bin [--elf firmware.elf]
//!               [--boot app|rom] [--max-insns N] [--trace] [--trace-from N] [--break ADDR] [--log-periph]
use esp32s3::{Machine, Stop};
use std::path::PathBuf;

fn find_rom() -> Option<PathBuf> {
    let home = std::env::var("HOME").ok()?;
    let dir = std::fs::read_dir(format!("{}/.espressif/tools/esp-rom-elfs", home)).ok()?;
    for e in dir.flatten() { let p = e.path().join("esp32s3_rev0_rom.elf"); if p.exists() { return Some(p); } }
    None
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let mut rom = find_rom(); let mut bootloader = None; let mut ptable = None; let mut app = None; let mut elfs: Vec<String> = Vec::new();
    let mut boot = "app".to_string(); let mut max_insns = u64::MAX; let mut trace = false; let mut trace_from = 0u64;
    let mut breaks = Vec::new(); let mut log_periph = false; let mut dump_at_end = true; let mut peeks: Vec<(u32, usize)> = Vec::new(); let mut disasms: Vec<(u32, usize)> = Vec::new(); let mut watch = None; let mut stop_exc = u64::MAX; let mut profile = false; let mut wav: Option<String> = None; let mut tft_png: Option<String> = None; let mut gram_png: Option<String> = None; let mut script: Option<String> = None; let mut max_seconds: Option<f64> = None; let mut console = "both".to_string(); let mut console_prefix = false; let mut regtrace: Option<String> = None; let mut regtrace_max = u64::MAX; let mut regtrace_from_pc: Option<u32> = None; let mut efuse_file: Option<String> = None; let mut regs_init: Option<String> = None; let mut web_port: Option<u16> = None; let mut realtime = false; let mut web_dir: Option<String> = None; let mut strap: Option<u32> = None; let mut reset_cause: Option<u32> = None; let mut flash_mb: usize = 8; let mut flash_image: Option<String> = None; let mut board = "atech14".to_string(); let mut no_reboot = false; let mut psram_mb: usize = 2; let mut cam_image: Option<String> = None; let mut cam_fps: f64 = 10.0; let mut stubs: Vec<String> = Vec::new(); let mut regstat: Option<String> = None; let mut trace_fns: Vec<String> = Vec::new(); let mut wifi: Option<String> = None;
    let mut i = 1;
    while i < args.len() {
        let a = args[i].as_str();
        let mut next = || { i += 1; args.get(i).cloned().unwrap_or_default() };
        match a {
            "--rom" => rom = Some(PathBuf::from(next())),
            "--bootloader" => bootloader = Some(next()),
            "--ptable" => ptable = Some(next()),
            "--app" => app = Some(next()),
            "--elf" => elfs.push(next()),
            "--boot" => boot = next(),
            "--max-insns" => max_insns = next().replace('_', "").parse().expect("max-insns"),
            "--trace" => trace = true,
            "--trace-from" => { trace = true; trace_from = next().replace('_', "").parse().expect("trace-from") }
            "--break" => breaks.push(u32::from_str_radix(next().trim_start_matches("0x"), 16).expect("break addr")),
            "--log-periph" => log_periph = true,
            "--no-dump" => dump_at_end = false,
            "--peek" => { let v = next(); let mut it = v.split(','); let a = u32::from_str_radix(it.next().unwrap().trim_start_matches("0x"), 16).expect("peek addr"); let n = it.next().map(|x| x.parse().unwrap()).unwrap_or(8); peeks.push((a, n)); }
            "--disasm" => { let v = next(); let mut it = v.split(','); let a = u32::from_str_radix(it.next().unwrap().trim_start_matches("0x"), 16).expect("addr"); let n = it.next().map(|x| x.parse().unwrap()).unwrap_or(16); disasms.push((a, n)); }
            "--watch" => watch = Some(u32::from_str_radix(next().trim_start_matches("0x"), 16).expect("watch addr")),
            "--stop-after-exceptions" => stop_exc = next().parse().expect("count"),
            "--profile" => profile = true,
            "--wav" => wav = Some(next()),
            "--tft-png" => tft_png = Some(next()),
            "--script" => script = Some(next()),
            "--console" => console = next(),          // usb | uart0 | both | all | none
            "--console-prefix" => console_prefix = true,
            "--regtrace" => regtrace = Some(next()),
            "--regtrace-max" => regtrace_max = next().parse().expect("n"),
            "--regtrace-from-pc" => regtrace_from_pc = Some(u32::from_str_radix(next().trim_start_matches("0x"), 16).expect("pc")),
            "--efuse-regs" => efuse_file = Some(next()),        // text: lines "0x600070xx: xxxxxxxx xxxxxxxx ..." (openocd mdw) or "off value"
            "--web" => web_port = Some(next().parse().expect("port")),
            "--web-dir" => web_dir = Some(next()),
            "--board" => board = next(),
            "--no-reboot" => no_reboot = true,
            "--realtime" => realtime = true,
            "--regs-init" => regs_init = Some(next()),   // openocd mdw dump taken at reset halt
            "--strap" => strap = Some(u32::from_str_radix(next().trim_start_matches("0x"), 16).expect("strap")),
            "--reset-cause" => reset_cause = Some(u32::from_str_radix(next().trim_start_matches("0x"), 16).expect("cause")),
            "--flash-mb" => flash_mb = next().parse().expect("mb"),
            "--psram-mb" => psram_mb = next().parse().expect("mb"),
            "--cam-image" => cam_image = Some(next()),
            "--cam-fps" => cam_fps = next().parse().expect("fps"),
            "--stub" => stubs.push(next()),
            "--regstat" => regstat = Some(next()),
            "--trace-fn" => trace_fns.push(next()),
            "--wifi" => wifi = Some(next()),
            "--flash-image" => flash_image = Some(next()),      // raw flash dump written at offset 0
            "--max-seconds" => max_seconds = Some(next().parse().expect("seconds")),
            "--gram-png" => gram_png = Some(next()),
            _ => { eprintln!("unknown arg {}", a); std::process::exit(2); }
        }
        i += 1;
    }
    let mut m = Machine::new([0x44, 0x1b, 0xf6, 0x75, 0xdc, 0xe0]);
    m.bus.board = esp32s3::board::make_board(&board).unwrap_or_else(|| { eprintln!("unknown board '{}' (atech14, waveshare-cam, waveshare-lcd4b, none)", board); std::process::exit(2) });
    for (addr, dev) in m.bus.board.i2c_devices() { m.bus.periph.i2c[0].attach(addr, dev); }
    if regstat.is_some() { m.bus.periph.regstat = Some(Default::default()); }
    if let Some(spec) = &wifi {
        let mut cfg = esp32s3::wifi::ApConfig { ssid: "esp32sim".into(), bssid: [0x02, 0x53, 0x49, 0x4d, 0x00, 0x01], channel: 6, psk: None };
        for kv in spec.split(',') {
            match kv.split_once('=') {
                Some(("ssid", v)) => cfg.ssid = v.to_string(),
                Some(("chan", v)) | Some(("channel", v)) => cfg.channel = v.parse().unwrap_or(6),
                Some(("psk", v)) | Some(("password", v)) => cfg.psk = Some(v.to_string()),
                Some(("bssid", v)) => { let b: Vec<u8> = v.split(':').filter_map(|x| u8::from_str_radix(x, 16).ok()).collect(); if b.len() == 6 { cfg.bssid.copy_from_slice(&b); } }
                _ => {}
            }
        }
        eprintln!("[emu] virtual AP '{}' bssid {} channel {} ({})", cfg.ssid, esp32s3::wifi::mac_str(&cfg.bssid), cfg.channel, if cfg.psk.is_some() { "WPA2-PSK" } else { "open" });
        m.bus.periph.wifi.ap = Some(esp32s3::wifi::VirtualAp::new(cfg));
    }
    if let Some(p) = &cam_image { match esp32s3::picture::load(p) { Ok(pic) => { eprintln!("[emu] camera picture {} ({}x{})", p, pic.w, pic.h); m.bus.board.set_camera_picture(pic); } Err(e) => { eprintln!("[emu] {}", e); std::process::exit(2); } } }
    m.bus.periph.lcd_cam.frame_cycles = (esp32s3::periph::CPU_HZ as f64 / cam_fps) as u64;
    if flash_mb != 8 { m.bus.flash = vec![0xff; flash_mb * 1024 * 1024]; let cap = (flash_mb * 1024 * 1024).trailing_zeros() as u8; m.bus.periph.spi1.jedec[2] = cap; m.bus.periph.spi0.jedec[2] = cap; }
    if psram_mb != 2 { m.bus.psram = vec![0; psram_mb * 1024 * 1024]; }
    if let Some(p) = &flash_image { m.write_flash(0, &std::fs::read(p).expect("flash image")).unwrap(); }
    m.trace = trace; m.trace_from = trace_from; m.breakpoints = breaks; m.bus.periph.log_unknown = log_periph;
    if let Some(r) = &rom { match std::fs::read(r) { Ok(d) => { m.load_rom(&d).expect("rom"); eprintln!("[emu] ROM loaded from {}", r.display()); } Err(e) => eprintln!("[emu] no ROM ({}): {}", r.display(), e) } }
    if let Some(p) = &bootloader { m.write_flash(0x0, &std::fs::read(p).expect("bootloader")).unwrap(); }
    if let Some(p) = &ptable { m.write_flash(0x8000, &std::fs::read(p).expect("ptable")).unwrap(); }
    if let Some(p) = &app { m.write_flash(0x10000, &std::fs::read(p).expect("app")).unwrap(); }
    for p in &elfs { m.add_symbols(&std::fs::read(p).expect("elf")).expect("elf symbols"); }
    for pre in &trace_fns {
        let mut n = 0;
        for (&a, name) in m.symbols.iter() { if name.starts_with(pre.as_str()) || (pre.ends_with('$') && name == &pre[..pre.len() - 1]) { m.fn_probes.insert(a, name.clone()); n += 1; } }
        eprintln!("[emu] --trace-fn {}: {} functions", pre, n);
    }
    for st in &stubs {
        let (name, val) = match st.split_once('=') { Some((n, v)) => (n, u32::from_str_radix(v.trim_start_matches("0x"), if v.starts_with("0x") { 16 } else { 10 }).unwrap_or(0)), None => (st.as_str(), 0) };
        let addr = if let Some(a) = m.sym_addr(name) { a } else if let Ok(a) = u32::from_str_radix(name.trim_start_matches("0x"), 16) { a } else { eprintln!("--stub: unknown symbol {}", name); std::process::exit(2) };
        eprintln!("[emu] stub {} @ {:#010x} -> returns {:#x}", name, addr, val);
        m.stubs.insert(addr, val);
    }
    match boot.as_str() {
        "app" => { let entry = m.boot_app(0x10000).expect("boot app"); eprintln!("[emu] app boot: entry {:#010x} {}", entry, m.sym(entry)); }
        "rom" => { m.boot_rom(); eprintln!("[emu] ROM boot from reset vector {:#010x}", m.cpu.pc); }
        _ => { eprintln!("--boot app|rom"); std::process::exit(2); }
    }
    for &(a, n) in &peeks { eprintln!("[peek before run]\n{}", m.peek(a, n)); }
    if let Some(wa) = watch { let v = xtensa_lx7::bus::Bus::read32(&mut m.bus, wa).unwrap_or(0); m.watch = Some((wa, v)); }
    m.stop_after_exceptions = stop_exc;
    m.console_mask = match console.as_str() { "usb" => 1, "uart0" => 2, "uart" => 2, "both" => 3, "all" => 7, "none" => 0, _ => 3 };
    m.console_prefix = console_prefix;
    if let Some(p) = &regtrace { m.regtrace = Some(std::io::BufWriter::new(std::fs::File::create(p).expect("regtrace file"))); m.regtrace_max = regtrace_max; if let Some(a) = regtrace_from_pc { m.regtrace_from_pc = Some(a); m.regtrace_armed = false; } }
    if let Some(p) = &efuse_file {
        let txt = std::fs::read_to_string(p).expect("efuse file");
        let mut n = 0;
        for line in txt.lines() {
            let line = line.trim(); if line.is_empty() { continue; }
            let (addr_s, rest) = match line.split_once(':') { Some(x) => x, None => continue };
            let Ok(mut a) = u32::from_str_radix(addr_s.trim().trim_start_matches("0x"), 16) else { continue };
            for w in rest.split_whitespace() { if let Ok(v) = u32::from_str_radix(w, 16) { let off = if a >= 0x6000_7000 { a - 0x6000_7000 } else { a }; m.bus.periph.efuse.ram.write(off, v); a += 4; n += 1; } }
        }
        eprintln!("[emu] loaded {} efuse words from {}", n, p);
    }
    if let Some(p) = &regs_init {
        let txt = std::fs::read_to_string(p).expect("regs-init file");
        let mut n = 0;
        for line in txt.lines() {
            let (addr_s, rest) = match line.trim().split_once(':') { Some(x) => x, None => continue };
            let Ok(mut a) = u32::from_str_radix(addr_s.trim().trim_start_matches("0x"), 16) else { continue };
            for w in rest.split_whitespace() { if let Ok(v) = u32::from_str_radix(w, 16) { if m.bus.periph.init_regs(a, v) { n += 1; } a += 4; } }
        }
        eprintln!("[emu] applied {} reset-state register words from {}", n, p);
    }
    if let Some(port) = web_port {
        let dir = web_dir.clone().unwrap_or_else(|| { let exe = std::env::current_exe().unwrap(); let mut d = exe.parent().unwrap().to_path_buf(); for _ in 0..3 { if d.join("web").exists() { break; } d = d.parent().unwrap().to_path_buf(); } d.join("web").to_string_lossy().to_string() });
        let w = esp32s3::web::WebServer::start(port, dir.clone()).expect("web server");
        eprintln!("[emu] board UI: http://127.0.0.1:{}/  (serving {})", port, dir);
        m.web = Some(w); m.realtime = true;
    }
    if realtime { m.realtime = true; }
    if let Some(v) = strap { m.bus.periph.gpio.strap = v; }
    if let Some(c) = reset_cause { m.bus.periph.rtc.ram.write(0x38, c | (c << 6)); }
    if profile { m.profile = Some(Default::default()); }
    if let Some(p) = &script { m.load_script(&std::fs::read_to_string(p).expect("script")).expect("script"); }
    if let Some(sec) = max_seconds { m.max_cycles = (sec * esp32s3::periph::CPU_HZ as f64) as u64; }
    m.bus.periph.spi1.log = std::env::var("ESP_EMU_DEBUG_SPI").is_ok();
    let t0 = std::time::Instant::now();
    let stop = loop {
        let stop = m.run(max_insns);
        if let Stop::SwReset = stop {
            let cause = m.bus.periph.rtc.reset_cause;
            eprintln!("[emu] chip reset at t={:.3}s: cause {:#x} ({})", m.bus.cycles as f64 / esp32s3::periph::CPU_HZ as f64, cause, esp32s3::periph::reset_cause_name(cause));
            if no_reboot || boot != "rom" { break stop; }
            m.reboot();
            continue;
        }
        break stop;
    };
    let dt = t0.elapsed().as_secs_f64();
    let total = m.cpu.insn_count + m.cpu1.insn_count;
    eprintln!("\n[emu] stop: {:?} — core0 {} + core1 {} insns in {:.1}s wall = {:.1} Minsn/s; emulated {:.3}s ({} cycles); {} exceptions, {} interrupts",
              stop, m.cpu.insn_count, m.cpu1.insn_count, dt, total as f64 / dt / 1e6, m.bus.cycles as f64 / esp32s3::periph::CPU_HZ as f64, m.bus.cycles, m.exceptions, m.interrupts);
    if let Stop::Unimplemented(pc, raw) = stop {
        if let Ok(b) = xtensa_lx7::bus::Bus::fetch(&mut m.bus, pc) { let ins = xtensa_lx7::decode(pc, b); eprintln!("[emu] unimplemented at {:08x} {}: {} (raw {:#x})", pc, m.sym(pc), xtensa_lx7::disasm::format(&ins), raw); }
    }
    if let Some((a, w)) = m.bus.last_fault { eprintln!("[emu] last bus fault: {} {:#010x}", if w { "write" } else { "read" }, a); }
    for &(a, n) in &peeks { eprintln!("[peek after run]\n{}", m.peek(a, n)); }
    for &(a, n) in &disasms { eprintln!("[disasm {:#010x}]\n{}", a, m.disasm(a, n)); }
    if profile { eprintln!("{}", m.profile_report(12)); }
    eprintln!("{}", m.irq_report());
    if let Some(w) = &wav { match m.write_wav(w) { Ok(n) => eprintln!("[emu] wrote {} samples ({:.2} s) to {}", n, n as f64 / m.bus.periph.audio().sample_rate as f64, w), Err(e) => eprintln!("[emu] wav: {}", e) } }
    eprintln!("[emu] i2s frames out: {} (i2s0) {} (i2s1)", m.bus.periph.i2s0.frames_out, m.bus.periph.i2s1.frames_out);
    if let Some(t) = m.bus.board.tft() { eprintln!("[emu] tft: {} RAMWR, {} pixels, madctl={:#x} inverted={} on={} bbox={:?} top colours {:x?}; gpio events {}", t.frames, t.pixels_written, t.madctl, t.inverted, t.on, t.bbox(), t.histogram(5), m.bus.board.gpio_events()); }
    if let (Some(path), Some(st)) = (&regstat, &m.bus.periph.regstat) {
        use std::io::Write;
        let mut rows: Vec<_> = st.iter().collect(); rows.sort_by(|a, b| b.1.0.cmp(&a.1.0));
        let mut f = std::io::BufWriter::new(std::fs::File::create(path).expect("regstat file"));
        let _ = writeln!(f, "# count kind block+off addr last_value pc symbol");
        for (&(addr, pc, wr), &(n, val)) in rows {
            let block = (addr.wrapping_sub(esp32s3::periph::PERIPH_BASE)) >> 12;
            let _ = writeln!(f, "{} {} {}+0x{:03x} {:#010x} {:#010x} {:#010x} {}", n, if wr { "wr" } else { "rd" }, esp32s3::periph::Peripherals::block_name_pub(block), addr & 0xfff, addr, val, pc, m.sym(pc));
        }
        eprintln!("[emu] wrote {} register access rows to {}", st.len(), path);
    }
    { let w = &m.bus.periph.wifi; if w.tx_frames + w.rx_frames > 0 { eprintln!("[emu] wifi: {} frames sent by the station, {} received ({} dropped: no descriptor){}", w.tx_frames, w.rx_frames, w.rx_dropped, w.ap.as_ref().map_or(String::new(), |ap| format!("; AP: {} beacons, {} probe responses, {} data frames from the station, state {:?}", ap.stats.0, ap.stats.1, ap.stats.2, ap.state))); } }
    if m.stub_hits > 0 { eprintln!("[emu] stubs hit {} times", m.stub_hits); }
    if m.bus.periph.lcd_cam.lcd_frames > 0 { eprintln!("[emu] lcd: {} RGB frames", m.bus.periph.lcd_cam.lcd_frames); }
    if m.bus.periph.lcd_cam.frames + m.bus.periph.lcd_cam.dropped > 0 { eprintln!("[emu] camera: {} frames delivered, {} dropped (no DMA/no picture)", m.bus.periph.lcd_cam.frames, m.bus.periph.lcd_cam.dropped); }
    if let Some(r) = m.bus.board.ring() { eprintln!("[emu] ring: {} updates, leds {:?}; rmt tx {}", r.updates, &r.leds[..4], m.bus.periph.rmt.tx_count); }
    if let Some(p) = &tft_png { match m.write_tft_png(p, 3) { Ok(()) => eprintln!("[emu] wrote {}", p), Err(e) => eprintln!("[emu] png: {}", e) } }
    if let Some(p) = &gram_png { match m.write_gram_png(p) { Ok(()) => eprintln!("[emu] wrote {}", p), Err(e) => eprintln!("[emu] png: {}", e) } }
    if dump_at_end { eprintln!("{}", m.dump_regs()); }
}
