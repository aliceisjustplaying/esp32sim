//! The machine: one LX7 core + SoC bus, boot modes, run loop with tracing.
use crate::bus::*;
use crate::{elf, image};
use std::collections::BTreeMap;
use xtensa_lx7::bus::Bus;
use xtensa_lx7::state::{ps, INTTYPE_LEVEL};
use xtensa_lx7::{decode, disasm, step, Cpu, Trap};

pub struct Machine {
    pub mac: [u8; 6],
    pub reboots: u64,
    /// function stubs: at this PC (a function entry, before its `entry`), return `value` in a2 immediately
    pub stubs: std::collections::HashMap<u32, u32>,
    /// one bit per PC bucket for `stubs` / `fn_probes`, so the common case costs a shift and a test
    /// instead of hashing every PC (a hash lookup per instruction cost ~16% of run time)
    stub_bloom: u64,
    probe_bloom: u64,
    pub stub_hits: u64,
    /// function-entry tracing: pc -> name (`--trace-fn PREFIX`)
    pub fn_probes: std::collections::HashMap<u32, String>,
    pub cpu: Cpu,
    /// APP CPU (core 1)
    pub cpu1: Cpu,
    pub core1_was_reset: bool,
    pub bus: SocBus,
    pub symbols: BTreeMap<u32, String>,
    pub trace: bool,
    pub trace_from: u64,
    pub breakpoints: Vec<u32>,
    pub stop_on_unimplemented: bool,
    pub console: Vec<u8>,
    pub exceptions: u64,
    pub interrupts: u64,
    /// stop when the 32-bit word at this address changes
    pub watch: Option<(u32, u32)>,
    pub stop_after_exceptions: u64,
    /// pc histogram (enabled with --profile)
    pub profile: Option<std::collections::HashMap<u32, u64>>,
    pub irq_hist: [[u64; 32]; 2],
    /// scheduled host actions: (cycle, action), sorted by cycle
    pub script: Vec<(u64, ScriptAction)>,
    pub script_pos: usize,
    pub max_cycles: u64,
    pub script_log: bool,
    /// which consoles to mirror to stdout: bit0 = USB-CDC, bit1 = UART0, bit2 = UART1/2
    pub console_mask: u32,
    pub console_prefix: bool,
    /// compact per-instruction register trace (pc a0..a15 ps wb) for hardware comparison
    pub regtrace: Option<std::io::BufWriter<std::fs::File>>,
    pub regtrace_core: usize,
    pub regtrace_max: u64,
    /// start the register trace only once this pc is reached (None = from start)
    pub regtrace_from_pc: Option<u32>,
    pub regtrace_armed: bool,
    pub regtrace_count: u64,
    /// live web UI
    pub web: Option<crate::web::WebServer>,
    pub realtime: bool,
    wall_start: Option<std::time::Instant>,
    web_last_frame: u64,
    web_audio_sent: usize,
    web_ring_updates: u64,
    web_last_push_cycles: u64,
    pub console_usb: Vec<u8>,
    pub console_uart0: Vec<u8>,
    knob_next: u64,
    web_px_pending: u64,
    web_px_sent: u64,
    rt_last_check: u64,
    pub rt_behind: f64,
    irq_poll: u64,
    pub rt_resyncs: u64,
    pub rt_log: bool,
    rt_log_last: Option<std::time::Instant>,
    rt_log_insns: (u64, u64),
    web_cam_pushed: u64,
    web_cam_sent: bool,
}

#[derive(Clone, Debug)]
pub enum ScriptAction {
    Gpio(u8, bool),
    Serial(String),
    Stop,
    Touch(u16, u16, bool),
    Poke(u32, u32),
}

#[derive(Debug)]
pub enum Stop {
    MaxInsns,
    Breakpoint(u32),
    Unimplemented(u32, u32),
    SwReset,
    Halted,
    Simcall(u32),
    Watch(u32, u32, u32),
    Exceptions(u64),
}

use xtensa_lx7::block::pc_bit;

impl Machine {
    pub fn new(mac: [u8; 6]) -> Self {
        Machine {
            mac,
            reboots: 0,
            stubs: std::collections::HashMap::new(),
            stub_bloom: 0,
            probe_bloom: 0,
            stub_hits: 0,
            fn_probes: std::collections::HashMap::new(),
            cpu: Cpu::new(0xCDCD),
            cpu1: Cpu::new(0xABAB),
            core1_was_reset: true,
            bus: SocBus::new(8 * 1024 * 1024, 2 * 1024 * 1024, mac),
            symbols: BTreeMap::new(),
            trace: false,
            trace_from: 0,
            breakpoints: Vec::new(),
            stop_on_unimplemented: true,
            console: Vec::new(),
            exceptions: 0,
            interrupts: 0,
            watch: None,
            stop_after_exceptions: u64::MAX,
            profile: None,
            irq_hist: [[0; 32]; 2],
            script: Vec::new(),
            script_pos: 0,
            max_cycles: u64::MAX,
            script_log: true,
            console_mask: 3,
            console_prefix: false,
            regtrace: None,
            regtrace_core: 0,
            regtrace_max: u64::MAX,
            regtrace_from_pc: None,
            regtrace_armed: true,
            regtrace_count: 0,
            web: None,
            realtime: false,
            wall_start: None,
            web_last_frame: 0,
            web_audio_sent: 0,
            web_ring_updates: 0,
            web_last_push_cycles: 0,
            console_usb: Vec::new(),
            console_uart0: Vec::new(),
            knob_next: 0,
            web_px_pending: 0,
            web_px_sent: 0,
            rt_last_check: 0,
            rt_behind: 0.0,
            irq_poll: 0,
            rt_resyncs: 0,
            rt_log: std::env::var("ESP_EMU_RT_LOG").is_ok(),
            rt_log_last: None,
            rt_log_insns: (0, 0),
            web_cam_pushed: u64::MAX,
            web_cam_sent: false,
        }
    }

