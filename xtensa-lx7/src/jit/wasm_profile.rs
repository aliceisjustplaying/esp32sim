//! Opt-in statistical block profiling. No instrumentation is present in production builds.
//! Uniform pseudorandom samples avoid aliasing with periodic guest loops. Timings include
//! block lookup/decoding and dispatch, but not the outer SoC scheduler. Browser clock
//! quantization and timer-call overhead make these estimates, not a CPU flame graph.
use super::{emitter, BlockInsn};
use std::collections::HashMap;
use std::fmt::Write;

#[link(wasm_import_module = "env")]
extern "C" {
    fn host_profile_now() -> f64;
}

pub fn now() -> f64 {
    // SAFETY: the profiling host provides a monotonic clock in milliseconds.
    unsafe { host_profile_now() }
}

#[derive(Default)]
struct Row {
    samples: u64,
    instructions: u64,
    ms: f64,
    ops: String,
    missing: String,
}

pub const PATHS: [&str; 12] = ["irq", "waiting", "find_trap", "interp_nocode", "interp_cold", "region_hot", "region_slow",
    "body_noregion", "body_gatefail", "body_after_reject", "body_resumed", "body_observed"];
pub fn missing(ops: &[BlockInsn], fast: bool) -> String {
    let mut v: Vec<String> = ops.iter().filter(|i| !emitter::supported_insn(&i.insn, fast)).map(|i| if i.insn.op == crate::Op::Pie { format!("Pie:{}", crate::pie_table::OPS[i.insn.imm as usize].name) } else if matches!(i.insn.op, crate::Op::Rsr | crate::Op::Wsr | crate::Op::Xsr | crate::Op::Rur | crate::Op::Wur) { format!("{:?}:{}", i.insn.op, i.insn.imm) } else { format!("{:?}", i.insn.op) }).collect();
    v.sort(); v.dedup(); v.join("+")
}
#[derive(Default)]
pub struct Census {
    pub nocode_blocks: HashMap<u32, (String, u64, u64)>,
    pub path: usize,
    pub budget: Vec<u64>,
    pub done: Vec<u64>,
    pub paths: [(u64, u64); 12],
    pub gate: [u64; 4],
    pub body_exits: [[u64; 8]; 2],
    pub interp_ops: HashMap<String, u64>,
    pub helper_ops: HashMap<String, u64>,
    pub pie_table: HashMap<&'static str, u64>,
    pub pie_packed: HashMap<&'static str, u64>,
    pub pcs: HashMap<u32, (u64, u64)>,
}
impl Census {
    pub fn dispatch(&mut self, pc: u32, budget: u32, done: u32) {
        if self.budget.is_empty() { self.budget = vec![0; 67]; self.done = vec![0; 67]; }
        self.budget[budget.min(66) as usize] += 1;
        self.done[done.min(66) as usize] += 1;
        let p = &mut self.paths[self.path]; p.0 += 1; p.1 += done as u64;
        let e = self.pcs.entry(pc).or_default(); e.0 += 1; e.1 += done as u64;
    }
    pub fn report(&self) -> String {
        let mut t = String::new();
        let total: u64 = self.paths.iter().map(|p| p.0).sum();
        let insns: u64 = self.paths.iter().map(|p| p.1).sum();
        writeln!(t, "[census] dispatches={total} iterations={insns}").unwrap();
        for (n, p) in PATHS.iter().zip(&self.paths) { writeln!(t, "[census-path] {n}\t{}\t{}", p.0, p.1).unwrap(); }
        writeln!(t, "[census-budget] {:?}", self.budget).unwrap();
        writeln!(t, "[census-done] {:?}", self.done).unwrap();
        writeln!(t, "[census-gate] budget_lt_len={} bloom={} loop={} stale_pages={}", self.gate[0], self.gate[1], self.gate[2], self.gate[3]).unwrap();
        writeln!(t, "[census-body-exits] entry0[end,left,trap,cut,pre,reject]={:?} resumed={:?}", &self.body_exits[0][..6], &self.body_exits[1][..6]).unwrap();
        for (name, m) in [("interp", &self.interp_ops), ("helper", &self.helper_ops)] {
            let mut v: Vec<_> = m.iter().collect(); v.sort_by(|a, b| b.1.cmp(a.1));
            for (k, n) in v.iter().take(40) { writeln!(t, "[census-{name}-op] {k}\t{n}").unwrap(); }
        }
        for (name, m) in [("pie_table", &self.pie_table), ("pie_packed", &self.pie_packed)] {
            let mut v: Vec<_> = m.iter().collect(); v.sort_by(|a, b| b.1.cmp(a.1));
            for (k, n) in v.iter().take(40) { writeln!(t, "[census-{name}] {k}\t{n}").unwrap(); }
        }
        let mut by: HashMap<&str, (u64, u64)> = HashMap::new();
        for (m, d, i) in self.nocode_blocks.values() { let e = by.entry(m.as_str()).or_default(); e.0 += d; e.1 += i; }
        let mut v: Vec<_> = by.iter().collect(); v.sort_by(|a, b| b.1.0.cmp(&a.1.0));
        for (k, (d, i)) in v.iter().take(40) { writeln!(t, "[census-nocode-missing] {k}\t{d}\t{i}").unwrap(); }
        let mut v: Vec<_> = self.pcs.iter().collect(); v.sort_by(|a, b| b.1.1.cmp(&a.1.1));
        writeln!(t, "[census-pcs] distinct={}", v.len()).unwrap();
        for (pc, (d, i)) in v.iter().take(3000) { writeln!(t, "[census-pc] {pc:08x}\t{d}\t{i}").unwrap(); }
        t
    }
}

