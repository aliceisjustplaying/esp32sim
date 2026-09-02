//! Derived peripheral clocks. The CPU clock is the time base; each slower domain is an integer
//! divider of it, delivered with "done" accounting so a domain never loses or gains a tick to
//! rounding when the CPU advances in odd-sized batches.

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ClockDomain { Cpu, Apb, Systimer, RtcSlow, Xtal }

pub struct ClockTree {
    pub cpu_hz: u64,
    /// (domain, CPU cycles per tick)
    dividers: Vec<(ClockDomain, u64)>,
    cycle_total: u64,
    done: Vec<u64>,
}

impl ClockTree {
    pub fn new(cpu_hz: u64, dividers: &[(ClockDomain, u64)]) -> Self {
        ClockTree { cpu_hz, dividers: dividers.to_vec(), cycle_total: 0, done: vec![0; dividers.len()] }
    }
    pub fn cycles(&self) -> u64 { self.cycle_total }
    /// Advance by `cycles` CPU cycles and report every domain that gained ticks, in the order the
    /// dividers were given, as (domain, ticks).
    pub fn advance(&mut self, cycles: u64, mut f: impl FnMut(ClockDomain, u64)) {
        self.cycle_total += cycles;
        for (i, &(d, div)) in self.dividers.iter().enumerate() {
            let now = self.cycle_total / div;
            if now > self.done[i] { f(d, now - self.done[i]); self.done[i] = now; }
        }
    }
    /// CPU cycles per tick of `d` (the CPU itself is 1).
    pub fn divider(&self, d: ClockDomain) -> u64 { self.dividers.iter().find(|x| x.0 == d).map_or(1, |x| x.1) }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn delivered_ticks_never_drift() {
        let mut t = ClockTree::new(240_000_000, &[(ClockDomain::Systimer, 15), (ClockDomain::Apb, 3)]);
        let (mut st, mut apb) = (0, 0);
        for c in [1u64, 7, 64, 512, 5, 3, 100] { t.advance(c, |d, n| match d { ClockDomain::Systimer => st += n, ClockDomain::Apb => apb += n, _ => {} }); }
        assert_eq!(st, 692 / 15); assert_eq!(apb, 692 / 3); assert_eq!(t.cycles(), 692);
    }
}