    pub fn load_rom(&mut self, rom_elf: &[u8]) -> Result<(), String> {
        let e = elf::parse(rom_elf)?;
        for s in &e.segments {
            if s.data.is_empty() {
                continue;
            }
            self.bus.load_bytes(s.vaddr, &s.data)?;
            // the mask ROM also holds the initialiser image at paddr (copied by the reset handler)
            if s.paddr != s.vaddr {
                let _ = self.bus.load_bytes(s.paddr, &s.data);
            }
        }
        // RAM initialisers live in sections without program headers (.data.interface.*, .data_*)
        let dbg = std::env::var("ESP_EMU_DEBUG").is_ok();
        if dbg {
            eprintln!(
                "[emu] rom: {} segments, {} alloc sections",
                e.segments.len(),
                e.sections.len()
            );
        }
        for s in &e.sections {
            if dbg {
                eprintln!(
                    "[emu]   section {:<36} addr {:#010x} len {:#x} bss={}",
                    s.name,
                    s.addr,
                    s.data.len(),
                    s.is_bss
                );
            }
            if s.is_bss || s.data.is_empty() {
                continue;
            }
            if let Err(err) = self.bus.load_bytes(s.addr, &s.data) {
                eprintln!("[emu] rom section {} @ {:#x}: {}", s.name, s.addr, err);
            }
        }
        // The reset handler copies RAM initialisers from ROM using a 16-byte-entry table
        // (dst_start, dst_end, rom_src, 0) between _data_start and _data_end. The ELF does
        // not carry the ROM-side copies for the W-only sections, so back-fill them from the
        // RAM contents we just loaded.
        let find = |name: &str| e.by_name.get(name).copied();
        if let (Some(ds), Some(de)) = (find("_data_start"), find("_data_end")) {
            let mut t = ds;
            let mut n = 0;
            while t + 16 <= de {
                let (Ok(d0), Ok(d1), Ok(src)) = (
                    self.bus.read32(t),
                    self.bus.read32(t + 4),
                    self.bus.read32(t + 8),
                ) else {
                    break;
                };
                if d1 > d0 && d1 - d0 < 0x20000 {
                    let bytes: Vec<u8> = (d0..d1).map(|a| self.bus.read8(a).unwrap_or(0)).collect();
                    if self.bus.load_bytes(src, &bytes).is_ok() {
                        n += 1;
                    }
                }
                t += 16;
            }
            if dbg {
                eprintln!(
                    "[emu] rom: back-filled {} initialiser blocks into ROM from table {:#x}..{:#x}",
                    n, ds, de
                );
            }
        }
        self.symbols.extend(e.symbols);
        Ok(())
    }

    pub fn add_symbols(&mut self, elf_bytes: &[u8]) -> Result<(), String> {
        let e = elf::parse(elf_bytes)?;
        self.symbols.extend(e.symbols);
        Ok(())
    }

    pub fn write_flash(&mut self, offset: usize, data: &[u8]) -> Result<(), String> {
        if offset + data.len() > self.bus.flash.len() {
            return Err("flash image too large".into());
        }
        self.bus.flash[offset..offset + data.len()].copy_from_slice(data);
        self.bus
            .note_written(crate::bus::SRC_FLASH, offset, data.len());
        Ok(())
    }

    /// Boot the application image at flash `app_off` the way the 2nd-stage bootloader
    /// would: copy IRAM/DRAM segments, map IROM/DROM through the MMU, jump to entry.
    pub fn boot_app(&mut self, app_off: usize) -> Result<u32, String> {
        self.bus.periph.system.preset_after_bootloader();
        self.bus.periph.rtc.preset_after_bootloader();
        let img = image::parse(&self.bus.flash[app_off..])?;
        for s in &img.segments {
            let start = app_off + s.file_off as usize;
            let end = start + s.len as usize;
            if end > self.bus.flash.len() {
                return Err("segment beyond flash".into());
            }
            let flash_mapped = (DBUS_LOW..DBUS_HIGH).contains(&s.load_addr)
                || (IBUS_LOW..IBUS_HIGH).contains(&s.load_addr);
            if flash_mapped {
                // esptool aligns segments so vaddr and flash offset agree modulo 64 KiB
                if (s.load_addr & 0xffff) != (start as u32 & 0xffff) {
                    return Err(format!(
                        "segment {:#x} not page-aligned with flash offset {:#x}",
                        s.load_addr, start
                    ));
                }
                let first_page = (start as u32) >> 16;
                let npages = ((s.load_addr & 0xffff) + s.len + 0xffff) >> 16;
                for i in 0..npages {
                    let vpage = (((s.load_addr & 0x1FF_FFFF) >> 16) + i) as usize;
                    self.bus.mmu[vpage] = first_page + i;
                }
                self.bus.invalidate_tlb();
            } else {
                let data = self.bus.flash[start..end].to_vec();
                self.bus.load_bytes(s.load_addr, &data)?;
            }
        }
        self.cpu.reset();
        self.cpu.pc = img.entry;
        self.cpu.ps = ps::WOE | ps::UM; // windows enabled, user vector; INTLEVEL 0
        self.cpu.vecbase = 0x4000_0000;
        self.cpu.set_ar(1, 0x3FCE_B000); // bootloader stack (in DRAM, app treats as free)
        self.cpu.set_ar(0, 0);
        Ok(img.entry)
    }

    /// Cold boot from the mask ROM reset vector (needs ROM + flash image with bootloader).
    pub fn boot_rom(&mut self) {
        self.cpu.reset();
    }

    /// Chip reset (software / watchdog): CPUs back to the reset vector, digital peripherals re-initialised,
    /// cache MMU invalid; SRAM, RTC memories, efuses and the RTC-domain registers survive, as on silicon.
    /// Returns the reset cause that the ROM will report.
    pub fn reboot(&mut self) -> u32 {
        let cause = self.bus.periph.rtc.reset_cause;
        let old = std::mem::replace(
            &mut self.bus.periph,
            crate::periph::Peripherals::new(self.mac),
        );
        let p = &mut self.bus.periph;
        p.efuse = old.efuse;
        p.gpio.strap = old.gpio.strap;
        p.log_unknown = old.log_unknown;
        p.spi1.log = old.spi1.log;
        p.rtc.ram = old.rtc.ram;
        p.rtc.slow_ticks = old.rtc.slow_ticks;
        p.rtc.ram.write(0x38, cause | (cause << 6));
        p.rtc.ram.write(0x98, 0); // watchdog disarmed by the reset; the ROM re-arms it
        p.i2s0.pcm = old.i2s0.pcm;
        p.i2s0.frames_out = old.i2s0.frames_out;
        p.i2s1.pcm = old.i2s1.pcm;
        p.i2s1.frames_out = old.i2s1.frames_out; // keep the captured audio continuous
        self.bus.mmu = [crate::bus::MMU_INVALID; crate::bus::MMU_ENTRIES];
        self.bus.invalidate_tlb();
        self.bus.irq_dirty = true;
        self.cpu.reset();
        self.cpu1.reset();
        self.cpu1.prid = 0xABAB;
        self.core1_was_reset = true;
        self.reboots += 1;
        cause
    }

    /// Address of a symbol loaded from the ELFs.
    pub fn sym_addr(&self, name: &str) -> Option<u32> {
        self.symbols
            .iter()
            .find(|(_, n)| n.as_str() == name)
            .map(|(&a, _)| a)
    }

    pub fn sym(&self, addr: u32) -> String {
        match self.symbols.range(..=addr).next_back() {
            Some((&a, n)) if addr - a < 0x10000 => {
                if a == addr {
                    n.clone()
                } else {
                    format!("{}+{:#x}", n, addr - a)
                }
            }
            _ => String::new(),
        }
    }

