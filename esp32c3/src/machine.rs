//! Machine: the C3's single RISC-V core plus the SoC bus, with the loaders and the run loop.

use crate::bus::{SocBus, DBUS_HIGH, DBUS_LOW, IBUS_HIGH, IBUS_LOW};
use crate::periph::CPU_HZ;
use esp32s3::{elf, image};
use riscv_rv32::bus::Bus;
use riscv_rv32::exec::{step, Trap};
use riscv_rv32::state::Cpu;
use std::collections::BTreeMap;

#[derive(Debug)]
pub enum Stop {
    MaxInsns,
    Halted,
    Breakpoint(u32),
    /// `ebreak` outside a handler the guest installed — a panic or an assert
    Ebreak(u32),
    SwReset,
    Exceptions(u64),
    /// `--watch`: a word changed value (addr, old, new)
    Watch(u32, u32, u32),
}

pub struct Machine {
    pub cpu: Cpu,
    pub bus: SocBus,
    pub symbols: BTreeMap<u32, String>,
    pub mac: [u8; 6],
    pub console: Vec<u8>,
    pub console_usb: Vec<u8>,
    pub console_uart0: Vec<u8>,
    /// 1 = usb, 2 = uart0
    pub console_mask: u8,
    /// When set, `drain_console` leaves the text in `console` for the caller to take instead of
    /// printing it — the WebAssembly build has no stdout worth writing to.
    pub capture_console: bool,
    pub trace: bool,
    pub trace_from: u64,
    pub breakpoints: Vec<u32>,
    /// stop when the word at this address changes
    pub watch: Option<(u32, u32)>,
    pub max_cycles: u64,
    pub exceptions: u64,
    pub interrupts: u64,
    pub stop_after_exceptions: u64,
    pub reboots: u32,
    /// interrupt-line histogram, for the end-of-run report
    pub irq_hist: [u64; 32],
}

impl Machine {
    pub fn new(mac: [u8; 6], flash_size: usize) -> Self {
        Machine {
            cpu: Cpu::new(),
            bus: SocBus::new(flash_size, mac),
            symbols: BTreeMap::new(),
            mac,
            console: Vec::new(),
            console_usb: Vec::new(),
            console_uart0: Vec::new(),
            console_mask: 3,
            capture_console: false,
            trace: false,
            trace_from: 0,
            breakpoints: Vec::new(),
            watch: None,
            max_cycles: u64::MAX,
            exceptions: 0,
            interrupts: 0,
            stop_after_exceptions: u64::MAX,
            reboots: 0,
            irq_hist: [0; 32],
        }
    }

