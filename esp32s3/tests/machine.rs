//! The machine without a ROM: the scheduler's idle skipping, a second core released from reset,
//! what a chip reset keeps, action-script parsing against the board, console capture. Programs
//! are two instructions long; the goldens cover real firmware.
use emu_core::{Bus, Core};
use esp32s3::board::{make_board, NoBoard};
use esp_soc::{BrowserExternalBlockRefusal, ScriptAction, Stop};

const IRAM: u32 = 0x4037_0000;
const RESET: u32 = 0x4000_0400;
const WAITI_LOOP: [u8; 6] = [0x00, 0x70, 0x00, 0x06, 0xff, 0xff];   // waiti 0 ; j .
const SPIN: [u8; 3] = [0x06, 0xff, 0xff];                             // j .   (objdump: ffff06)

fn machine() -> esp32s3::Machine { let mut m = esp32s3::machine([1, 2, 3, 4, 5, 6]); m.console.capture = true; m }
fn park(m: &mut esp32s3::Machine, core: usize, at: u32, prog: &[u8]) { esp_soc::SocBus::load_bytes(&mut m.bus, at, prog).unwrap(); m.cores[core].pc = at; m.cores[core].ps = 0; }

#[test]
fn peripheral_block_names_cover_aes_neighbors() {
    use esp32s3::periph::Peripherals;
    assert_eq!(Peripherals::block_name_pub(0x39), "USB_WRAP");
    assert_eq!(Peripherals::block_name_pub(0x3a), "AES");
    assert_eq!(Peripherals::block_name_pub(0x3b), "SHA");
}

/// A core in `waiti` with nothing pending costs no instructions: a millisecond of emulated time
/// passes in a few hundred scheduling steps.
#[test]
fn idle_cores_skip_time() {
    let mut m = machine();
    park(&mut m, 0, IRAM, &WAITI_LOOP);
    m.max_cycles = 240_000;                                   // 1 ms
    assert!(matches!(m.run(u64::MAX), Stop::Halted));
    assert!(m.bus.cycles >= 240_000);
    assert!(m.insns() < 1000, "{} instructions for 1 ms of sleep", m.insns());
    assert!(m.cores[0].waiting());
}

/// Core 1 sits in reset until SYSTEM_CORE_1_CONTROL_0 releases it, then runs from the reset vector.
#[test]
fn core1_runs_when_released() {
    let mut m = machine();
    park(&mut m, 0, IRAM, &SPIN);
    esp_soc::SocBus::load_bytes(&mut m.bus, RESET, &SPIN).unwrap();
    m.max_cycles = 64 * 100;
    m.run(u64::MAX);
    assert_eq!(m.cores[1].insn_count(), 0, "held in reset");
    m.bus.write32(0x600c_0000, 0b010).unwrap();                // clock on, not stalled, not in reset
    m.max_cycles = 64 * 300;
    m.run(u64::MAX);
    assert!(m.cores[1].insn_count() > 0, "core 1 ran");
    assert_eq!(m.cores[1].pc(), RESET);
    assert_eq!(m.cores[1].prid, 0xABAB);
}

#[test]
fn browser_external_blocks_are_single_core_scheduler_transactions() {
    let mut m = machine();
    park(&mut m, 0, IRAM, &SPIN);
    assert_eq!(m.browser_external_block_budget(1), Some(64));
    let quantum = m.browser_external_quantum(64).unwrap();
    assert!(m.finish_browser_external_quantum(quantum).is_none());
    assert_eq!(m.bus.cycles, 64);

    m.bus.write32(0x600c_0000, 0b010).unwrap();
    assert_eq!(m.browser_external_block_budget(7), None, "core 1 is running");
    assert_eq!(
        m.browser_external_block_budget_result(7),
        Err(BrowserExternalBlockRefusal::OtherCoreActive.bit() | BrowserExternalBlockRefusal::BusBreak.bit())
    );
    let refusals = m.browser_external_block_refusal_mask(7);
    assert_ne!(
        refusals & BrowserExternalBlockRefusal::OtherCoreActive.bit(),
        0
    );
    assert_ne!(refusals & BrowserExternalBlockRefusal::BusBreak.bit(), 0);
}

