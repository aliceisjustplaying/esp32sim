//! Differential tests executed by tools/wasm-jit-test.mjs in an actual WASM runtime.
//! Constructed instructions exercise the emitter independently of encoding; the scheduler
//! case below uses real encoded instructions and proves that hot dispatch actually happens.
use super::*;
use crate::bus::{tlb_index, TLB_ENTRIES};
use crate::{Fault, FlatRam, Insn, Op, Trap};
const BASE: u32 = 0x4037_0000;
struct Ram {
    ram: FlatRam,
    versions: Vec<u32>,
    tlb: Vec<TlbEntry>,
    fast: bool,
    readonly: bool,
    noted: u32,
}
impl Ram {
    fn new(fast: bool, readonly: bool) -> Self {
        let mut ram = FlatRam::new(BASE, 65536);
        for (i, b) in ram.mem.iter_mut().enumerate() {
            *b = (i as u8).wrapping_mul(37);
        }
        let mut tlb = vec![TlbEntry::EMPTY; TLB_ENTRIES];
        tlb[tlb_index(BASE)] = TlbEntry {
            lo: BASE,
            hi: BASE + 65536,
            base: ram.mem.as_mut_ptr(),
            vbase: 0,
            writable: (!readonly) as u32,
            off: 0,
            src: 0,
        };
        Self {
            ram,
            versions: vec![0; 256],
            tlb,
            fast,
            readonly,
            noted: 0,
        }
    }
    fn wrote(&mut self, a: u32, n: u32) {
        for p in (a - BASE) / 256..=(a - BASE + n - 1) / 256 {
            self.versions[p as usize] += 1;
        }
    }
}
impl Bus for Ram {
    fn read8(&mut self, a: u32) -> Result<u8, Fault> {
        self.ram.read8(a)
    }
    fn read16(&mut self, a: u32) -> Result<u16, Fault> {
        self.ram.read16(a)
    }
    fn read32(&mut self, a: u32) -> Result<u32, Fault> {
        self.ram.read32(a)
    }
    fn write8(&mut self, a: u32, v: u8) -> Result<(), Fault> {
        if self.readonly {
            return Err(Fault::Prohibited);
        }
        self.ram.write8(a, v)?;
        self.wrote(a, 1);
        Ok(())
    }
    fn write16(&mut self, a: u32, v: u16) -> Result<(), Fault> {
        if self.readonly {
            return Err(Fault::Prohibited);
        }
        self.ram.write16(a, v)?;
        self.wrote(a, 2);
        Ok(())
    }
    fn write32(&mut self, a: u32, v: u32) -> Result<(), Fault> {
        if self.readonly {
            return Err(Fault::Prohibited);
        }
        self.ram.write32(a, v)?;
        self.wrote(a, 4);
        Ok(())
    }
    fn fetch(&mut self, a: u32) -> Result<[u8; 4], Fault> {
        self.ram.fetch(a)
    }
    fn page_versions(&self) -> &[u32] {
        &self.versions
    }
    fn code_page(&mut self, a: u32) -> u32 {
        a.wrapping_sub(BASE) / 256
    }
    fn fast_mem(&mut self) -> Option<FastMem> {
        self.fast.then_some(FastMem {
            tlb: self.tlb.as_ptr(),
            page_ver: self.versions.as_mut_ptr(),
        })
    }
    fn note_pc(&mut self, pc: u32) {
        self.noted = pc;
    }
}
fn cpu(seed: u32) -> Cpu {
    let mut c = Cpu::new(0);
    c.pc = BASE;
    c.ps = 0;
    c.vecbase = BASE + 0x8000;
    c.windowbase = seed % 16;
    c.sar = seed % 64;
    let mut x = seed;
    for r in &mut c.ar {
        x = x.wrapping_mul(1664525).wrapping_add(1013904223);
        *r = x;
    }
    c
}
fn same(a: &Cpu, b: &Cpu) {
    assert_eq!(a.ar, b.ar, "registers at {:x}", a.pc);
    assert_eq!(a.pc, b.pc, "PC");
    assert_eq!(a.ps, b.ps);
    assert_eq!(a.sar, b.sar);
    assert_eq!(a.windowbase, b.windowbase);
    assert_eq!(a.windowstart, b.windowstart);
    assert_eq!(a.lcount, b.lcount);
    assert_eq!(a.epc, b.epc);
    assert_eq!(a.exccause, b.exccause);
    assert_eq!(a.insn_count, b.insn_count);
    assert_eq!(a.ccount, b.ccount);
}
fn insn(op: Op) -> BlockInsn {
    let i = Insn {
        op,
        r: 3,
        s: 4,
        t: 5,
        imm: 3,
        imm2: 7,
        len: 3,
        raw: 0,
    };
    BlockInsn {
        insn: i,
        max_ar: crate::exec::max_ar(&i),
        off: 0,
    }
}
fn compare(
    block: &mut [BlockInsn],
    seed: u32,
    entry: u32,
    budget: u32,
    addr: Option<u32>,
    fast: bool,
    readonly: bool,
    loop_end: bool,
    overflow: bool,
) {
    compare_configured(block, seed, entry, budget, addr, fast, readonly, loop_end, overflow, |_| {});
}
fn compare_configured(
    block: &mut [BlockInsn], seed: u32, entry: u32, budget: u32, addr: Option<u32>,
    fast: bool, readonly: bool, loop_end: bool, overflow: bool, configure: impl Fn(&mut Cpu),
) {
    let mut cc = CodeCache::new(0).unwrap();
    let code = queue(&mut cc, block, BASE, fast);
    for _ in 0..HOT {
        ready(&cc, code);
    }
    assert!(ready(&cc, code), "compiled module must execute");
    let (mut a, mut b) = (cpu(seed), cpu(seed));
    let (mut ra, mut rb) = (Ram::new(fast, readonly), Ram::new(fast, readonly));
    for c in [&mut a, &mut b] {
        c.pc = BASE + entry * 3;
        if let Some(addr) = addr {
            c.set_ar(4, addr.wrapping_sub(3));
        }
        if loop_end {
            c.lend = BASE + 6;
            c.lbeg = BASE;
            c.lcount = 2;
        }
        if overflow {
            c.ps = ps::WOE;
            c.windowstart = 1 << ((c.windowbase + 1) % 16);
        }
        configure(c);
    }
    let fm = rb.fast_mem();
    let result = unsafe {
        run(
            &cc,
            code,
            &mut b,
            &mut rb,
            &Helpers::new::<Ram>(),
            budget,
            entry,
            fm,
        )
    };
    let done = result & 0xffff;
    let exit = result >> 16;
    let mut count = 0;
    let mut trap = None;
    let mut pre = false;
    for instruction in block.iter().skip(entry as usize).take(budget as usize) {
        if let Some(t) = a.check_overflow(instruction.max_ar) {
            trap = Some(t);
            pre = true;
            break;
        }
        let pc = a.pc;
        ra.note_pc(pc);
        let r = exec_insn(&mut a, &mut ra, &instruction.insn);
        count += 1;
        if let Err(t) = r {
            trap = Some(t);
            break;
        }
        // A pre-instruction trap after a completed prefix must still be checked
        // on the next iteration; it retires no additional instruction.
        if a.pc != pc + 3 || (count == done && exit != CODE_TRAP_PRE) {
            break;
        }
    }
    assert_eq!(count, done);
    assert_eq!(pre, exit == CODE_TRAP_PRE);
    assert_eq!(trap, b.jit_trap.take());
    same(&a, &b);
    assert_eq!(ra.ram.mem, rb.ram.mem);
    assert_eq!(ra.versions, rb.versions);
    if done > 0 {
        assert_eq!(ra.noted, rb.noted);
    }
}