    fn drain_console(&mut self) {
        use std::io::Write;
        let mut o = std::io::stdout();
        let mut emit =
            |bit: u32, tag: &str, d: Vec<u8>, console: &mut Vec<u8>, mask: u32, prefix: bool| {
                if d.is_empty() {
                    return;
                }
                console.extend_from_slice(&d);
                if mask & bit == 0 {
                    return;
                }
                if prefix {
                    for line in d.split_inclusive(|&b| b == b'\n') {
                        let _ = o.write_all(tag.as_bytes());
                        let _ = o.write_all(line);
                    }
                } else {
                    let _ = o.write_all(&d);
                }
                let _ = o.flush();
            };
        let d = std::mem::take(&mut self.bus.periph.usb.tx_out);
        self.console_usb.extend_from_slice(&d);
        if self.console_usb.len() > 65536 {
            let cut = self.console_usb.len() - 49152;
            self.console_usb.drain(..cut);
        }
        if let Some(w) = &self.web {
            if !d.is_empty() {
                w.send_text(&format!(
                    "{{\"t\":\"serial\",\"src\":\"usb\",\"data\":\"{}\"}}",
                    crate::web::json_escape(&String::from_utf8_lossy(&d))
                ));
            }
        }
        emit(
            1,
            "[usb]  ",
            d,
            &mut self.console,
            self.console_mask,
            self.console_prefix,
        );
        for u in 0..3 {
            let d = std::mem::take(&mut self.bus.periph.uart[u].tx_out);
            if u == 0 {
                self.console_uart0.extend_from_slice(&d);
                if self.console_uart0.len() > 65536 {
                    let cut = self.console_uart0.len() - 49152;
                    self.console_uart0.drain(..cut);
                }
            }
            if let Some(w) = &self.web {
                if !d.is_empty() {
                    w.send_text(&format!(
                        "{{\"t\":\"serial\",\"src\":\"uart{}\",\"data\":\"{}\"}}",
                        u,
                        crate::web::json_escape(&String::from_utf8_lossy(&d))
                    ));
                }
            }
            let (bit, tag) = if u == 0 {
                (2, "[uart0] ")
            } else if u == 1 {
                (4, "[uart1] ")
            } else {
                (4, "[uart2] ")
            };
            emit(
                bit,
                tag,
                d,
                &mut self.console,
                self.console_mask,
                self.console_prefix,
            );
        }
    }

    /// Execute up to `budget` instructions on `core` through the block interpreter. Returns the
    /// iterations consumed (as `step_core` would have counted them) and a stop, if any.
    #[inline]
    fn step_blocks(&mut self, core: usize, budget: u32) -> (u32, Option<Stop>) {
        let (cpu, bus) = if core == 0 {
            (&mut self.cpu, &mut self.bus)
        } else {
            (&mut self.cpu1, &mut self.bus)
        };
        let pc = cpu.pc;
        // stubs and probes are block boundaries, so testing them at block start is exact
        if (self.stub_bloom | self.probe_bloom) & pc_bit(pc) != 0 && !cpu.waiting {
            if let Some(name) = self.fn_probes.get(&pc) {
                eprintln!(
                    "[fn] i={} t={:.4}s c{} {}(a2={:#x} a3={:#x} a4={:#x}) ret={:#x}",
                    cpu.insn_count,
                    bus.cycles as f64 / crate::periph::CPU_HZ as f64,
                    core,
                    name,
                    cpu.get_ar(2),
                    cpu.get_ar(3),
                    cpu.get_ar(4),
                    cpu.get_ar(0) & 0x3fff_ffff | 0x4000_0000
                );
            }
            if let Some(&ret) = self.stubs.get(&pc) {
                let a0 = cpu.get_ar(0);
                cpu.set_ar(2, ret);
                cpu.pc = (a0 & 0x3fff_ffff) | (pc & 0xc000_0000);
                cpu.insn_count += 1;
                cpu.advance_ccount(1);
                self.stub_hits += 1;
                return (1, None);
            }
        }
        let (used, trap) = xtensa_lx7::block::run_block(cpu, bus, budget);
        match trap {
            None => {}
            Some(Trap::Exception(_)) => {
                self.exceptions += 1;
            }
            Some(Trap::Interrupt(irq)) => {
                self.interrupts += 1;
                self.irq_hist[core][irq as usize] += 1;
            }
            Some(Trap::Unimplemented(p, raw)) => {
                if self.stop_on_unimplemented {
                    return (used, Some(Stop::Unimplemented(p, raw)));
                }
            }
            Some(Trap::Simcall) => return (used, Some(Stop::Simcall(pc))),
        }
        if bus.irq_dirty {
            bus.irq_dirty = false;
            if bus.periph.lines_dirty() || bus.periph.intmatrix_dirty {
                bus.periph.intmatrix_dirty = false;
                let (l0, l1) = bus.periph.cpu_lines_both();
                self.cpu.interrupt = (self.cpu.interrupt & !INTTYPE_LEVEL) | (l0 & INTTYPE_LEVEL);
                self.cpu1.interrupt = (self.cpu1.interrupt & !INTTYPE_LEVEL) | (l1 & INTTYPE_LEVEL);
            }
        }
        if self.exceptions >= self.stop_after_exceptions {
            return (used, Some(Stop::Exceptions(self.exceptions)));
        }
        (used, None)
    }