#[test]
fn browser_external_blocks_advance_an_idle_released_core() {
    let mut m = machine();
    park(&mut m, 0, IRAM, &SPIN);
    esp_soc::SocBus::load_bytes(&mut m.bus, RESET, &WAITI_LOOP).unwrap();
    m.bus.write32(0x600c_0000, 0b010).unwrap();
    m.max_cycles = 128;
    assert!(matches!(m.run(u64::MAX), Stop::Halted));
    assert!(m.cores[1].waiting());
    assert_eq!(m.browser_external_block_budget(64), Some(64));

    let before = m.cores[1].ccount;
    m.max_cycles = u64::MAX;
    let quantum = m.browser_external_quantum(64).unwrap();
    assert!(m.finish_browser_external_quantum(quantum).is_none());
    assert_eq!(m.cores[1].ccount.wrapping_sub(before), 64);
}

#[test]
fn browser_external_blocks_deliver_scripts_after_the_quantum() {
    let mut m = machine();
    park(&mut m, 0, IRAM, &SPIN);
    m.load_script("0.0000001 serial hello\n").unwrap();
    assert_eq!(m.browser_external_block_budget(64), Some(64));
    assert_eq!(m.script.pos, 0);

    let quantum = m.browser_external_quantum(64).unwrap();
    assert!(m.finish_browser_external_quantum(quantum).is_none());
    assert_eq!(m.bus.cycles, 64);
    assert_eq!(m.script.pos, 1);
}

// Exercise the external API against the scheduler that defines its behavior. The core-0
// program runs through the real block interpreter; only its scheduler entry point differs.
fn external_round(m: &mut esp32s3::Machine) -> Option<Stop> {
    let Ok(quantum) = m.browser_external_quantum(64) else {
        return match m.run(64) { Stop::MaxInsns => None, stop => Some(stop) };
    };
    let mut left = quantum.budget();
    while left > 0 {
        let (used, trap) = m.cores[0].run(&mut m.bus, left);
        assert!(trap.is_none(), "core 0 unexpectedly trapped: {trap:?}");
        assert!(used > 0);
        left -= used;
    }
    m.finish_browser_external_quantum(quantum)
}

fn idle_peer_machine() -> esp32s3::Machine {
    let mut m = machine();
    park(&mut m, 0, IRAM, &SPIN);
    esp_soc::SocBus::load_bytes(&mut m.bus, RESET, &WAITI_LOOP).unwrap();
    m.bus.write32(0x600c_0000, 0b010).unwrap();
    m.max_cycles = 128;
    assert!(matches!(m.run(u64::MAX), Stop::Halted));
    m.max_cycles = u64::MAX;
    m.cores[1].ps = 0;
    assert!(m.cores[1].waiting());
    m
}

fn assert_scheduler_equal(a: &esp32s3::Machine, b: &esp32s3::Machine) {
    assert_eq!(a.bus.cycles, b.bus.cycles);
    assert_eq!(a.script.pos, b.script.pos);
    assert_eq!(a.max_cycles, b.max_cycles);
    assert_eq!(a.console.all, b.console.all);
    assert_eq!(a.dump_regs(), b.dump_regs(), "held-core bookkeeping must agree");
    for (a, b) in a.cores.iter().zip(&b.cores) {
        assert_eq!((a.pc, a.ccount, a.insn_count), (b.pc, b.ccount, b.insn_count));
        assert_eq!((a.waiting, a.interrupt, a.ps), (b.waiting, b.interrupt, b.ps));
    }
}

#[test]
fn browser_external_scheduler_matches_idle_peer_and_timer_wake() {
    for timer in [false, true] {
        let (mut ordinary, mut external) = (idle_peer_machine(), idle_peer_machine());
        if timer {
            for m in [&mut ordinary, &mut external] {
                let vector = m.cores[1].vecbase + xtensa_lx7::state::vec::KERNEL;
                esp_soc::SocBus::load_bytes(&mut m.bus, vector, &SPIN).unwrap();
                m.cores[1].intenable = 1 << xtensa_lx7::state::TIMER_INTERRUPT[0];
                m.cores[1].ccompare[0] = m.cores[1].ccount + 64 * 3 + 17;
            }
        }
        for _ in 0..400 {
            assert!(matches!(ordinary.run(64), Stop::MaxInsns));
            assert!(external_round(&mut external).is_none());
            assert_scheduler_equal(&ordinary, &external);
        }
    }
}