fn terminal_helpers() -> u32 {
    use Op::*;
    let mut tests = 0;
    for op in [Call0, Call4, Call8, Call12, Callx0, Callx4, Callx8, Callx12, Ret, RetN, Retw, RetwN] {
        let mut block = [insn(Add), insn(MovN), insn(op)];
        if matches!(op, Callx0 | Callx4 | Callx8 | Callx12) {
            block[2].insn.s = match op { Callx0 => 0, Callx4 => 4, Callx8 => 8, _ => 12 };
            block[2].max_ar = crate::exec::max_ar(&block[2].insn);
        }
        // Dirty the implicit return register: the helper must see its spilled value,
        // and helper writes/window rotations must not be overwritten after it returns.
        block[1].insn.t = 0;
        block[1].max_ar = crate::exec::max_ar(&block[1].insn);
        let mut cc = CodeCache::new(0).unwrap();
        assert!(compile(&mut cc, &mut block, BASE, false).is_some());
        assert!(compile(&mut cc, &mut [insn(op), insn(Add)], BASE, false).is_none());
        assert!(compile(&mut cc, &mut [insn(op)], BASE, false).is_none());
        for wb in [0, 7, 15] {
            for flags in [0, ps::WOE, ps::WOE | ps::EXCM] {
                for windows in [0, 1 << 2, 0xffff] {
                    for inc in 0..4 {
                        for entry in 0..3 {
                            for budget in 1..=3 {
                                compare_configured(&mut block, wb, entry, budget, None, false, false,
                                    false, false, |c| {
                                        c.ps = flags;
                                        c.windowbase = wb;
                                        c.windowstart = windows;
                                        let ret = (inc << 30) | ((BASE + 0x400) & 0x3fff_ffff);
                                        c.set_ar(0, ret);
                                        c.set_ar(4, ret);
                                    });
                                tests += 1;
                            }
                        }
                    }
                }
            }
        }
    }
    tests
}

