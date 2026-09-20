use emu_core::{Bus, Core};
use esp_soc::Stop;

const IRAM: u32 = 0x4037_0000;

#[test]
fn architectural_stop_preserves_unfinished_round() {
    // Native virtual quanta require ESP32SIM_VQ_NATIVE=1 and the interpreter.
    // Cover both round boundaries and partial rounds, with either core busy.
    for busy in 0..2 {
        for instructions in [1, 63, 64, 65, 127, 128, 129] {
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
                m.vq_max = vq;
                m.max_cycles = 64 + (instructions as u64).div_ceil(64).max(2) * 64;
                let stop = m.run(1024);
                assert!(matches!(stop, Stop::Exceptions(1)), "busy={busy} instructions={instructions} vq={vq}: {stop:?}");
                if vq > 1 && std::env::var_os("ESP32SIM_VQ_NATIVE").is_some() {
                    assert!(m.vq_stats[0] > 0);
                }
                assert_eq!(m.bus.vq_violations, 0, "no undeferred device accesses");
                results.push((m.bus.cycles, m.cores.iter().map(|c| (c.ccount, c.insn_count, c.pc, c.ps)).collect::<Vec<_>>()));
            }
            assert_eq!(results[0], results[1], "busy={busy} instructions={instructions}");
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