#[test]
fn browser_external_scheduler_reports_script_stop_and_drains_console() {
    let (mut ordinary, mut external) = (idle_peer_machine(), idle_peer_machine());
    for m in [&mut ordinary, &mut external] {
        m.load_script("0.0000100 serial hello\n0.0000200 stop\n").unwrap();
    }
    for _ in 0..100 {
        // New output is produced within this round, including the stopping round.
        ordinary.bus.periph.usb.tx_out.push(b'x');
        external.bus.periph.usb.tx_out.push(b'x');
        let stop = ordinary.run(64);
        let external_stop = external_round(&mut external);
        // The ordinary run drains at its slice boundary; external finish must do the same.
        assert_scheduler_equal(&ordinary, &external);
        if matches!(stop, Stop::Halted) {
            assert!(matches!(external_stop, Some(Stop::Halted)));
            assert_eq!(external.max_cycles, 0);
            assert!(external.bus.periph.usb.tx_out.is_empty());
            return;
        }
        assert!(matches!(stop, Stop::MaxInsns));
        assert!(external_stop.is_none());
    }
    panic!("script stop was never reported");
}

#[test]
fn browser_external_scheduler_keeps_peer_snapshot_across_reset_store() {
    let (mut ordinary, mut external) = (idle_peer_machine(), idle_peer_machine());
    for m in [&mut ordinary, &mut external] {
        // s32i.n a2,a3,0 ; j . — the store resets core 1 after this round's peer snapshot.
        park(m, 0, IRAM, &[0x29, 0x03, 0x06, 0xff, 0xff]);
        m.cores[0].set_ar(2, 0b110);
        m.cores[0].set_ar(3, 0x600c_0000);
    }
    let before = ordinary.cores[1].ccount;
    assert!(matches!(ordinary.run(64), Stop::MaxInsns));
    assert!(external_round(&mut external).is_none());
    assert_scheduler_equal(&ordinary, &external);
    assert_eq!(external.cores[1].ccount.wrapping_sub(before), 64);
    assert!(matches!(ordinary.run(64), Stop::MaxInsns));
    assert!(external_round(&mut external).is_none());
    assert_scheduler_equal(&ordinary, &external);
    assert_eq!(external.cores[1].ccount.wrapping_sub(before), 64, "reset peer stops next round");
}

/// A chip reset re-creates the digital peripherals but keeps the efuses, the straps, the RTC
/// domain and the captured audio, and publishes the cause where the ROM reads it.
#[test]
fn reboot_keeps_what_silicon_keeps() {
    let mut m = machine();
    m.bus.periph.efuse.ram.write(0x44, 0xdead_beef);
    m.bus.periph.gpio.strap = 0x7;
    m.bus.periph.rtc.ram.write(0x120, 0x1234);
    m.bus.periph.rtc.slow_ticks = 999;
    m.bus.periph.i2s0.pcm = vec![1, 2, 3]; m.bus.periph.i2s0.frames_out = 3;
    m.bus.periph.uart[0].tx_out = b"gone".to_vec();
    m.bus.periph.systimer.conf = 0xffff;
    m.cores[0].pc = IRAM;
    m.bus.periph.rtc.reset_cause = esp_periph::RST_SW_CPU;
    let cause = m.reboot();
    assert_eq!(cause, esp_periph::RST_SW_CPU);
    let p = &m.bus.periph;
    assert_eq!(p.efuse.ram.read(0x44), 0xdead_beef); assert_eq!(p.gpio.strap, 0x7);
    assert_eq!(p.rtc.ram.read(0x120), 0x1234); assert_eq!(p.rtc.slow_ticks, 999);
    assert_eq!(p.rtc.ram.read(0x38), cause | (cause << 6));
    assert_eq!(p.i2s0.pcm, vec![1, 2, 3]);
    assert!(p.uart[0].tx_out.is_empty() && p.systimer.conf == 0, "digital peripherals are fresh");
    assert_eq!(m.cores[0].pc(), RESET); assert_eq!(m.reboots, 1);
    assert!(!m.dump_regs().contains("core1:"), "core 1 is back in reset");
}

/// Host touch keeps its intended board-edge timestamp and is applied at the fast scheduler's next
/// existing bus tick, which is bounded by one instruction quantum.
#[test]
fn host_touch_is_delivered_on_the_next_fast_path_bus_tick() {
    let mut m = machine();
    park(&mut m, 0, IRAM, &SPIN);
    m.bus.board = Box::new(esp32s3::board::WaveshareAmoled18V2::new());
    m.bus.attach_board_devices();
    m.bus.periph.gpio.pin[esp32s3::board::PIN_AMOLED_TOUCH_INT as usize] = (2 << 7) | (1 << 13);
    m.max_cycles = 64;
    assert!(matches!(m.run(u64::MAX), Stop::Halted));
    let horizon = m.bus.cycles;
    esp_soc::SocBus::observe_gpio(&mut m.bus, true);
    esp_soc::SocBus::touch_input(&mut m.bus, 100, 200, true);
    assert!(esp_soc::SocBus::take_gpio_events(&mut m.bus).is_empty());

    m.max_cycles = horizon + 64;
    assert!(matches!(m.run(u64::MAX), Stop::Halted));

    assert!((horizon + 1..=horizon + 64).contains(&m.bus.cycles));
    assert_eq!(esp_soc::SocBus::take_gpio_events(&mut m.bus),
               [(horizon + 1, esp32s3::board::PIN_AMOLED_TOUCH_INT, false)]);
}