fn whole_block_guards() -> u32 {
    let mut tests = 0;
    for offset in [-3i32, 0, 1, 3, 4, 6, 9, 10, 12] {
        for count in [0, 1, 0xffff_ffff] {
            for flags in [0, ps::WOE, ps::WOE | ps::EXCM] {
                for windows in [0, 0xffff] {
                    for entry in 0..3 {
                        for budget in 1..=3 {
                            compare_configured(&mut [insn(Op::Add), insn(Op::MovN), insn(Op::Xor)],
                                15, entry, budget, None, false, false, false, false, |c| {
                                    c.lend = BASE.wrapping_add(offset as u32);
                                    c.lbeg = BASE + 0x100;
                                    c.lcount = count;
                                    c.ps = flags;
                                    c.windowstart = windows;
                                });
                            tests += 1;
                        }
                    }
                }
            }
        }
    }
    tests
}
fn scheduler() {
    // addi.n a3,a3,1; addi.n a4,a4,1; j back to the first instruction.
    let program = [0x1b, 0x33, 0x1b, 0x44, 0x06, 0xfe, 0xff];
    let (mut a, mut b) = (cpu(7), cpu(7));
    let (mut ra, mut rb) = (Ram::new(true, false), Ram::new(true, false));
    ra.ram.mem[..7].copy_from_slice(&program);
    rb.ram.mem[..7].copy_from_slice(&program);
    let mut total = 0;
    for turn in 0..400 {
        // Cut/resume at every possible position; change a hot instruction after compilation.
        if turn == 250 {
            ra.write8(BASE + 1, 0x55).unwrap();
            rb.write8(BASE + 1, 0x55).unwrap();
        }
        let budget = 1 + turn % 7;
        let (done, trap) = crate::block::run_block(&mut b, &mut rb, budget);
        assert!(trap.is_none());
        for _ in 0..done {
            crate::step(&mut a, &mut ra).unwrap();
        }
        total += done;
        same(&a, &b);
    }
    assert!(
        b.blocks.jit_instructions > 100,
        "scheduler did not use compiled blocks ({total})"
    );
    // A timer deadline inside an already hot block must land at the same instruction.
    for c in [&mut a, &mut b] {
        c.ccompare[0] = c.ccount + 2;
        c.intenable = 1 << 6;
    }
    for _ in 0..10 {
        let (done, trap) = crate::block::run_block(&mut b, &mut rb, 32);
        let mut oracle = None;
        for _ in 0..done {
            if let Err(t) = crate::step(&mut a, &mut ra) {
                oracle = Some(t);
                break;
            }
        }
        assert_eq!(trap, oracle);
        same(&a, &b);
        if matches!(trap, Some(Trap::Interrupt(_))) {
            return;
        }
    }
    panic!("timer interrupt not delivered");
}
fn retention() {
    use Op::*;
    let mut cc = CodeCache::new(0).unwrap();
    let mut code = [insn(Add), insn(S32i), insn(Xor)];
    let first = queue(&mut cc, &mut code, BASE, true);
    for _ in 0..HOT {
        ready(&cc, first);
    }
    let slot = cc.blocks[first as usize].slot.get();
    assert!(slot != 0 && slot != NONE);
    cc.reset();
    let reused = queue(&mut cc, &mut code, BASE, true);
    assert_eq!(cc.blocks[reused as usize].slot.get(), slot);
    // Force the retained module through its slow helper: the embedded instruction
    // pointer must still be live after cache compaction, and the store must run once.
    let mut c = cpu(15);
    c.set_ar(4, BASE + 0x200 - 3);
    let mut ram = Ram::new(false, false);
    let result = unsafe {
        run(
            &cc,
            reused,
            &mut c,
            &mut ram,
            &Helpers::new::<Ram>(),
            3,
            0,
            None,
        )
    };
    assert_eq!(result & 0xffff, 2);
    assert_eq!(ram.versions[2], 1);
    assert_eq!(ram.read32(BASE + 0x200).unwrap(), c.get_ar(5));
    code[0].insn.imm = 123;
    let changed = queue(&mut cc, &mut code, BASE, true);
    assert_ne!(
        changed, reused,
        "decoded fields must all participate in identity"
    );
    assert_eq!(cc.blocks[changed as usize].slot.get(), NONE);
    assert_ne!(
        queue(&mut cc, &mut code[..2], BASE, true),
        changed,
        "boundary split"
    );
    assert_ne!(
        queue(&mut cc, &mut code, BASE, false),
        changed,
        "fast-memory contract"
    );
    for _ in 0..3 {
        cc.reset();
    }
    assert!(cc.blocks.is_empty(), "unused generations must expire");
    for pc in 0..RETAIN_BLOCKS + 7 {
        queue(&mut cc, &mut code, pc as u32, false);
    }
    cc.reset();
    assert_eq!(cc.blocks.len(), RETAIN_BLOCKS);
}