    /// Execute one instruction on `core`; returns Some(stop) if the run must end.
    #[inline]
    fn step_core(&mut self, core: usize) -> Option<Stop> {
        let (cpu, bus) = if core == 0 {
            (&mut self.cpu, &mut self.bus)
        } else {
            (&mut self.cpu1, &mut self.bus)
        };
        let pc = cpu.pc;
        if self.probe_bloom & pc_bit(pc) != 0 && !cpu.waiting {
            if let Some(name) = self.fn_probes.get(&pc) {
                eprintln!(
                    "[fn] i={} t={:.4}s c{} {}(a2={:#x} a3={:#x} a4={:#x}) ret={:#x}",
                    cpu.insn_count,
                    bus.cycles as f64 / crate::periph::CPU_HZ as f64,
                    core,
                    name,
                    cpu.get_ar(2),
                    cpu.get_ar(3),
                    cpu.get_ar(4),
                    cpu.get_ar(0) & 0x3fff_ffff | 0x4000_0000
                );
            }
        }
        if self.stub_bloom & pc_bit(pc) != 0 && !cpu.waiting {
            if let Some(&ret) = self.stubs.get(&pc) {
                // synthetic return from a windowed function entry (its `entry` has not executed): a0 holds
                // the return address with the call increment in bits 31:30; no window rotation to undo
                let a0 = cpu.get_ar(0);
                cpu.set_ar(2, ret);
                cpu.pc = (a0 & 0x3fff_ffff) | (pc & 0xc000_0000);
                cpu.insn_count += 1;
                cpu.advance_ccount(1);
                self.stub_hits += 1;
                return None;
            }
        }
        if !self.breakpoints.is_empty() && self.breakpoints.contains(&pc) && cpu.insn_count > 0 {
            return Some(Stop::Breakpoint(pc));
        }
        if self.trace && cpu.insn_count >= self.trace_from {
            if let Ok(b) = bus.fetch(pc) {
                let i = decode(pc, b);
                let sym = match self.symbols.range(..=pc).next_back() {
                    Some((&a, n)) if pc - a < 0x10000 => {
                        if a == pc {
                            n.clone()
                        } else {
                            format!("{}+{:#x}", n, pc - a)
                        }
                    }
                    _ => String::new(),
                };
                eprintln!("{}{:>10} {:08x}: {:<32} {}  a0={:08x} a1={:08x} a2={:08x} a3={:08x} ps={:06x} wb={}", if core == 1 { "C1 " } else { "" }, cpu.insn_count, pc, disasm::format(&i), sym,
                          cpu.get_ar(0), cpu.get_ar(1), cpu.get_ar(2), cpu.get_ar(3), cpu.ps, cpu.windowbase);
            }
        }
        if core == self.regtrace_core && !cpu.waiting && self.regtrace.is_some() {
            if !self.regtrace_armed && self.regtrace_from_pc == Some(pc) {
                self.regtrace_armed = true;
            }
            if self.regtrace_armed && self.regtrace_count >= self.regtrace_max {
                return Some(Stop::Halted);
            }
            if self.regtrace_armed {
                self.regtrace_count += 1;
            }
            if let Some(w) = &mut self.regtrace {
                if !self.regtrace_armed { /* not yet */
                } else {
                    use std::io::Write;
                    let _ = write!(w, "{:08x}", pc);
                    for i in 0..16u8 {
                        let _ = write!(w, " {:08x}", cpu.get_ar(i));
                    }
                    let _ = writeln!(w, " {:08x} {:x}", cpu.ps, cpu.windowbase);
                }
            }
        }
        bus.periph.cur_pc = pc;
        if let Some(h) = &mut self.profile {
            *h.entry(pc).or_insert(0) += 1;
        }
        match step(cpu, bus) {
            Ok(()) => {}
            Err(Trap::Exception(c)) => {
                self.exceptions += 1;
                if self.trace {
                    eprintln!("          ** core{} exception cause {} at {:08x} -> {:08x} (excvaddr {:08x})", core, c, pc, cpu.pc, cpu.excvaddr);
                }
            }
            Err(Trap::Interrupt(irq)) => {
                self.interrupts += 1;
                self.irq_hist[core][irq as usize] += 1;
                if self.trace {
                    eprintln!(
                        "          ** core{} interrupt {} at {:08x} -> {:08x}",
                        core, irq, pc, cpu.pc
                    );
                }
            }
            Err(Trap::Unimplemented(p, raw)) => {
                if self.stop_on_unimplemented {
                    return Some(Stop::Unimplemented(p, raw));
                }
            }
            Err(Trap::Simcall) => return Some(Stop::Simcall(pc)),
        }
        if bus.irq_dirty {
            bus.irq_dirty = false;
            if bus.periph.lines_dirty() || bus.periph.intmatrix_dirty {
                bus.periph.intmatrix_dirty = false;
                let (l0, l1) = bus.periph.cpu_lines_both();
                self.cpu.interrupt = (self.cpu.interrupt & !INTTYPE_LEVEL) | (l0 & INTTYPE_LEVEL);
                self.cpu1.interrupt = (self.cpu1.interrupt & !INTTYPE_LEVEL) | (l1 & INTTYPE_LEVEL);
            }
        }
        if let Some((wa, wv)) = self.watch {
            if let Ok(v) = bus.read32(wa) {
                if v != wv {
                    self.watch = Some((wa, v));
                    return Some(Stop::Watch(wa, wv, v));
                }
            }
        }
        if self.exceptions >= self.stop_after_exceptions {
            return Some(Stop::Exceptions(self.exceptions));
        }
        None
    }

    pub fn run(&mut self, max_insns: u64) -> Stop {
        const QUANTUM: u64 = 64;
        self.stub_bloom = self.stubs.keys().fold(0, |m, &pc| m | pc_bit(pc));
        self.probe_bloom = self.fn_probes.keys().fold(0, |m, &pc| m | pc_bit(pc));
        for c in [&mut self.cpu, &mut self.cpu1] {
            c.boundary_bloom = self.stub_bloom | self.probe_bloom;
            c.blocks.flush();
        }
        // the block interpreter cannot honour per-instruction observers; those runs single-step
        let blocks = !(self.trace
            || self.profile.is_some()
            || self.regtrace.is_some()
            || self.watch.is_some()
            || !self.breakpoints.is_empty());
        let mut n = 0u64;
        loop {
            if n >= max_insns {
                self.drain_console();
                return Stop::MaxInsns;
            }
            let (clk, reset, stall) = self.bus.periph.core1_control();
            if reset {
                self.core1_was_reset = true;
            }
            let core1_on = clk && !stall && !reset;
            if core1_on && self.core1_was_reset {
                self.core1_was_reset = false;
                self.cpu1.reset();
                self.cpu1.prid = 0xABAB;
                if self.trace {
                    eprintln!("          ** core1 released from reset");
                }
            }
            let c0_idle = self.cpu.waiting && self.cpu.check_interrupts_pending() == 0;
            let c1_idle =
                !core1_on || (self.cpu1.waiting && self.cpu1.check_interrupts_pending() == 0);
            let slow_path = self.trace
                || self.profile.is_some()
                || self.regtrace.is_some()
                || self.watch.is_some();
            if c0_idle && c1_idle && !slow_path {
                // both cores asleep: let time pass in larger steps until a device raises a line
                let chunk = QUANTUM * 8;
                self.cpu.advance_ccount(chunk as u32);
                if core1_on {
                    self.cpu1.advance_ccount(chunk as u32);
                }
                n += chunk;
                self.after_round(chunk);
                if self.bus.periph.rtc.sw_reset {
                    self.drain_console();
                    return Stop::SwReset;
                }
                if self.bus.cycles >= self.max_cycles {
                    self.drain_console();
                    return Stop::Halted;
                }
                if n & 0xffff < chunk {
                    self.drain_console();
                }
                continue;
            }
            if c0_idle && !slow_path {
                self.cpu.advance_ccount(QUANTUM as u32);
            } else if blocks {
                let mut left = QUANTUM as u32;
                while left > 0 {
                    let (used, stop) = self.step_blocks(0, left);
                    if let Some(stop) = stop {
                        self.drain_console();
                        return stop;
                    }
                    left -= used.min(left);
                }
            } else {
                for _ in 0..QUANTUM {
                    if let Some(stop) = self.step_core(0) {
                        self.drain_console();
                        return stop;
                    }
                }
            }
            n += QUANTUM;
            if core1_on {
                if c1_idle && !slow_path {
                    self.cpu1.advance_ccount(QUANTUM as u32);
                } else if blocks {
                    let mut left = QUANTUM as u32;
                    while left > 0 {
                        let (used, stop) = self.step_blocks(1, left);
                        if let Some(stop) = stop {
                            self.drain_console();
                            return stop;
                        }
                        left -= used.min(left);
                    }
                } else {
                    for _ in 0..QUANTUM {
                        if let Some(stop) = self.step_core(1) {
                            self.drain_console();
                            return stop;
                        }
                    }
                }
            }
            self.after_round(QUANTUM);
            if self.bus.periph.rtc.sw_reset {
                self.drain_console();
                return Stop::SwReset;
            }
            if self.bus.cycles >= self.max_cycles {
                self.drain_console();
                return Stop::Halted;
            }
            if n & 0xffff < QUANTUM {
                self.drain_console();
            }
        }
    }