/// Scripts resolve the board's pin names and expand an encoder detent into its quadrature.
#[test]
fn scripts_use_the_board() {
    let mut m = machine();
    m.load_script("1.0 press btn1 50\n2.0 knob cw 2\n3.0 serial hello\n# comment\n4.0 stop\n").unwrap();
    let ev = &m.script.events;
    assert_eq!(ev.len(), 2 + 8 + 1 + 1);
    assert!(ev[0].0 == 240_000_000 && matches!(ev[0].1, ScriptAction::Gpio(17, false)), "{:?}", ev[0]);
    assert_eq!(ev[1].0, 240_000_000 + 12_000_000, "released 50 ms later");
    assert!(matches!(ev[2].1, ScriptAction::Gpio(5, false)), "a CW detent starts with CLK falling");
    assert!(matches!(ev.last().unwrap().1, ScriptAction::Stop));
    assert!(m.load_script("1.0 press nosuchpin").is_err());
    assert!(m.load_script("1.0 frobnicate").is_err());
    m.bus.board = Box::new(NoBoard);
    assert!(m.load_script("1.0 press btn1").is_err(), "no such name on a bare module");
    assert!(m.load_script("1.0 knob cw").is_err(), "no encoder on a bare module");
    assert!(make_board("waveshare-lcd4b").is_some() && make_board("nope").is_none());
}

#[test]
fn browser_touch_reaches_the_amoled_controller_and_gpio() {
    let mut m = machine();
    let board = esp32s3::board::WaveshareAmoled18V2::new();
    let touch = board.touch_state.clone();
    m.bus.board = Box::new(board);
    let web = esp_soc::web::WebServer::queued();
    web.push_incoming(r#"{"t":"touch","x":"123","y":"234","down":"1"}"#.to_string());
    m.web = Some(web);
    park(&mut m, 0, IRAM, &SPIN);
    m.max_cycles = esp32s3::periph::CPU_HZ / 50 + 256;

    assert!(matches!(m.run(u64::MAX), Stop::Halted));
    let state = *touch.lock().expect("AMOLED touch state mutex poisoned");
    assert!(state.down);
    assert_eq!((state.x, state.y), (123, 234));

    m.max_cycles = m.bus.cycles + 256;
    assert!(matches!(m.run(u64::MAX), Stop::Halted));
    assert!(!m.bus.periph.gpio.level(esp32s3::board::PIN_AMOLED_TOUCH_INT));
}

/// Console bytes from every stream go to the backlogs and the aggregate; the mask only chooses
/// what stdout gets, and capture keeps stdout out of it entirely.
#[test]
fn console_capture_and_backlog() {
    let mut m = machine();
    m.console.mask = 2;
    m.bus.periph.usb.tx_out = b"usb\n".to_vec();
    m.bus.periph.uart[0].tx_out = b"uart0\n".to_vec();
    m.bus.periph.uart[2].tx_out = b"uart2\n".to_vec();
    m.drain_console();
    assert_eq!(m.console.all, b"usb\nuart0\nuart2\n");
    assert_eq!(m.console.usb, b"usb\n"); assert_eq!(m.console.uart0, b"uart0\n");
    assert!(m.bus.periph.usb.tx_out.is_empty());
}

/// With an observer that wants every instruction the machine single-steps; the block observer
/// runs on the fast path and sees the same instruction total.
#[test]
fn observers_count_the_same_instructions_either_way() {
    use esp_soc::observers::{BlockProfile, PcHist};
    for slow in [false, true] {
        let mut m = machine();
        park(&mut m, 0, IRAM, &[0x0c, 0x03, 0x1b, 0x33, 0x86, 0xfe, 0xff]);   // movi.n a3,0 ; addi.n a3,a3,1 ; j back to the addi (objdump: 030c 331b fffe86)
        if slow { m.add_observer(Box::new(PcHist::new(4))); } else { m.add_observer(Box::new(BlockProfile::new(4))); }
        m.max_cycles = 64 * 50;
        m.run(u64::MAX);
        let r = m.reports();
        assert!(r.contains(&format!("of {} instructions", m.insns())), "{}", r);
    }
}
