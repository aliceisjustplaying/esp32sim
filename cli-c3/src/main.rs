//! esp32sim-c3 — run ESP32-C3 firmware. Mirrors the flags of the ESP32-S3 `esp32sim` binary;
//! see docs/esp32c3.md. The two will merge behind a `--chip` flag once the C3 model is complete.
use esp32c3::{Machine, Stop};
use riscv_rv32::bus::Bus;

fn usage() -> ! {
    eprintln!("usage: esp32sim-c3 [--boot rom|app] [--rom ELF] [--bootloader BIN] [--ptable BIN] [--app BIN]");
    eprintln!("                   [--elf ELF]... [--flash-mb N] [--flash-image BIN] [--flash-at OFF=FILE]...");
    eprintln!("                   [--max-seconds S] [--max-insns N] [--console usb|uart0|both|none]  (default uart0:");
    eprintln!("                   the ROM mirrors its output to both, so \"both\" prints everything twice)");
    eprintln!("                   [--trace [--trace-from N]] [--break ADDR] [--log-periph] [--peek ADDR[,N]]");
    eprintln!("                   [--disasm ADDR[,N]] [--watch ADDR] [--stop-after-exceptions N] [--no-reboot] [--serial TEXT]");
    eprintln!("                   [--mac xx:xx:xx:xx:xx:xx] [--reset-cause N] [--strap N]   (match a real board)");
    std::process::exit(2)
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let (mut boot, mut rom, mut bootloader, mut ptable, mut app) =
        ("rom".to_string(), None::<String>, None, None, None);
    let (mut elfs, mut flash_mb, mut flash_image): (Vec<String>, usize, Option<String>) =
        (Vec::new(), 4, None);
    let (mut max_seconds, mut max_insns, mut console) =
        (None::<f64>, u64::MAX, "uart0".to_string());
    let (mut trace, mut trace_from, mut breaks) = (false, 0u64, Vec::new());
    let (mut log_periph, mut peeks, mut disasms) = (false, Vec::new(), Vec::new());
    let (mut stop_exc, mut no_reboot, mut serial) = (u64::MAX, false, None::<String>);
    let mut watch: Option<u32> = None;
    let (mut mac_arg, mut reset_cause, mut strap) = (None::<String>, None::<u32>, None::<u32>);
    let mut flash_at: Vec<String> = Vec::new();

    let mut i = 1;
    let num = |s: &str| -> u32 {
        u32::from_str_radix(
            s.trim_start_matches("0x"),
            if s.starts_with("0x") { 16 } else { 10 },
        )
        .unwrap_or(0)
    };
    let pair = |s: &str| -> (u32, usize) {
        match s.split_once(',') {
            Some((a, n)) => (num(a), n.parse().unwrap_or(1)),
            None => (num(s), 1),
        }
    };
    while i < args.len() {
        let a = args[i].clone();
        let mut next = || {
            i += 1;
            args.get(i).cloned().unwrap_or_else(|| usage())
        };
        match a.as_str() {
            "--boot" => boot = next(),
            "--rom" => rom = Some(next()),
            "--bootloader" => bootloader = Some(next()),
            "--ptable" => ptable = Some(next()),
            "--app" => app = Some(next()),
            "--elf" => elfs.push(next()),
            "--flash-mb" => flash_mb = next().parse().unwrap_or(4),
            "--flash-image" => flash_image = Some(next()),
            "--flash-at" => flash_at.push(next()),
            "--max-seconds" => max_seconds = next().parse().ok(),
            "--max-insns" => max_insns = next().parse().unwrap_or(u64::MAX),
            "--console" => console = next(),
            "--trace" => trace = true,
            "--trace-from" => trace_from = next().parse().unwrap_or(0),
            "--break" => {
                let v = next();
                breaks.push(num(&v));
            }
            "--log-periph" => log_periph = true,
            "--peek" => {
                let v = next();
                peeks.push(pair(&v));
            }
            "--disasm" => {
                let v = next();
                disasms.push(pair(&v));
            }
            "--stop-after-exceptions" => stop_exc = next().parse().unwrap_or(u64::MAX),
            "--watch" => {
                let v = next();
                watch = Some(num(&v));
            }
            "--mac" => mac_arg = Some(next()),
            "--reset-cause" => {
                let v = next();
                reset_cause = Some(num(&v));
            }
            "--strap" => {
                let v = next();
                strap = Some(num(&v));
            }
            "--no-reboot" => no_reboot = true,
            "--serial" => serial = Some(next()),
            "-h" | "--help" => usage(),
            _ => {
                eprintln!("unknown arg {}", a);
                usage()
            }
        }
        i += 1;
    }

    let mut mac = [0x60, 0x55, 0xf9, 0x00, 0x11, 0x22];
    if let Some(t) = &mac_arg {
        let b: Vec<u8> = t
            .split(':')
            .filter_map(|x| u8::from_str_radix(x, 16).ok())
            .collect();
        if b.len() == 6 {
            mac.copy_from_slice(&b);
        } else {
            eprintln!("--mac wants xx:xx:xx:xx:xx:xx");
            std::process::exit(2);
        }
    }
    let mut m = Machine::new(mac, flash_mb * 1024 * 1024);
    m.bus.periph.log_unknown = log_periph;
    m.bus.periph.spi1.log = std::env::var("ESP_EMU_DEBUG_SPI").is_ok();
    m.trace = trace;
    m.trace_from = trace_from;
    m.breakpoints = breaks;
    m.stop_after_exceptions = stop_exc;
    if let Some(a) = watch {
        m.watch = Some((a, 0));
    }
    m.console_mask = match console.as_str() {
        "usb" => 1,
        "uart0" => 2,
        "none" => 0,
        _ => 3,
    };
    let cap = (flash_mb * 1024 * 1024).trailing_zeros() as u8;
    m.bus.periph.spi1.jedec[2] = cap;
    m.bus.periph.spi0.jedec[2] = cap;

    // The mask ROM ELF ships with ESP-IDF; rev3 is what the current C3 modules are.
    let rom_path = rom.map(std::path::PathBuf::from).or_else(|| {
        let home = std::env::var("HOME").ok()?;
        let dir = std::path::Path::new(&home).join(".espressif/tools/esp-rom-elfs");
        let mut best: Option<std::path::PathBuf> = None;
        for e in std::fs::read_dir(dir).ok()? {
            let p = e.ok()?.path().join("esp32c3_rev3_rom.elf");
            if p.exists() {
                best = Some(p);
            }
        }
        best
    });
    if boot == "rom" {
        match &rom_path {
            Some(p) => match std::fs::read(p) {
                Ok(d) => {
                    m.load_rom(&d).expect("rom");
                    eprintln!("[emu] ROM loaded from {}", p.display());
                }
                Err(e) => {
                    eprintln!("[emu] {}: {}", p.display(), e);
                    std::process::exit(2)
                }
            },
            None => {
                eprintln!("[emu] no esp32c3 mask ROM ELF found (pass --rom, or use --boot app)");
                std::process::exit(2)
            }
        }
    }
    if let Some(p) = &flash_image {
        m.write_flash(0, &std::fs::read(p).expect("flash image"))
            .unwrap();
    }
    if let Some(p) = &bootloader {
        m.write_flash(0x0, &std::fs::read(p).expect("bootloader"))
            .unwrap();
    }
    if let Some(p) = &ptable {
        m.write_flash(0x8000, &std::fs::read(p).expect("ptable"))
            .unwrap();
    }
    if let Some(p) = &app {
        m.write_flash(0x10000, &std::fs::read(p).expect("app"))
            .unwrap();
    }
    for spec in &flash_at {
        let Some((off, path)) = spec.split_once('=') else {
            eprintln!("--flash-at needs OFFSET=FILE");
            std::process::exit(2)
        };
        let off = usize::from_str_radix(off.trim_start_matches("0x"), 16).unwrap_or_else(|_| {
            eprintln!("--flash-at: bad offset");
            std::process::exit(2)
        });
        let data = std::fs::read(path).unwrap_or_else(|e| {
            eprintln!("--flash-at: {}: {}", path, e);
            std::process::exit(2)
        });
        m.write_flash(off, &data).unwrap();
        eprintln!("[emu] flash {:#x}: {} ({} bytes)", off, path, data.len());
    }
    for p in &elfs {
        m.add_symbols(&std::fs::read(p).expect("elf"))
            .expect("elf symbols");
    }
    if let Some(s) = &serial {
        m.bus.periph.usb.host_input(s.as_bytes());
    }

    if boot == "app" {
        match m.boot_app(0x10000) {
            Ok(e) => eprintln!("[emu] booting app image directly, entry {:#010x}", e),
            Err(e) => {
                eprintln!("[emu] {}", e);
                std::process::exit(2)
            }
        }
    } else {
        m.boot_rom();
        eprintln!("[emu] ROM boot from reset vector {:#010x}", m.cpu.pc);
    }
    // Match a real board's boot conditions for a differential run: the ROM prints the reset cause
    // and the strapping-derived boot mode, and takes a different path for a non-power-on reset.
    if let Some(c) = reset_cause {
        m.bus.periph.rtc.ram.write(0x38, c | (c << 6));
        m.bus.periph.rtc.reset_cause = c;
    }
    if let Some(v) = strap {
        m.bus.periph.gpio.strap = v;
    }
    if let Some(s) = max_seconds {
        m.max_cycles = (s * esp32c3::periph::CPU_HZ as f64) as u64;
    }
    if let Some((a, _)) = m.watch {
        let v = m.bus.read32(a).unwrap_or(0);
        m.watch = Some((a, v));
    }
    for &(a, n) in &peeks {
        eprintln!("[peek before run]\n{}", peek(&mut m, a, n));
    }

    let t0 = std::time::Instant::now();
    let stop = loop {
        let s = m.run(max_insns);
        if let Stop::SwReset = s {
            let cause = m.bus.periph.rtc.reset_cause;
            eprintln!(
                "[emu] chip reset at t={:.3}s: cause {:#x} ({})",
                m.seconds(),
                cause,
                esp32s3::periph::reset_cause_name(cause)
            );
            if no_reboot || boot != "rom" {
                break s;
            }
            m.reboot();
            continue;
        }
        break s;
    };
    let dt = t0.elapsed().as_secs_f64();
    m.drain_console();
    eprintln!("\n[emu] stop: {:?} — {} insns in {:.1}s wall = {:.1} Minsn/s; emulated {:.3}s ({} cycles); {} exceptions, {} interrupts",
              stop, m.cpu.insn_count, dt, m.cpu.insn_count as f64 / dt / 1e6, m.seconds(), m.bus.cycles, m.exceptions, m.interrupts);
    eprintln!(
        "[emu] pc={:#010x} {}  mtvec={:#010x} mcause={:#010x} mepc={:#010x}",
        m.cpu.pc,
        m.sym(m.cpu.pc),
        m.cpu.mtvec,
        m.cpu.mcause,
        m.cpu.mepc
    );
    if let Some((a, w)) = m.bus.last_fault {
        eprintln!(
            "[emu] last bus fault: {} {:#010x}",
            if w { "write" } else { "read" },
            a
        );
    }
    let irqs: Vec<String> = (0..32)
        .filter(|&n| m.irq_hist[n] > 0)
        .map(|n| format!("int{}:{}", n, m.irq_hist[n]))
        .collect();
    if !irqs.is_empty() {
        eprintln!("[emu] interrupt lines taken: {}", irqs.join(" "));
    }
    for &(a, n) in &peeks {
        eprintln!("[peek after run]\n{}", peek(&mut m, a, n));
    }
    for &(a, n) in &disasms {
        eprintln!("[disasm {:#010x}]\n{}", a, disasm(&mut m, a, n));
    }
}

fn peek(m: &mut Machine, addr: u32, n: usize) -> String {
    (0..n)
        .map(|i| {
            let a = addr + 4 * i as u32;
            format!(
                "{:08x}: {}",
                a,
                m.bus
                    .read32(a)
                    .map(|v| format!("{:08x}", v))
                    .unwrap_or_else(|_| "--------".into())
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn disasm(m: &mut Machine, addr: u32, n: usize) -> String {
    let mut out = Vec::new();
    let mut pc = addr;
    for _ in 0..n {
        let Ok(b) = m.bus.fetch(pc) else { break };
        let i = riscv_rv32::decode::decode(pc, b);
        out.push(format!(
            "{:08x}: {:<30} {}",
            pc,
            riscv_rv32::disasm::format(&i).replace('\t', " "),
            m.sym(pc)
        ));
        pc += i.len as u32;
    }
    out.join("\n")
}