    /// Device time, interrupt lines, scripts, web, real-time pacing after a scheduling round.
    #[inline]
    fn after_round(&mut self, cycles: u64) {
        // device models only change state when they run, so the lines are re-derived after a
        // flush or a register write and never on a fixed cadence
        let ticked = self.bus.tick(cycles as u32) != 0;
        if self.bus.irq_dirty || ticked {
            let dirty = self.bus.periph.lines_dirty() || self.bus.periph.intmatrix_dirty;
            self.irq_poll += 1;
            self.bus.irq_dirty = false;
            self.bus.periph.intmatrix_dirty = false;
            if dirty {
                let (l0, l1) = self.bus.periph.cpu_lines_both();
                self.cpu.interrupt = (self.cpu.interrupt & !INTTYPE_LEVEL) | (l0 & INTTYPE_LEVEL);
                self.cpu1.interrupt = (self.cpu1.interrupt & !INTTYPE_LEVEL) | (l1 & INTTYPE_LEVEL);
            }
            return self.after_round_rest();
        }
        self.after_round_rest();
    }

    #[inline]
    fn after_round_rest(&mut self) {
        while self.script_pos < self.script.len()
            && self.script[self.script_pos].0 <= self.bus.cycles
        {
            let (t, a) = self.script[self.script_pos].clone();
            self.script_pos += 1;
            if self.script_log {
                eprintln!(
                    "[script] t={:.3}s {:?}",
                    t as f64 / crate::periph::CPU_HZ as f64,
                    a
                );
            }
            match a {
                ScriptAction::Gpio(pin, level) => {
                    self.bus.periph.gpio.set_input(pin, level);
                    self.bus.irq_dirty = true;
                }
                ScriptAction::Serial(text) => self.bus.periph.usb.host_input(text.as_bytes()),
                ScriptAction::Stop => {
                    self.max_cycles = 0;
                }
                ScriptAction::Touch(x, y, d) => {
                    self.bus.board.touch(x, y, d);
                }
                ScriptAction::Poke(a, v) => {
                    let _ = self.bus.write32(a, v);
                }
            }
        }
        if self.web.is_some()
            && self.bus.cycles.wrapping_sub(self.web_last_push_cycles) >= crate::periph::CPU_HZ / 50
        {
            self.web_last_push_cycles = self.bus.cycles;
            self.web_push();
            self.web_poll_input();
        }
        if self.realtime && self.bus.cycles.wrapping_sub(self.rt_last_check) >= 1 << 16 {
            self.rt_last_check = self.bus.cycles;
            let start = *self.wall_start.get_or_insert_with(std::time::Instant::now);
            let emulated = std::time::Duration::from_secs_f64(
                self.bus.cycles as f64 / crate::periph::CPU_HZ as f64,
            );
            let wall = start.elapsed();
            if emulated > wall + std::time::Duration::from_millis(2) {
                std::thread::sleep(emulated - wall);
                self.rt_behind = 0.0;
            } else if wall > emulated + std::time::Duration::from_millis(50) {
                self.rt_behind = (wall - emulated).as_secs_f64();
                // more than half a second behind: resynchronise (skip the lag) rather than flood the client while catching up
                if wall > emulated + std::time::Duration::from_millis(500) {
                    self.rt_resyncs += 1;
                    self.wall_start = Some(std::time::Instant::now() - emulated);
                }
            } else {
                self.rt_behind = 0.0;
            }
        }
    }

