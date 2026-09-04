//! The Cooja-NG lock-step peer (`--cooja`) against a guest small enough to reason about: a
//! 32-instruction RV32 program that listens on the 802.15.4 MAC and echoes every frame it
//! receives (`ECHO`, source below). A hand-written NDJSON session — hello, a few steps, frames
//! injected mid-slice — must produce the same `done` lines every time, with the transmissions
//! stamped at the cycle the guest wrote `TX_START`, one PSDU air time plus a handful of
//! instructions after the frame went in. No ROM, no firmware, no toolchain: the program's bytes
//! are checked in.
#[path = "../../tests/common.rs"]
mod common;
use common::expect_text;
use esp32sim::cooja::{self, Config, Hello};

/// `echo.S`, assembled with `riscv32-esp-elf-as -march=rv32imac_zicsr` (`.option norvc`) and
/// linked at 0x40810000:
/// ```text
/// _start: lui a0,0x600a3            # IEEE802154
///         lui a1,0x60010            # INTMTX
///         li t0,1; sw t0,0x30(a1)   # source 12 (ZB_MAC) -> CPU line 1
///         lui a2,0x20001            # PLIC
///         li t0,2; sw t0,0(a2)      # enable line 1
///         li t0,1; sw t0,0x14(a2)   # priority[1] = 1
///         sw zero,0x90(a2)          # threshold 0
///         lui a3,0x40818            # frame buffer: [len][frame][rssi][lqi]
///         li t0,3; sw t0,0x60(a0)   # EVENT_EN = TX_DONE | RX_DONE
///         sw a3,0xe0(a0)            # DMA_RX_ADDR
///         li t0,0x42; sw t0,0(a0)   # RX_START
/// loop:   wfi
///         lw t1,0x64(a0)            # EVENT_STATUS
///         andi t2,t1,2; beqz t2,no_rx
///         li t0,2; sw t0,0x64(a0)   # clear RX_DONE
///         sw a3,0xd0(a0)            # DMA_TX_ADDR = the same buffer: echo it
///         li t0,0x41; sw t0,0(a0)   # TX_START
/// no_rx:  andi t2,t1,1; beqz t2,loop
///         li t0,1; sw t0,0x64(a0)   # clear TX_DONE
///         li t0,0x42; sw t0,0(a0)   # RX_START
///         j loop
/// ```
/// No trap handler: the interrupt line only ends the `wfi` (mstatus.MIE stays clear), and the
/// events are polled.
const ECHO: [u8; 128] = [
    0x37, 0x35, 0x0a, 0x60, 0xb7, 0x05, 0x01, 0x60, 0x93, 0x02, 0x10, 0x00, 0x23, 0xa8, 0x55, 0x02, 0x37, 0x16, 0x00, 0x20,
    0x93, 0x02, 0x20, 0x00, 0x23, 0x20, 0x56, 0x00, 0x93, 0x02, 0x10, 0x00, 0x23, 0x2a, 0x56, 0x00, 0x23, 0x28, 0x06, 0x08,
    0xb7, 0x86, 0x81, 0x40, 0x93, 0x02, 0x30, 0x00, 0x23, 0x20, 0x55, 0x06, 0x23, 0x20, 0xd5, 0x0e, 0x93, 0x02, 0x20, 0x04,
    0x23, 0x20, 0x55, 0x00, 0x73, 0x00, 0x50, 0x10, 0x03, 0x23, 0x45, 0x06, 0x93, 0x73, 0x23, 0x00, 0x63, 0x8c, 0x03, 0x00,
    0x93, 0x02, 0x20, 0x00, 0x23, 0x22, 0x55, 0x06, 0x23, 0x28, 0xd5, 0x0c, 0x93, 0x02, 0x10, 0x04, 0x23, 0x20, 0x55, 0x00,
    0x93, 0x73, 0x13, 0x00, 0xe3, 0x8c, 0x03, 0xfc, 0x93, 0x02, 0x10, 0x00, 0x23, 0x22, 0x55, 0x06, 0x93, 0x02, 0x20, 0x04,
    0x23, 0x20, 0x55, 0x00, 0x6f, 0xf0, 0x5f, 0xfc,
];
const ENTRY: u32 = 0x4081_0000;

