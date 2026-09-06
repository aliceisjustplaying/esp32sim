//! Generate an independently interpreted two-core fixture and experimental WASM modules.
use emu_core::{Bus, Core};
use esp_soc::board::BoardEdge;
use esp_soc::{BoardModel, Ctx, Machine, Observer, Soc, SocBus, Stop, Wants};
use esp32sim_wasm_jit::scheduled::{self, Mutations, PCS, SRAM_BASE, SRAM_LEN};
use std::{cell::RefCell, rc::Rc};
use xtensa_lx7::Cpu;

// Keep both real LX7 cores running without an unpriced core-release MMIO write.
struct Pair;
impl Soc for Pair {
    type Core = Cpu;
    type Bus = esp32s3::bus::SocBus;
    const NAME: &'static str = "esp32s3";
    const ROM_ELF: &'static str = "esp32s3_rev0_rom.elf";
    const CPU_HZ: u64 = 240_000_000;
    const CORES: usize = 2;
    const IDLE_CHUNK: u64 = 512;
    const ROM_DATA_TABLE: &'static [&'static str] = &[];
    fn new_core(i: usize) -> Cpu { <esp32s3::S3 as Soc>::new_core(i) }
    fn reset_core(c: &mut Cpu, i: usize) { <esp32s3::S3 as Soc>::reset_core(c, i); c.set_pc(PCS[i]); c.set_ar(2, SRAM_BASE); c.set_ar(3, 7 + i as u32); c.set_ar(4, 0xdead_beef + i as u32); }
    fn boot_core(c: &mut Cpu, pc: u32) { <esp32s3::S3 as Soc>::boot_core(c, pc); }
    fn irqs(bus: &Self::Bus, out: &mut [u32]) { <esp32s3::S3 as Soc>::irqs(bus, out); }
}

struct Edge { at: u64, fired: bool, edges: Vec<BoardEdge> }
impl BoardModel for Edge {
    fn name(&self) -> &'static str { "timed-pair-test-edge" }
    fn input_levels(&self) -> Vec<(u8, bool)> { vec![(0, self.fired)] }
    fn next_deadline(&self) -> Option<u64> { (!self.fired).then_some(self.at) }
    fn advance_to(&mut self, now: u64) {
        if !self.fired && now >= self.at {
            self.fired = true;
            self.edges.push(BoardEdge { cycle: self.at, pin: 0, level: true });
        }
    }
    fn take_edges(&mut self) -> Vec<BoardEdge> { std::mem::take(&mut self.edges) }
}
#[derive(Default)]
struct Observed { start: (u64, usize, u32), rows: Vec<Vec<u64>>, edges: Vec<u64> }
struct Trace(Rc<RefCell<Observed>>);
impl Observer<Pair> for Trace {
    fn name(&self) -> &'static str { "timed-pair" }
    fn wants(&self) -> Wants { Wants::INSN | Wants::GPIO }
    fn on_insn(&mut self, cx: &Ctx, core: usize, _: &Cpu, _: &mut <Pair as Soc>::Bus, pc: u32) -> Option<Stop> {
        self.0.borrow_mut().start = (cx.cycles, core, pc); None
    }
    fn after_insn(&mut self, _: &Ctx, _: usize, cpu: &Cpu, bus: &mut <Pair as Soc>::Bus) -> Option<Stop> {
        let mut observed = self.0.borrow_mut();
        let (time, core, pc) = observed.start;
        let mut row = vec![time, core as u64, u64::from(pc), 0, bus.gpio_input() & 1];
        row.extend((0..16).map(|reg| u64::from(cpu.get_ar(reg))));
        row.push(u64::from(cpu.pc()));
        observed.rows.push(row); None
    }
    fn on_gpio(&mut self, cycle: u64, pin: u8, level: bool) {
        if pin == 0 && level { self.0.borrow_mut().edges.push(cycle); }
    }
}

