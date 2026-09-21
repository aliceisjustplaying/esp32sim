use emu_core::{Bus, Core};
use esp_soc::Stop;

const IRAM: u32 = 0x4037_0000;

#[test]
fn peripheral_alarm_inside_a_batch_matches_single_round_scheduling() {
    const VECTOR: u32 = IRAM + 0x1000;
    // 16 MHz SYSTIMER deadlines straddle the 64-cycle scheduling boundary.
    for ticks in [9u32, 12, 13, 17] {
        for busy in 0..2 {
            let mut results = Vec::new();
            for vq in [1, 1024] {
                let mut m = esp32s3::machine([0; 6]);
                m.console.capture = true;
                m.vq_max = 1;
                for core in &mut m.cores { core.set_jit(false); }
                let spin = [0x06, 0xff, 0xff];
                m.bus.load_bytes(IRAM, &spin).unwrap();
                m.bus.load_bytes(0x4000_0400, &spin).unwrap();
                m.cores[0].pc = IRAM;
                m.cores[0].ps = 0;
                m.bus.write32(0x600c_0000, 2).unwrap();
                assert!(matches!(m.run(64), Stop::MaxInsns));
                m.cores[1 - busy].waiting = true;
                m.cores[busy].pc = IRAM;
                m.cores[busy].ps = 0;
                m.cores[busy].vecbase = VECTOR;
                m.cores[busy].intenable = 1 << 1; // level-one external interrupt
                // Capture interrupt delivery time once, then spin in the handler.
                m.bus.load_bytes(VECTOR + xtensa_lx7::state::vec::KERNEL,
                    &[0x20, 0xea, 0x03, 0x06, 0xff, 0xff]).unwrap(); // rsr a2,ccount; j .
                let source = esp32s3::periph::SRC_SYSTIMER_T0 as u32;
                m.bus.write32(0x600c_2000 + busy as u32 * 0x800 + source * 4, 1).unwrap();
                m.bus.write32(0x6002_3000, (1 << 30) | (1 << 24)).unwrap();
                m.bus.write32(0x6002_3020, ticks).unwrap();
                m.bus.write32(0x6002_3064, 1).unwrap();
                m.bus.write32(0x6002_3050, 1).unwrap();
                m.vq_max = vq;
                m.max_cycles = m.bus.cycles + 512;
                assert!(matches!(m.run(2048), Stop::Halted));
                assert_eq!(m.interrupts, 1, "alarm must reach the running core");
                assert_eq!(m.cores[busy].epc[1], IRAM);
                assert!((64..576).contains(&m.cores[busy].get_ar(2)), "handler must capture its arrival time");
                assert_eq!(m.bus.periph.systimer.int_raw & 1, 1);
                assert_eq!(m.bus.vq_violations, 0);
                if vq > 1 && std::env::var_os("ESP32SIM_VQ_NATIVE").is_some() {
                    assert!(m.vq_stats[0] > 0 && m.vq_stats[1] >= 2, "exercise batched rounds");
                }
                results.push((m.bus.cycles, m.run_steps(), m.irq_hist.clone(), m.cores.iter()
                    .map(|c| (c.ccount, c.insn_count, c.pc, c.ps, c.interrupt, c.epc, c.get_ar(2))).collect::<Vec<_>>()));
            }
            assert_eq!(results[0], results[1], "busy={busy} ticks={ticks}");
        }
    }
}

#[test]
fn architectural_stop_preserves_unfinished_round() {
    // Native virtual quanta require ESP32SIM_VQ_NATIVE=1 and the interpreter.
    // Cover both round boundaries and partial rounds, with either core busy.
    for (quantum, busy) in [32usize, 64, 128].into_iter().flat_map(|q| (0..2).map(move |b| (q, b))) {
        for instructions in [1, quantum - 1, quantum, quantum + 1, 2 * quantum - 1, 2 * quantum, 2 * quantum + 1] {
            let mut results = Vec::new();
            for vq in [1, 1024] {
                let mut m = esp32s3::machine([0; 6]);
                m.console.capture = true;
                m.vq_max = 1;
                for c in &mut m.cores { c.set_jit(false); }
                m.bus.load_bytes(IRAM, &[0x06, 0xff, 0xff]).unwrap();
                m.bus.load_bytes(0x4000_0400, &[0x06, 0xff, 0xff]).unwrap();
                m.cores[0].pc = IRAM;
                m.cores[0].ps = 0;
                if busy == 1 { m.bus.write32(0x600c_0000, 2).unwrap(); }
                assert!(matches!(m.run(64), Stop::MaxInsns));
                if busy == 1 { m.cores[0].waiting = true; }
                let mut code = [0x3d, 0xf0].repeat(instructions - 1); // nop.n
                code.extend([0, 0, 0]); // ill
                m.bus.load_bytes(IRAM + 0x100, &code).unwrap();
                m.cores[busy].pc = IRAM + 0x100;
                m.cores[busy].ps = 0;
                m.dbg.stop_after_exceptions = 1;
                m.quantum = quantum as u64;
                m.vq_max = vq;
                m.max_cycles = 64 + (instructions as u64).div_ceil(quantum as u64).max(2) * quantum as u64;
                let stop = m.run(1024);
                assert!(matches!(stop, Stop::Exceptions(1)), "quantum={quantum} busy={busy} instructions={instructions} vq={vq}: {stop:?}");
                if vq > 1 && std::env::var_os("ESP32SIM_VQ_NATIVE").is_some() {
                    assert!(m.vq_stats[0] > 0);
                }
                assert_eq!(m.bus.vq_violations, 0, "no undeferred device accesses");
                results.push((m.bus.cycles, m.cores.iter().map(|c| (c.ccount, c.insn_count, c.pc, c.ps)).collect::<Vec<_>>()));
            }
            assert_eq!(results[0], results[1], "quantum={quantum} busy={busy} instructions={instructions}");
        }
    }
}

#[test]
fn virtual_runs_stop_before_register_reads_and_script_events() {
    let mut results = Vec::new();
    for vq in [1, 1024] {
        let mut m = esp32s3::machine([0; 6]);
        m.console.capture = true;
        for core in &mut m.cores { core.set_jit(false); }
        let mut code = [0x3d, 0xf0].repeat(150); // nop.n
        code.extend([0x22, 0x23, 0x00]); // l32i a2,a3,0: device access cuts virtual run
        code.extend([0x06, 0xff, 0xff]); // j .
        m.bus.load_bytes(IRAM, &code).unwrap();
        m.cores[0].pc = IRAM;
        m.cores[0].ps = 0;
        m.cores[0].set_ar(3, 0x6002_3000); // SYSTIMER configuration register
        m.vq_max = vq;
        m.script.log = false;
        m.script.events = vec![(192, esp_soc::ScriptAction::Serial("event".into())), (320, esp_soc::ScriptAction::Stop)];
        assert!(matches!(m.run(1024), Stop::Halted));
        assert_eq!(m.bus.vq_violations, 0);
        assert_eq!(m.script.pos, 2);
        assert_eq!(m.bus.periph.usb.rx.iter().copied().collect::<Vec<_>>(), b"event");
        if vq > 1 && std::env::var_os("ESP32SIM_VQ_NATIVE").is_some() {
            assert!(m.vq_stats[0] > 0 && m.vq_stats[2] > 0, "exercise register deferral");
        }
        results.push((m.bus.cycles, m.insns(), m.cores[0].ccount, m.cores[0].pc, m.cores[0].get_ar(2)));
    }
    assert_eq!(results[0], results[1]);
    assert_eq!(results[0].0, 320);
}