/// A nullnet broadcast as Contiki-NG's framer puts it on the air (no FCS): data frame, PAN
/// compression, short broadcast destination, extended source, 4-byte payload.
const FRAME: &str = "41d801cdabffff010101000174120007000000";
const FRAME2: &str = "41d802cdabffff010101000174120008000000";

/// hello, then steps at fixed times regardless of `wake` (a recorded session, not a csim): a
/// frame two-thirds into a slice, a re-step, a slice with two frames 100 ns apart (the second
/// lands on the first and is dropped), and a stop.
const SESSION: &str = r#"{"type":"hello","proto":1,"id":1,"slot":0,"x":0,"y":0,"seed":1,"args":{},"nodes":[1]}
{"type":"step","t":1000000,"in":[]}
{"type":"step","t":3000000,"in":[{"type":"rx","t":2000000,"from":2,"ch":26,"rssi":-70,"frame":"FRAME"}]}
{"type":"step","t":4000000,"in":[]}
{"type":"step","t":6000000,"in":[{"type":"rx","t":5000000,"from":2,"ch":26,"rssi":-40,"frame":"FRAME"},{"type":"rx","t":5000100,"from":3,"ch":26,"rssi":-40,"frame":"FRAME2"}]}
{"type":"step","t":7000000,"in":[]}
{"type":"stop","t":7000000,"reason":"end of session"}
"#;

fn session() -> String { SESSION.replace("FRAME2", FRAME2).replace("FRAME", FRAME) }

fn run_session() -> (String, cooja::Summary) {
    let mut m = esp32c6::machine(cooja::mac_for_node(1), 4 << 20);
    m.bus.load_bytes(ENTRY, &ECHO).unwrap();
    m.cores[0].pc = ENTRY;
    let hello = Hello { id: 1, ..Default::default() };
    let input = session();
    let mut reader = std::io::Cursor::new(input.into_bytes());
    let mut out = Vec::new();
    // frames reported at their start, so the echo tests the air-time countdown too
    let cfg = Config { rx_on_air: true, slice_ns: 1_000_000, ..Config::default() };
    let s = cooja::run(&mut m, cfg, &hello, &mut reader, &mut out).unwrap();
    (String::from_utf8(out).unwrap(), s)
}

/// (t, frame) of every tx event in the output, in order.
fn tx_events(out: &str) -> Vec<(u64, String)> {
    let mut v = Vec::new();
    for line in out.lines() {
        let mut rest = line;
        while let Some(i) = rest.find("{\"type\":\"tx\"") {
            let ev = &rest[i..];
            let end = ev.find('}').unwrap();
            let ev = &ev[..=end];
            let t: u64 = ev.split("\"t\":").nth(1).unwrap().split(',').next().unwrap().parse().unwrap();
            let frame = ev.split("\"frame\":\"").nth(1).unwrap().split('"').next().unwrap().to_string();
            v.push((t, frame));
            rest = &rest[i + end + 1..];
        }
    }
    v
}