    /// Send display / audio / ring updates to the browser (called ~50x per emulated second).
    fn web_push(&mut self) {
        if self.rt_log {
            let now = std::time::Instant::now();
            let (i0, i1) = (self.cpu.insn_count, self.cpu1.insn_count);
            if let Some(last) = self.rt_log_last {
                let dt = now.duration_since(last).as_secs_f64() * 1e3;
                if dt > 40.0 {
                    eprintln!("[rt] t={:.2}s window took {:.0} ms: core0 {} insns (pc {:08x} {}), core1 {} insns (pc {:08x} {})", self.bus.cycles as f64 / crate::periph::CPU_HZ as f64, dt,
                              i0 - self.rt_log_insns.0, self.cpu.pc, self.sym(self.cpu.pc), i1 - self.rt_log_insns.1, self.cpu1.pc, self.sym(self.cpu1.pc));
                }
            }
            self.rt_log_last = Some(now);
            self.rt_log_insns = (i0, i1);
        }
        let Some(w) = self.web.clone() else { return };
        self.drain_console();
        if self.bus.board.tft().is_none() {
            if let Some((w_, h_, px, ver)) = self.bus.board.display() {
                if ver != self.web_px_sent {
                    self.web_px_sent = ver;
                    let mut b = vec![1u8, w_ as u8, (w_ >> 8) as u8, h_ as u8, (h_ >> 8) as u8];
                    for p in &px {
                        b.push(*p as u8);
                        b.push((*p >> 8) as u8);
                    }
                    w.send_binary(&b);
                }
            }
        }
        if let Some(tft) = self.bus.board.tft() {
            // send a frame once the pixel stream has been quiet for a push interval (avoids half-drawn frames)
            let px = tft.pixels_written;
            if px != self.web_px_pending {
                self.web_px_pending = px;
            } else if px != self.web_px_sent {
                self.web_px_sent = px;
                self.web_last_frame = tft.frames;
                let f = tft.frame_160x80();
                let mut b = vec![1u8, 160, 0, 80, 0];
                for p in &f {
                    b.push(*p as u8);
                    b.push((*p >> 8) as u8);
                }
                w.send_binary(&b);
            }
        }
        if self.web_cam_pushed != self.bus.periph.lcd_cam.frames / 20 || !self.web_cam_sent {
            if let Some(rgb) = self.bus.board.camera_preview(320, 240) {
                let mut b = vec![
                    4u8,
                    (320u16 & 255) as u8,
                    (320u16 >> 8) as u8,
                    (240u16 & 255) as u8,
                    (240u16 >> 8) as u8,
                ];
                b.extend_from_slice(&rgb);
                w.send_binary(&b);
                self.web_cam_sent = true;
            }
            self.web_cam_pushed = self.bus.periph.lcd_cam.frames / 20;
        }
        let audio = self.bus.periph.audio();
        let (pcm, rate) = (&audio.pcm, audio.sample_rate);
        if pcm.len() > self.web_audio_sent {
            let chunk = &pcm[self.web_audio_sent..];
            let mut b = vec![2u8];
            b.extend_from_slice(&rate.to_le_bytes());
            for s in chunk {
                b.extend_from_slice(&s.to_le_bytes());
            }
            w.send_binary(&b);
            self.web_audio_sent = pcm.len();
        }
        if let Some(ring) = self.bus.board.ring() {
            if ring.updates != self.web_ring_updates {
                self.web_ring_updates = ring.updates;
                let leds: Vec<String> = ring
                    .leds
                    .iter()
                    .map(|c| format!("[{},{},{}]", c[0], c[1], c[2]))
                    .collect();
                w.send_text(&format!("{{\"t\":\"ring\",\"leds\":[{}]}}", leds.join(",")));
            }
        }
        // snapshot for late-joining clients: backlog, frame, ring
        {
            use crate::web::json_escape;
            let mut hello: Vec<Vec<u8>> = Vec::new();
            let mk = |s: &str| -> Vec<u8> {
                let mut f = vec![0x81u8];
                let n = s.len();
                if n < 126 {
                    f.push(n as u8);
                } else if n < 65536 {
                    f.push(126);
                    f.extend_from_slice(&(n as u16).to_be_bytes());
                } else {
                    f.push(127);
                    f.extend_from_slice(&(n as u64).to_be_bytes());
                }
                f.extend_from_slice(s.as_bytes());
                f
            };
            let mkb = |d: &[u8]| -> Vec<u8> {
                let mut f = vec![0x82u8];
                let n = d.len();
                if n < 126 {
                    f.push(n as u8);
                } else if n < 65536 {
                    f.push(126);
                    f.extend_from_slice(&(n as u16).to_be_bytes());
                } else {
                    f.push(127);
                    f.extend_from_slice(&(n as u64).to_be_bytes());
                }
                f.extend_from_slice(d);
                f
            };
            hello.push(mk(&format!(
                "{{\"t\":\"serial\",\"src\":\"uart0\",\"data\":\"{}\"}}",
                json_escape(&String::from_utf8_lossy(&self.console_uart0))
            )));
            hello.push(mk(&format!(
                "{{\"t\":\"serial\",\"src\":\"usb\",\"data\":\"{}\"}}",
                json_escape(&String::from_utf8_lossy(&self.console_usb))
            )));
            hello.push(mk(&format!(
                "{{\"t\":\"board\",\"name\":\"{}\"}}",
                self.bus.board.name()
            )));
            if let Some((w_, h_, px, _)) = self.bus.board.display() {
                let mut b = vec![1u8, w_ as u8, (w_ >> 8) as u8, h_ as u8, (h_ >> 8) as u8];
                for p in &px {
                    b.push(*p as u8);
                    b.push((*p >> 8) as u8);
                }
                hello.push(mkb(&b));
            }
            if let Some(rgb) = self.bus.board.camera_preview(320, 240) {
                let mut b = vec![
                    4u8,
                    (320u16 & 255) as u8,
                    (320u16 >> 8) as u8,
                    (240u16 & 255) as u8,
                    (240u16 >> 8) as u8,
                ];
                b.extend_from_slice(&rgb);
                hello.push(mkb(&b));
            }
            if let Some(ring) = self.bus.board.ring() {
                let leds: Vec<String> = ring
                    .leds
                    .iter()
                    .map(|c| format!("[{},{},{}]", c[0], c[1], c[2]))
                    .collect();
                hello.push(mk(&format!(
                    "{{\"t\":\"ring\",\"leds\":[{}]}}",
                    leds.join(",")
                )));
            }
            w.set_hello(hello);
        }
        let t = self.bus.cycles as f64 / crate::periph::CPU_HZ as f64;
        w.send_text(&format!("{{\"t\":\"stat\",\"time\":{:.2},\"insns\":{},\"frames\":{},\"behind\":{:.2},\"resyncs\":{},\"cam\":{},\"gpio_in\":\"{:x}\"}}", t, self.cpu.insn_count + self.cpu1.insn_count, self.bus.board.tft().map_or(self.bus.periph.lcd_cam.lcd_frames, |t| t.frames), self.rt_behind, self.rt_resyncs, self.bus.periph.lcd_cam.frames, self.bus.periph.gpio.input));
    }

    fn web_poll_input(&mut self) {
        let Some(w) = self.web.clone() else { return };
        use crate::board::*;
        use crate::web::json_str;
        for b in w.poll_incoming_bin() {
            // type 3: camera picture from the browser — [3][w u16 le][h u16 le][RGBA...]
            if b.len() >= 5 && b[0] == 3 {
                let (wd, ht) = (
                    u16::from_le_bytes([b[1], b[2]]) as usize,
                    u16::from_le_bytes([b[3], b[4]]) as usize,
                );
                if wd > 0 && ht > 0 && b.len() >= 5 + wd * ht * 4 {
                    let mut rgb = Vec::with_capacity(wd * ht * 3);
                    for px in b[5..5 + wd * ht * 4].chunks(4) {
                        rgb.extend_from_slice(&px[..3]);
                    }
                    self.bus.board.set_camera_picture(crate::picture::Picture {
                        w: wd as u32,
                        h: ht as u32,
                        rgb,
                    });
                }
            }
        }
        for m in w.poll_incoming() {
            let t = json_str(&m, "t").unwrap_or_default();
            match t.as_str() {
                "btn" => {
                    let pin: u8 = json_str(&m, "pin")
                        .and_then(|x| x.parse().ok())
                        .unwrap_or(0);
                    let v = json_str(&m, "v").unwrap_or_default() == "1";
                    self.bus.periph.gpio.set_input(pin, !v);
                    self.bus.irq_dirty = true;
                }
                "knobpress" => {
                    let v = json_str(&m, "v").unwrap_or_default() == "1";
                    self.bus.periph.gpio.set_input(PIN_ENC_SW, !v);
                    self.bus.irq_dirty = true;
                }
                "knob" => {
                    let d: i32 = json_str(&m, "d").and_then(|x| x.parse().ok()).unwrap_or(1);
                    let hz = crate::periph::CPU_HZ;
                    let step = hz / 500; // 2 ms per phase
                    let mut tc = (self.bus.cycles + step).max(self.knob_next); // queue detents back to back, never overlapping
                    let seq: [(u8, bool); 4] = if d > 0 {
                        [
                            (PIN_ENC_CLK, false),
                            (PIN_ENC_DT, false),
                            (PIN_ENC_CLK, true),
                            (PIN_ENC_DT, true),
                        ]
                    } else {
                        [
                            (PIN_ENC_DT, false),
                            (PIN_ENC_CLK, false),
                            (PIN_ENC_DT, true),
                            (PIN_ENC_CLK, true),
                        ]
                    };
                    for _ in 0..d.unsigned_abs() {
                        for (pn, l) in seq {
                            self.script.push((tc, ScriptAction::Gpio(pn, l)));
                            tc += step;
                        }
                        tc += step * 4;
                    }
                    self.knob_next = tc;
                    self.script.sort_by_key(|e| e.0);
                    self.script_pos = self
                        .script
                        .iter()
                        .position(|e| e.0 > self.bus.cycles)
                        .unwrap_or(self.script.len());
                }
                "serial" => {
                    let line = json_str(&m, "line").unwrap_or_default();
                    self.bus
                        .periph
                        .usb
                        .host_input(format!("{}\n", line).as_bytes());
                }
                "touch" => {
                    let x: u16 = json_str(&m, "x").and_then(|v| v.parse().ok()).unwrap_or(0);
                    let y: u16 = json_str(&m, "y").and_then(|v| v.parse().ok()).unwrap_or(0);
                    let down = json_str(&m, "down").unwrap_or_default() == "1";
                    self.bus.board.touch(x, y, down);
                }
                _ => {}
            }
        }
    }