fn window_masks() -> u32 {
    let mut cc = CodeCache::new(0).unwrap();
    let mut cases = 0;
    for high in [3, 7, 11, 15] {
        let mut low = insn(Op::Movi);
        low.insn.t = 1;
        low.max_ar = 1;
        let mut upper = insn(Op::Add);
        upper.insn.r = high;
        upper.insn.s = 2;
        upper.insn.t = 3;
        upper.max_ar = crate::exec::max_ar(&upper.insn);
        let mut block = [low, upper];
        let id = queue(&mut cc, &mut block, BASE, false);
        for _ in 0..HOT {
            ready(&cc, id);
        }
        for wb in 0..16 {
            for frame in 1..=3 {
                for status in [0, ps::WOE, ps::WOE | ps::EXCM] {
                    for entry in 0..2 {
                        for budget in 1..=2 {
                            let (mut a, mut b) = (cpu(wb), cpu(wb));
                            for c in [&mut a, &mut b] {
                                c.pc = BASE + entry * 3;
                                c.ps = status;
                                c.windowstart = 1 << ((wb + frame) & 15);
                            }
                            let (mut ra, mut rb) = (Ram::new(false, false), Ram::new(false, false));
                            let actual = unsafe {
                                run(
                                    &cc,
                                    id,
                                    &mut b,
                                    &mut rb,
                                    &Helpers::new::<Ram>(),
                                    budget,
                                    entry,
                                    None,
                                )
                            };
                            let (mut done, mut trap) = (0, None);
                            for bi in block.iter().skip(entry as usize).take(budget as usize) {
                                if let Some(t) = a.check_overflow(bi.max_ar) {
                                    trap = Some(t);
                                    break;
                                }
                                exec_insn(&mut a, &mut ra, &bi.insn).unwrap();
                                done += 1;
                            }
                            assert_eq!(actual & 0xffff, done);
                            assert_eq!(trap, b.jit_trap.take());
                            same(&a, &b);
                            cases += 1;
                        }
                    }
                }
            }
        }
    }
    cases
}

pub fn run_tests() -> u32 {
    use Op::*;
    let ops = [
        Nop, NopN, Memw, Extw, Movi, MoviN, Mov, MovN, Add, AddN, Sub, And, Or, Xor, Mull, Salt,
        Saltu, Addi, AddiN, Addmi, Addx2, Addx4, Addx8, Subx2, Subx4, Subx8, Neg, Slli, Srli, Srai,
        Extui, Sext, Ssr, Ssl, Ssa8l, Ssa8b, Ssai, Abs, Src, Min, Max, Minu, Maxu, Moveqz, Movnez,
        Movltz, Movgez, Nsau, J, Jx, Beqz, BeqzN, Bnez, BnezN, Bltz, Bgez, Beqi, Bnei, Blti, Bgei,
        Bltui, Bgeui, Beq, Bne, Blt, Bge, Bltu, Bgeu, Bbci, Bbsi, Bbc, Bbs,
    ];
    let mut tests = 0;
    for op in ops {
        for seed in [0, 1, 15, 0xffff_ffff] {
            for entry in 0..3 {
                for budget in 1..=3 {
                    let mut block = [insn(Add), insn(op), insn(Xor)];
                    compare(
                        &mut block, seed, entry, budget, None, false, false, false, false,
                    );
                    tests += 1;
                }
            }
        }
    }
    for op in [
        L8ui, L16ui, L16si, L32i, L32iN, L32r, S8i, S16i, S32i, S32iN,
    ] {
        for addr in [BASE + 0x100, BASE + 0x1ff, BASE + 65535, BASE - 16] {
            for fast in [false, true] {
                for readonly in [false, true] {
                    let mut block = [insn(Add), insn(op), insn(Xor)];
                    if op == L32r {
                        block[1].insn.imm = addr as i32;
                    }
                    compare(
                        &mut block,
                        15,
                        0,
                        3,
                        Some(addr),
                        fast,
                        readonly,
                        false,
                        false,
                    );
                    tests += 1;
                }
            }
        }
    }
    for overflow in [false, true] {
        for entry in 0..3 {
            compare(
                &mut [insn(Add), insn(Abs), insn(Xor)],
                15,
                entry,
                3,
                None,
                false,
                false,
                true,
                overflow,
            );
            tests += 1;
        }
    }
    scheduler();
    retention();
    tests + 2 + window_masks() + terminal_helpers() + whole_block_guards()
}