#[test]
fn echo_session_is_exact_and_reproducible() {
    let (out, s) = run_session();
    let (out2, _) = run_session();
    if std::env::var("COOJA_TEST_DUMP").is_ok() { eprintln!("{}", out); }
    assert_eq!(out, out2, "the same session must produce byte-identical replies");
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(lines.len(), 1 + 5, "one done per hello and per step (a yield ends a step early, it does not add a reply):\n{}", out);
    assert!(lines.iter().all(|l| l.starts_with("{\"type\":\"done\",\"t\":")), "{}", out);
    // the guest is busy right after hello, asleep from the first step on
    assert!(lines[0].contains("\"wake\":1000000,"), "busy guest: wake = slice: {}", lines[0]);
    assert!(lines[1].contains("\"wake\":null"), "nothing armed: {}", lines[1]);
    let tx = tx_events(&out);
    assert_eq!(tx.len(), 2, "one echo per delivered frame:\n{}", out);
    assert_eq!((tx[0].1.as_str(), tx[1].1.as_str()), (FRAME, FRAME), "the echoes carry the received frames");
    // RX_DONE at rx.t + (len byte + PSDU + FCS) air time, then a few instructions to TX_START
    let air = (1 + FRAME.len() as u64 / 2 + 2) * 32_000;
    for (i, (t, _)) in tx.iter().enumerate() {
        let rx_t = [2_000_000u64, 5_000_000][i];
        assert!((rx_t + air..rx_t + air + 200).contains(t), "tx {} at {} ns, expected just after {} ns", i, t, rx_t + air);
    }
    // the reply that carried the yield is stamped with it and asks to continue promptly
    let yielded: Vec<&&str> = lines.iter().filter(|l| l.contains("\"type\":\"tx\"")).collect();
    assert_eq!(yielded.len(), 2);
    assert!(yielded[0].starts_with(&format!("{{\"type\":\"done\",\"t\":{},\"wake\":{},", tx[0].0, tx[0].0 + 1_000_000)), "{}", yielded[0]);
    assert_eq!((s.steps, s.yields, s.tx, s.rx, s.rx_dropped), (5, 2, 2, 3, 1));
    expect_text("cooja-echo.ndjson", &out);
}