    pub fn load_rom(&mut self, rom_elf: &[u8]) -> Result<(), String> {
        let e = elf::parse(rom_elf)?;
        for s in &e.segments {
            if s.data.is_empty() {
                continue;
            }
            self.bus.load_bytes(s.vaddr, &s.data)?;
            if s.paddr != s.vaddr {
                let _ = self.bus.load_bytes(s.paddr, &s.data);
            }
        }
        for s in &e.sections {
            if s.is_bss || s.data.is_empty() {
                continue;
            }
            let _ = self.bus.load_bytes(s.addr, &s.data);
        }
        // The ROM's `unpackloop` copies RAM initialisers out of ROM using a table of
        // (dst_start, dst_end, rom_src, pad) 16-byte entries. The ELF carries the *RAM* copies
        // (as W-only sections) but not the ROM-side originals, so unpackloop would overwrite
        // what we just loaded with zeroes — back-fill the ROM side from the RAM contents.
        let find = |n: &str| e.by_name.get(n).copied();
        let dbg = std::env::var("ESP_EMU_DEBUG").is_ok();
        if let (Some(ds), Some(de)) = (
            find("_data_end_btdm_rom").or_else(|| find("_data_start")),
            find("_data_end"),
        ) {
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
        self.symbols.extend(elf::parse(elf_bytes)?.symbols);
        Ok(())
    }

    pub fn write_flash(&mut self, offset: usize, data: &[u8]) -> Result<(), String> {
        self.bus.write_flash(offset, data)
    }

    /// Boot the application image directly, as the 2nd-stage bootloader would: copy the RAM
    /// segments, map the flash-resident ones through the MMU, jump to the entry point.
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
            let mapped = (DBUS_LOW..DBUS_HIGH).contains(&s.load_addr)
                || (IBUS_LOW..IBUS_HIGH).contains(&s.load_addr);
            if mapped {
                // The C3 has ONE 128-entry table for both buses, and software keeps the DROM and
                // IROM page ranges disjoint (`CACHE_DROM_MMU_START = CACHE_IROM_MMU_END`). Naively
                // indexing by `vaddr & 0x7FFFFF` makes 0x3C01_0000 and 0x4201_0000 collide, so a
                // direct app boot needs the bootloader's split, which we do not model yet.
                if (DBUS_LOW..DBUS_HIGH).contains(&s.load_addr) {
                    return Err(
                        "--boot app is not supported on the C3 yet: its DROM and IROM share \
                                one MMU table and need the bootloader's page split. Boot from the \
                                mask ROM instead (--boot rom with --bootloader/--ptable/--app)"
                            .into(),
                    );
                }
                if (s.load_addr & 0xffff) != (start as u32 & 0xffff) {
                    return Err(format!(
                        "segment {:#x} not page-aligned with flash offset {:#x}",
                        s.load_addr, start
                    ));
                }
                let first_page = (start as u32) >> 16;
                let npages = ((s.load_addr & 0xffff) + s.len + 0xffff) >> 16;
                for i in 0..npages {
                    self.bus.mmu[(((s.load_addr & 0x7F_FFFF) >> 16) + i) as usize] = first_page + i;
                }
            } else {
                let data = self.bus.flash[start..end].to_vec();
                self.bus.load_bytes(s.load_addr, &data)?;
            }
        }
        self.cpu.reset();
        self.cpu.pc = img.entry;
        self.cpu.x[2] = 0x3FCD_E000; // a stack the bootloader would have left us
        Ok(img.entry)
    }

    /// Cold boot from the mask ROM reset vector.
    pub fn boot_rom(&mut self) {
        self.cpu.reset();
    }

    /// Chip reset: core back to the reset vector, digital peripherals re-created, SRAM kept.
    pub fn reboot(&mut self) -> u32 {
        let cause = self.bus.periph.rtc.reset_cause;
        let old = std::mem::replace(
            &mut self.bus.periph,
            crate::periph::Peripherals::new(self.mac),
        );
        let p = &mut self.bus.periph;
        p.efuse = old.efuse;
        p.log_unknown = old.log_unknown;
        p.usb.connected = old.usb.connected;
        p.rtc.reset_cause = cause;
        // The flash chip is on the board, not in the chip: its JEDEC capacity survives a reset.
        // (Real silicon reported 4 MB on every boot; without this the emulator re-detected the
        // default 8 MB from the second boot onward.)
        p.spi0.jedec = old.spi0.jedec;
        p.spi1.jedec = old.spi1.jedec;
        p.gpio.strap = old.gpio.strap; // strapping pins are board wiring, not chip state
                                       // Publish the cause where the ROM reads it, so the boot banner says RTC_SW_CPU_RST like
                                       // real silicon rather than POWERON.
        p.rtc.ram.write(0x38, cause | (cause << 6));
        self.bus.mmu = [crate::bus::MMU_INVALID; crate::bus::MMU_ENTRIES];
        self.cpu.reset();
        self.reboots += 1;
        cause
    }

    pub fn sym(&self, pc: u32) -> String {
        match self.symbols.range(..=pc).next_back() {
            Some((&a, n)) if pc - a < 0x10000 => {
                if a == pc {
                    n.clone()
                } else {
                    format!("{}+{:#x}", n, pc - a)
                }
            }
            _ => String::new(),
        }
    }

    pub fn sym_addr(&self, name: &str) -> Option<u32> {
        self.symbols
            .iter()
            .find(|(_, n)| n.as_str() == name)
            .map(|(a, _)| *a)
    }

    /// Run until something stops us. Devices see time in quanta, as on the S3.
    pub fn run(&mut self, max_insns: u64) -> Stop {
        const QUANTUM: u64 = 64;
        let mut n = 0u64;
        loop {
            if n >= max_insns {
                self.drain_console();
                return Stop::MaxInsns;
            }
            let slow = self.trace || !self.breakpoints.is_empty() || self.watch.is_some();
            // A core in WFI with nothing pending costs nothing: let time jump to the next quantum.
            if self.cpu.waiting && !slow && self.bus.periph.intc.pending().is_none() {
                self.cpu.insn_count += QUANTUM;
            } else {
                for _ in 0..QUANTUM {
                    if let Some(stop) = self.step_one() {
                        self.drain_console();
                        return stop;
                    }
                }
            }
            n += QUANTUM;
            self.bus.cycles += QUANTUM;
            self.bus.periph.tick(QUANTUM);
            self.bus.irq_dirty = false;
            self.bus.periph.refresh_lines();
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

    #[inline]
    fn step_one(&mut self) -> Option<Stop> {
        let pc = self.cpu.pc;
        if !self.breakpoints.is_empty() && self.breakpoints.contains(&pc) && self.cpu.insn_count > 0
        {
            return Some(Stop::Breakpoint(pc));
        }
        if self.trace && self.cpu.insn_count >= self.trace_from {
            if let Ok(b) = self.bus.fetch(pc) {
                let i = riscv_rv32::decode::decode(pc, b);
                eprintln!(
                    "{:>10} {:08x}: {:<28} {}  ra={:08x} sp={:08x} a0={:08x} a1={:08x}",
                    self.cpu.insn_count,
                    pc,
                    riscv_rv32::disasm::format(&i).replace('\t', " "),
                    self.sym(pc),
                    self.cpu.x[1],
                    self.cpu.x[2],
                    self.cpu.x[10],
                    self.cpu.x[11]
                );
            }
        }
        match step(&mut self.cpu, &mut self.bus) {
            Ok(()) => {}
            Err(Trap::Interrupt(line)) => {
                self.interrupts += 1;
                self.irq_hist[(line & 31) as usize] += 1;
            }
            Err(Trap::Exception(_)) => {
                self.exceptions += 1;
            }
            Err(Trap::Ebreak(pc)) => {
                self.exceptions += 1;
                // no handler installed yet: the guest would spin in the vector, so surface it
                if self.cpu.mtvec == 0 {
                    return Some(Stop::Ebreak(pc));
                }
            }
        }
        if self.bus.irq_dirty {
            self.bus.irq_dirty = false;
            self.bus.periph.refresh_lines();
        }
        if let Some((wa, wv)) = self.watch {
            if let Ok(v) = self.bus.read32(wa) {
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

    pub fn drain_console(&mut self) {
        let usb = std::mem::take(&mut self.bus.periph.usb.tx_out);
        if !usb.is_empty() {
            self.console_usb.extend_from_slice(&usb);
            if self.console_mask & 1 != 0 {
                self.console.extend_from_slice(&usb);
            }
        }
        let u0 = std::mem::take(&mut self.bus.periph.uart[0].tx_out);
        if !u0.is_empty() {
            self.console_uart0.extend_from_slice(&u0);
            if self.console_mask & 2 != 0 {
                self.console.extend_from_slice(&u0);
            }
        }
        if !self.console.is_empty() && !self.capture_console {
            let out = std::mem::take(&mut self.console);
            print!("{}", String::from_utf8_lossy(&out));
            use std::io::Write;
            let _ = std::io::stdout().flush();
        }
    }

    pub fn seconds(&self) -> f64 {
        self.bus.cycles as f64 / CPU_HZ as f64
    }
}