    /// Write captured I2S audio (left channel) as a 16-bit mono WAV.
    pub fn write_wav(&self, path: &str) -> std::io::Result<usize> {
        let a = self.bus.periph.audio();
        let pcm = &a.pcm;
        let rate = a.sample_rate;
        let mut out = Vec::with_capacity(44 + pcm.len() * 2);
        let data_len = (pcm.len() * 2) as u32;
        out.extend_from_slice(b"RIFF");
        out.extend_from_slice(&(36 + data_len).to_le_bytes());
        out.extend_from_slice(b"WAVEfmt ");
        out.extend_from_slice(&16u32.to_le_bytes());
        out.extend_from_slice(&1u16.to_le_bytes());
        out.extend_from_slice(&1u16.to_le_bytes());
        out.extend_from_slice(&rate.to_le_bytes());
        out.extend_from_slice(&(rate * 2).to_le_bytes());
        out.extend_from_slice(&2u16.to_le_bytes());
        out.extend_from_slice(&16u16.to_le_bytes());
        out.extend_from_slice(b"data");
        out.extend_from_slice(&data_len.to_le_bytes());
        for s in pcm {
            out.extend_from_slice(&s.to_le_bytes());
        }
        std::fs::write(path, out)?;
        Ok(pcm.len())
    }

    pub fn irq_report(&self) -> String {
        let mut s =
            String::from("[irq] per core, cpu-int: count (peripheral sources mapped to it)\n");
        for core in 0..2 {
            for irq in 0..32 {
                let n = self.irq_hist[core][irq];
                if n == 0 {
                    continue;
                }
                let srcs: Vec<String> = (0..crate::periph::NUM_SOURCES)
                    .filter(|&src| self.bus.periph.intmatrix.map[core][src] == irq as u32)
                    .map(|src| src.to_string())
                    .collect();
                s += &format!(
                    "  core{} int{:<2} {:>9}  sources [{}]\n",
                    core,
                    irq,
                    n,
                    srcs.join(",")
                );
            }
        }
        s
    }

    /// Save the TFT frame (160x80, scaled) as PNG.
    pub fn write_tft_png(&self, path: &str, scale: usize) -> std::io::Result<()> {
        let Some((w, h, px, _)) = self.bus.board.display() else {
            return Err(std::io::Error::other("this board has no display"));
        };
        write_png_rgb565(
            path,
            &px,
            w as usize,
            h as usize,
            if w > 200 { 1 } else { scale },
        )
    }
    pub fn write_gram_png(&self, path: &str) -> std::io::Result<()> {
        let Some(t) = self.bus.board.tft() else {
            return Err(std::io::Error::other("this board has no TFT"));
        };
        write_png_rgb565(
            path,
            &t.gram,
            crate::board::St7735::COLS,
            crate::board::St7735::ROWS,
            2,
        )
    }