/// The E6 stage-1 gate: Contiki-NG on ESP-IDF (esp32-contiki, `CONTIKI_C6_DIR` = its
/// `build-nullnet`, the nullnet cross-level probe) as a lock-stepped external mote. From the mask
/// ROM through the bootloader, FreeRTOS and the PHY blob (`bb_init` stubbed, as for the energy
/// scanner) to Contiki's periodic broadcast: every 5 s the guest's frame must come out as a `tx`
/// event stamped at its `TX_START`; a broadcast injected mid-slice must reach the driver (its
/// `RX SFD` and `RX: 19 bytes` lines right after it, csim reporting frames at their end) and Contiki's nullnet callback
/// (`rx len 4 count 7 from …`); two sessions must be byte-identical; and the whole exchange is
/// pinned as a golden.
#[test] #[ignore = "set CONTIKI_C6_DIR=/path/to/esp32-contiki/build-nullnet (built with -DCONTIKI_NULLNET=ON for esp32c6); needs the ESP32-C6 mask ROM ELF"]
fn external_nullnet_c6() {
    let dir = std::env::var("CONTIKI_C6_DIR").expect("CONTIKI_C6_DIR=/path/to/esp32-contiki/build-nullnet is required for this test");
    let rom = common::rom("esp32c6_rev0");
    // hello, a step every 10 ms to 10.3 s, one frame from node 1 at 6.205 s (inside the step to 6.21 s)
    let mut session = String::from("{\"type\":\"hello\",\"proto\":1,\"id\":2,\"slot\":1,\"x\":1.0,\"y\":0.0,\"seed\":7,\"args\":{},\"nodes\":[1,2]}\n");
    let mut t = 0u64;
    while t < 10_300_000_000 {
        t += 10_000_000;
        let input = if t == 6_210_000_000 { format!("{{\"type\":\"rx\",\"t\":6205000000,\"from\":1,\"ch\":26,\"rssi\":-65,\"frame\":\"{}\"}}", FRAME) } else { String::new() };
        session += &format!("{{\"type\":\"step\",\"t\":{},\"in\":[{}]}}\n", t, input);
    }
    session += &format!("{{\"type\":\"stop\",\"t\":{},\"reason\":\"end\"}}\n", t);
    let run = || {
        let mut child = std::process::Command::new(env!("CARGO_BIN_EXE_esp32sim-c6"))
            .args(["--cooja", "--boot", "rom", "--flash-mb", "2", "--no-dump", "--rom", rom.to_str().unwrap(), "--stub", "bb_init=0",
                   "--bootloader", &format!("{dir}/bootloader/bootloader.bin"), "--ptable", &format!("{dir}/partition_table/partition-table.bin"),
                   "--app", &format!("{dir}/esp32-blink.bin"), "--elf", &format!("{dir}/esp32-blink.elf")])
            .current_dir(common::root()).stdin(std::process::Stdio::piped()).stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::piped()).spawn().expect("esp32sim-c6");
        { use std::io::Write; child.stdin.take().unwrap().write_all(session.as_bytes()).unwrap(); }
        let out = child.wait_with_output().unwrap();
        assert!(out.status.success(), "esp32sim-c6 --cooja failed: {}", String::from_utf8_lossy(&out.stderr));
        (String::from_utf8(out.stdout).unwrap(), String::from_utf8_lossy(&out.stderr).to_string())
    };
    let (out, err) = run();
    let (out2, _) = run();
    assert_eq!(out, out2, "two sessions from the same input must be byte-identical");
    let tx = tx_events(&out);
    assert_eq!(tx.len(), 2, "one broadcast at ~5.07 s and one at ~10.07 s:\n{}", err);
    for (i, (t, frame)) in tx.iter().enumerate() {
        let expect_t = 5_065_000_000 + i as u64 * 5_000_000_000;
        assert!((expect_t..expect_t + 2_000_000).contains(t), "tx {} at {} ns, expected in [{}, +2 ms)", i, t, expect_t);
        // data frame, PAN compression, short broadcast destination, extended source from the node-2 MAC, 4-byte counter
        assert!(frame.starts_with("41d8") && frame[6..].starts_with("cdabffff020000feff000002") && frame.ends_with(&format!("{:02x}000000", i)), "tx {}: {}", i, frame);
    }
    let line_at = |needle: &str| -> u64 {
        let l = out.lines().flat_map(|l| l.split("{\"type\":\"log\",")).find(|l| l.contains(needle)).unwrap_or_else(|| panic!("no log line containing {:?}:\n{}", needle, err));
        l.split("\"t\":").nth(1).unwrap().split(',').next().unwrap().parse().unwrap()
    };
    // csim reports a frame at its end: the driver sees SFD and RX_DONE at rx.t, and prints within its ISR
    assert!(line_at("RX SFD received") >= 6_205_000_000);
    let rx_done = line_at("RX: 19 bytes, RSSI -65 dBm");
    assert!((6_205_000_000..6_205_300_000).contains(&rx_done), "the driver's RX_DONE line at {} should follow rx.t at once", rx_done);
    // Contiki's callback prints through newlib's stdout buffer while the port's putchar bypasses
    // it, so the line waits for the next printf with a newline in it: the 10 s broadcast
    assert!(line_at("rx len 4 count 7 from 0012740100010101") > rx_done);
    assert!(out.contains("\"state\":\"tx\"") && out.contains("\"state\":\"rx\""), "radio state events");
    expect_text("cooja-nullnet-c6.ndjson", &out);
}

/// Time passes exactly: a step to `t` leaves the guest at the first cycle at or after `t`, and
/// events are never stamped before the step that produced them.
#[test]
fn stamps_never_precede_their_step() {
    let (out, _) = run_session();
    let mut prev = 0u64;
    for line in out.lines() {
        let t: u64 = line.split("\"t\":").nth(1).unwrap().split(',').next().unwrap().parse().unwrap();
        assert!(t >= prev, "done times go backwards: {} after {}", t, prev);
        for ev_t in line.split("\"t\":").skip(2).map(|s| s.split(|c: char| !c.is_ascii_digit()).next().unwrap().parse::<u64>().unwrap()) {
            assert!(ev_t >= prev && ev_t <= t, "event at {} outside its slice ({}, {}]", ev_t, prev, t);
        }
        prev = t;
    }
}