pub struct Profile {
    pub census: Census,
    rng: u32,
    calls: u64,
    rows: HashMap<(u32, bool), Row>,
    loops: HashMap<u32, (u64, u64)>,
}

impl Default for Profile {
    fn default() -> Self {
        Self { census: Census::default(), rng: 0x914f_7ab3, calls: 0, rows: HashMap::new(), loops: HashMap::new() }
    }
}

impl Profile {
    pub fn sample(&mut self) -> bool {
        self.calls += 1;
        self.rng ^= self.rng << 13;
        self.rng ^= self.rng >> 17;
        self.rng ^= self.rng << 5;
        self.rng & 4095 == 0
    }

    #[allow(clippy::too_many_arguments)]
    pub fn record(&mut self, pc: u32, jit: bool, done: u32, ms: f64, ops: &[BlockInsn], fast: bool) {
        let row = self.rows.entry((pc, jit)).or_insert_with(|| Row {
            ops: ops.iter().map(|i| format!("{:?}", i.insn.op)).collect::<Vec<_>>().join(","),
            missing: ops.iter().filter(|i| !emitter::supported_insn(&i.insn, fast))
                .map(|i| format!("{:?}", i.insn.op)).collect::<Vec<_>>().join(","),
            ..Row::default()
        });
        row.samples += 1;
        row.instructions += done as u64;
        row.ms += ms;
    }

    /// Count completed retained backedges once per dispatch, never in generated loops.
    pub fn record_loop(&mut self, pc: u32, retained: u32) {
        if retained == 0 { return; }
        let row = self.loops.entry(pc).or_default();
        row.0 += 1;
        row.1 += retained as u64;
    }

    pub fn report(&self) -> String {
        let mut text = format!("[wasm-profile] calls={} sample_probability=1/4096\n", self.calls);
        let mut loops: Vec<_> = self.loops.iter().collect();
        loops.sort_by_key(|(pc, _)| **pc);
        for (pc, (calls, backedges)) in loops {
            writeln!(text, "[wasm-loop] pc={pc:08x} calls={calls} retained_backedges={backedges}").unwrap();
        }
        for jit in [false, true] {
            let rows: Vec<_> = self.rows.iter().filter(|((_, j), _)| *j == jit).collect();
            let samples: u64 = rows.iter().map(|(_, r)| r.samples).sum();
            let instructions: u64 = rows.iter().map(|(_, r)| r.instructions).sum();
            let ms: f64 = rows.iter().map(|(_, r)| r.ms).sum();
            writeln!(text, "jit={jit} samples={samples} sampled_instructions={instructions} sampled_ms={ms:.6}").unwrap();
        }
        let mut rows: Vec<_> = self.rows.iter().collect();
        rows.sort_by(|a, b| b.1.instructions.cmp(&a.1.instructions).then(a.0.cmp(b.0)));
        writeln!(text, "pc\tjit\tsamples\tinstructions\tms\tmissing\tops").unwrap();
        for ((pc, jit), r) in rows {
            writeln!(text, "{pc:08x}\t{jit}\t{}\t{}\t{:.6}\t{}\t{}", r.samples, r.instructions, r.ms, r.missing, r.ops).unwrap();
        }
        text
    }
}