    /// Parse a script: one action per line, `<seconds> <cmd> [args]`.
    ///   press <pin> [ms]   release <pin>   gpio <pin> <0|1>   serial <text...>   knob <cw|ccw> [detents]   touch <x> <y> <0|1>   stop
    /// Buttons/encoder are active-low with pull-ups (release = 1).
    pub fn load_script(&mut self, text: &str) -> Result<(), String> {
        use crate::board::*;
        let hz = crate::periph::CPU_HZ as f64;
        let mut ev: Vec<(u64, ScriptAction)> = Vec::new();
        for (ln, line) in text.lines().enumerate() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let mut it = line.splitn(2, char::is_whitespace);
            let t: f64 = it
                .next()
                .unwrap()
                .parse()
                .map_err(|_| format!("line {}: bad time", ln + 1))?;
            let after = it.next().unwrap_or("").trim_start();
            let cmd = after.split_whitespace().next().unwrap_or("");
            let rest = after[cmd.len()..].trim();
            let pin = |s: &str| -> Result<u8, String> {
                match s {
                    "btn1" => Ok(PIN_BTN1),
                    "btn2" => Ok(PIN_BTN2),
                    "sw" | "knob" => Ok(PIN_ENC_SW),
                    _ => s.parse().map_err(|_| format!("line {}: bad pin", ln + 1)),
                }
            };
            let c = (t * hz) as u64;
            match cmd {
                "press" => {
                    let mut p = rest.split_whitespace();
                    let pn = pin(p.next().unwrap_or(""))?;
                    let ms: f64 = p
                        .next()
                        .map(|x| x.parse().unwrap_or(100.0))
                        .unwrap_or(100.0);
                    ev.push((c, ScriptAction::Gpio(pn, false)));
                    ev.push((c + (ms / 1000.0 * hz) as u64, ScriptAction::Gpio(pn, true)));
                }
                "release" => ev.push((c, ScriptAction::Gpio(pin(rest)?, true))),
                "gpio" => {
                    let mut p = rest.split_whitespace();
                    let pn = pin(p.next().unwrap_or(""))?;
                    let l = p.next().unwrap_or("1") == "1";
                    ev.push((c, ScriptAction::Gpio(pn, l)));
                }
                "poke" => {
                    let mut p = rest.split_whitespace();
                    let a =
                        u32::from_str_radix(p.next().unwrap_or("0").trim_start_matches("0x"), 16)
                            .map_err(|e| e.to_string())?;
                    let v =
                        u32::from_str_radix(p.next().unwrap_or("0").trim_start_matches("0x"), 16)
                            .map_err(|e| e.to_string())?;
                    ev.push((c, ScriptAction::Poke(a, v)));
                }
                "touch" => {
                    let mut p = rest.split_whitespace();
                    let x: u16 = p.next().and_then(|v| v.parse().ok()).unwrap_or(0);
                    let y: u16 = p.next().and_then(|v| v.parse().ok()).unwrap_or(0);
                    let d = p.next().unwrap_or("1") == "1";
                    ev.push((c, ScriptAction::Touch(x, y, d)));
                }
                "serial" => ev.push((c, ScriptAction::Serial(format!("{}\n", rest)))),
                "knob" => {
                    let mut p = rest.split_whitespace();
                    let dir = p.next().unwrap_or("cw");
                    let n: usize = p.next().map(|x| x.parse().unwrap_or(1)).unwrap_or(1);
                    let step = (0.002 * hz) as u64; // 2 ms per quadrature phase
                    let mut tc = c;
                    for _ in 0..n {
                        // idle (1,1); CW: CLK falls while DT=1, then DT falls, CLK rises, DT rises. CCW: DT first.
                        let seq: [(u8, bool); 4] = if dir == "cw" {
                            [
                                (PIN_ENC_CLK, false),
                                (PIN_ENC_DT, false),
                                (PIN_ENC_CLK, true),
                                (PIN_ENC_DT, true),
                            ]
                        } else {
                            [
                                (PIN_ENC_DT, false),
                                (PIN_ENC_CLK, false),
                                (PIN_ENC_DT, true),
                                (PIN_ENC_CLK, true),
                            ]
                        };
                        for (pn, l) in seq {
                            ev.push((tc, ScriptAction::Gpio(pn, l)));
                            tc += step;
                        }
                        tc += step * 4;
                    }
                }
                "stop" => ev.push((c, ScriptAction::Stop)),
                _ => return Err(format!("line {}: unknown command {}", ln + 1, cmd)),
            }
        }
        ev.sort_by_key(|e| e.0);
        self.script = ev;
        self.script_pos = 0;
        Ok(())
    }

    pub fn profile_report(&self, top: usize) -> String {
        let Some(h) = &self.profile else {
            return String::new();
        };
        let mut v: Vec<(u32, u64)> = h.iter().map(|(a, c)| (*a, *c)).collect();
        v.sort_by(|a, b| b.1.cmp(&a.1));
        let total: u64 = v.iter().map(|x| x.1).sum();
        let mut s = format!("[profile] top {} pcs of {} instructions\n", top, total);
        for (a, c) in v.iter().take(top) {
            s += &format!(
                "  {:08x} {:>6.2}%  {}\n",
                a,
                *c as f64 * 100.0 / total as f64,
                self.sym(*a)
            );
        }
        s
    }

    pub fn disasm(&mut self, addr: u32, n: usize) -> String {
        let mut s = String::new();
        let mut pc = addr;
        for _ in 0..n {
            let Ok(b) = self.bus.fetch(pc) else { break };
            let i = decode(pc, b);
            s += &format!("{:08x}: {:<30} {}\n", pc, disasm::format(&i), self.sym(pc));
            pc += i.len as u32;
        }
        s
    }

    pub fn peek(&mut self, addr: u32, words: usize) -> String {
        let mut s = String::new();
        for i in 0..words {
            let a = addr.wrapping_add((i * 4) as u32);
            s += &format!(
                "{:08x}: {}\n",
                a,
                match self.bus.read32(a) {
                    Ok(v) => format!("{:08x}", v),
                    Err(_) => "--------".into(),
                }
            );
        }
        s
    }

    pub fn dump_regs(&self) -> String {
        let mut out = self.dump_core(&self.cpu, 0);
        if !self.core1_was_reset {
            out += &self.dump_core(&self.cpu1, 1);
        }
        out
    }

    fn dump_core(&self, c: &Cpu, core: usize) -> String {
        let mut s = format!("core{}: ", core);
        s += &format!("pc={:08x} {}  ps={:08x} wb={} ws={:04x} sar={} lcount={} exccause={} excvaddr={:08x} epc1={:08x} intenable={:08x} interrupt={:08x} ccount={} insns={}\n",
            c.pc, self.sym(c.pc), c.ps, c.windowbase, c.windowstart, c.sar, c.lcount, c.exccause, c.excvaddr, c.epc[1], c.intenable, c.interrupt, c.ccount, c.insn_count);
        for i in 0..16 {
            s += &format!("a{:<2}={:08x} ", i, c.get_ar(i));
            if i % 8 == 7 {
                s += "\n";
            }
        }
        s
    }
}

/// Minimal PNG writer (RGB565 -> RGB8, uncompressed deflate blocks) — no zlib dependency.
pub fn write_png_rgb565(
    path: &str,
    px: &[u16],
    w: usize,
    h: usize,
    scale: usize,
) -> std::io::Result<()> {
    fn crc32(data: &[u8]) -> u32 {
        let mut c = 0xffff_ffffu32;
        for &b in data {
            c ^= b as u32;
            for _ in 0..8 {
                c = if c & 1 != 0 {
                    0xEDB8_8320 ^ (c >> 1)
                } else {
                    c >> 1
                };
            }
        }
        !c
    }
    fn adler(data: &[u8]) -> u32 {
        let (mut a, mut b) = (1u32, 0u32);
        for &d in data {
            a = (a + d as u32) % 65521;
            b = (b + a) % 65521;
        }
        (b << 16) | a
    }
    fn chunk(out: &mut Vec<u8>, t: &[u8], d: &[u8]) {
        out.extend_from_slice(&(d.len() as u32).to_be_bytes());
        let mut td = t.to_vec();
        td.extend_from_slice(d);
        out.extend_from_slice(&td);
        out.extend_from_slice(&crc32(&td).to_be_bytes());
    }
    let (out_w, out_h) = (w * scale, h * scale);
    let mut raw = Vec::with_capacity(out_h * (out_w * 3 + 1));
    for y in 0..out_h {
        raw.push(0);
        for x in 0..out_w {
            let p = px[(y / scale) * w + x / scale] as u32;
            raw.push(((p >> 11) * 255 / 31) as u8);
            raw.push((((p >> 5) & 63) * 255 / 63) as u8);
            raw.push(((p & 31) * 255 / 31) as u8);
        }
    }
    // zlib stream with stored (uncompressed) deflate blocks
    let mut z = vec![0x78, 0x01];
    for (i, blk) in raw.chunks(65535).enumerate() {
        let last = (i + 1) * 65535 >= raw.len();
        z.push(last as u8);
        z.extend_from_slice(&(blk.len() as u16).to_le_bytes());
        z.extend_from_slice(&(!(blk.len() as u16)).to_le_bytes());
        z.extend_from_slice(blk);
    }
    z.extend_from_slice(&adler(&raw).to_be_bytes());
    let mut out = vec![137, 80, 78, 71, 13, 10, 26, 10];
    let mut ihdr = Vec::new();
    ihdr.extend_from_slice(&(out_w as u32).to_be_bytes());
    ihdr.extend_from_slice(&(out_h as u32).to_be_bytes());
    ihdr.extend_from_slice(&[8, 2, 0, 0, 0]);
    chunk(&mut out, b"IHDR", &ihdr);
    chunk(&mut out, b"IDAT", &z);
    chunk(&mut out, b"IEND", &[]);
    std::fs::write(path, out)
}