fn reference(deadline: u64, data: u32, chunks: &[u64], refusal: bool, plans: &[Vec<esp32s3::timing::LedgerEntry>; 2]) -> String {
    let mut bus = esp32s3::bus::SocBus::new(0x1000, 0x1000, [0; 6]);
    bus.board = Box::new(Edge { at: deadline, fired: false, edges: vec![] });
    bus.attach_board_devices();
    let mut machine = Machine::<Pair>::new([0; 6], bus);
    let code = scheduled::programs(refusal);
    for core in 0..2 {
        machine.bus.load_bytes(PCS[core], &code[core]).unwrap();
        machine.cores[core].set_pc(PCS[core]);
        machine.cores[core].set_ar(2, SRAM_BASE);
        machine.cores[core].set_ar(3, 7 + core as u32);
        machine.cores[core].set_ar(4, 0xdead_beef + core as u32);
    }
    let mut sram = vec![0; SRAM_LEN];
    sram[..4].copy_from_slice(&data.to_le_bytes());
    sram[4..8].copy_from_slice(&data.wrapping_add(11).to_le_bytes());
    machine.bus.load_bytes(SRAM_BASE, &sram).unwrap();
    let model = esp32s3::Esp32S3SramCostModel::new();
    let trace = Rc::new(RefCell::new(Observed::default()));
    machine.add_observer(Box::new(Trace(trace.clone())));
    machine.set_cost_model(Box::new(model.clone())).unwrap();
    let mut stopped = 0;
    for &chunk in chunks {
        match machine.run(chunk) {
            Stop::MaxInsns => {},
            Stop::CostModel { core, pc, reason } => { assert!(refusal && reason.contains("S32i"), "unexpected refusal core{core} pc{pc:#x}: {reason}"); stopped = core + 1; },
            other => panic!("unexpected stop {other:?}"),
        }
    }
    let ledger = model.ledger();
    let mut trace = trace.borrow_mut();
    for (row, entry) in trace.rows.iter_mut().zip(&ledger) {
        assert_eq!((row[1] as usize, row[2] as u32), (entry.core, entry.pc));
        assert_eq!(entry, plans[entry.core].iter().find(|p| p.pc == entry.pc).unwrap(), "runtime pricing differs from compile plan");
        row[3] = u64::from(entry.cycles);
    }
    assert_eq!(trace.rows.len(), ledger.len() + usize::from(refusal));
    let registers: Vec<Vec<u32>> = machine.cores.iter().map(|c| (0..16).map(|i| c.get_ar(i)).collect()).collect();
    let pcs: Vec<u32> = machine.cores.iter().map(Core::pc).collect();
    let ccounts: Vec<u32> = machine.cores.iter().map(|c| c.ccount).collect();
    let insns: Vec<u64> = machine.cores.iter().map(Core::insn_count).collect();
    let ready: Vec<u64> = (0..2).map(|core| ledger.iter().filter(|e| e.core == core).map(|e| u64::from(e.cycles)).sum()).collect();
    let memory: Vec<u8> = (0..SRAM_LEN).map(|i| machine.bus.read8(SRAM_BASE + i as u32).unwrap()).collect();
    format!("{{\"registers\":{registers:?},\"pcs\":{pcs:?},\"ccounts\":{ccounts:?},\"insns\":{insns:?},\"ready\":{ready:?},\"sram\":{memory:?},\"now\":{},\"flag\":{},\"events\":{},\"stopped\":{stopped},\"trace\":{:?},\"edges\":{:?}}}", machine.bus.cycles(), machine.bus.gpio_input() & 1, ledger.len(), trace.rows, trace.edges)
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let out = std::env::args().nth(1).unwrap_or_else(|| "work/timed-pair".into());
    let out = std::path::Path::new(&out); std::fs::create_dir_all(out)?;
    let normal = scheduled::compile(false, Mutations::default())?;
    let refused = scheduled::compile(true, Mutations::default())?;
    for (name, refusal, mutation) in [
        ("timed", false, Mutations::default()),
        ("reverse-ties", false, Mutations { reverse_ties: true, ..Default::default() }),
        ("late-deadline", false, Mutations { late_deadline: true, ..Default::default() }),
        ("omit-load-use", false, Mutations { omit_load_use: true, ..Default::default() }),
        ("refusal", true, Mutations::default()),
        ("refusal-reverse-ties", true, Mutations { reverse_ties: true, ..Default::default() }),
    ] { std::fs::write(out.join(format!("{name}.wasm")), scheduled::compile(refusal, mutation)?.bytes)?; }
    let mut cases = vec![];
    for deadline in [2, 5, 8, 19] {
        for data in [0, 41, u32::MAX] {
            let chunks = vec![20];
            let expected = reference(deadline, data, &chunks, false, &normal.plans);
            for split in [vec![1; 20], vec![3, 1, 7, 2, 7]] { assert_eq!(reference(deadline, data, &split, false, &normal.plans), expected); }
            cases.push(format!("{{\"deadline\":{deadline},\"data\":{data},\"refusal\":false,\"expected\":{expected}}}"));
        }
    }
    let expected = reference(5, 41, &[20, 1], true, &refused.plans);
    cases.push(format!("{{\"deadline\":5,\"data\":41,\"refusal\":true,\"expected\":{expected}}}"));
    // A longer independent oracle for benchmark validation, no small-case-only assumptions.
    let expected = reference(5, 41, &[1000], false, &normal.plans);
    cases.push(format!("{{\"deadline\":5,\"data\":41,\"refusal\":false,\"count\":1000,\"expected\":{expected}}}"));
    std::fs::write(out.join("cases.json"), format!("[{}]", cases.join(",\n")))?;
    println!("generated {} reference cases and six modules in {}", cases.len(), out.display());
    Ok(())
}
