//! Single-core measured timing policy for the ESP32-S3 interpreter.

use backend_api::{CostBinding, CostClaim, ImportedTimingProfile, TimingBlock};
use std::collections::BTreeSet;
use xtensa_lx7::decode::Op;
use xtensa_lx7::measured::{AccessKind, InstructionObservation, MemoryClass, Price, TimingSource};

#[derive(Clone)]
pub struct Esp32TimingSource {
    profile: ImportedTimingProfile,
    instruction_lines: BTreeSet<(String, u32)>,
    data_lines: BTreeSet<(String, u32)>,
    last_instruction_fill: Option<(String, u32)>,
    last_data_fill: Option<(String, u32)>,
    last_load: Option<(u8, String)>,
}

impl Esp32TimingSource {
    pub fn new(profile: ImportedTimingProfile) -> Self {
        Self {
            profile,
            instruction_lines: BTreeSet::new(),
            data_lines: BTreeSet::new(),
            last_instruction_fill: None,
            last_data_fill: None,
            last_load: None,
        }
    }

    pub fn profile(&self) -> &ImportedTimingProfile {
        &self.profile
    }

    fn exact(
        &self,
        binding: CostBinding,
        cycles: &mut u64,
        claims: &mut Vec<CostClaim>,
    ) -> Result<(), TimingBlock> {
        let claim = self.profile.claim_for(&binding)?.clone();
        let additional = self.profile.exact_cycles_for(&binding)?;
        *cycles = cycles.checked_add(additional).ok_or_else(|| TimingBlock {
            claim_id: claim.id.clone(),
            tier_candidate: "exact".into(),
            reason: "instruction cost overflow".into(),
        })?;
        claims.push(claim);
        Ok(())
    }

    fn memory_name(memory: &MemoryClass) -> Result<&'static str, TimingBlock> {
        match memory {
            MemoryClass::InternalSram => Ok("sram"),
            MemoryClass::MaskRom => Ok("rom"),
            MemoryClass::Flash => Ok("flash"),
            MemoryClass::Psram => Ok("psram"),
            MemoryClass::Rtc => Ok("rtc"),
            MemoryClass::Mmio { .. } => Ok("mmio"),
            MemoryClass::Unknown => Err(TimingBlock {
                claim_id: "memory-class".into(),
                tier_candidate: "unexplained".into(),
                reason: "address has no measured memory classification".into(),
            }),
        }
    }

    fn cache_cost(
        &self,
        cache: &str,
        memory: &MemoryClass,
        address: u32,
        lines: &BTreeSet<(String, u32)>,
        last_fill: &Option<(String, u32)>,
        line_bytes: u32,
        cycles: &mut u64,
        claims: &mut Vec<CostClaim>,
        staged: &mut Vec<String>,
    ) -> Result<(), TimingBlock> {
        let memory_name = Self::memory_name(memory)?;
        if !matches!(memory, MemoryClass::Flash | MemoryClass::Psram) {
            return Ok(());
        }
        let line = address / line_bytes;
        let key = (memory_name.to_string(), line);
        if lines.contains(&key) {
            return Ok(());
        }
        let event = if last_fill
            .as_ref()
            .is_some_and(|(previous_memory, previous)| {
                previous_memory == memory_name && previous.checked_add(1) == Some(line)
            }) {
            "subsequent-line-fill"
        } else {
            "first-line-fill"
        };
        self.exact(
            CostBinding::Cache {
                cache: cache.into(),
                memory: memory_name.into(),
                event: event.into(),
            },
            cycles,
            claims,
        )?;
        staged.push(format!("cache:{cache}:{memory_name}:{line}"));
        Ok(())
    }

    fn load_target(observation: &InstructionObservation) -> Option<(u8, String)> {
        use Op::*;
        match observation.instruction.op {
            L8ui | L16ui | L16si | L32i | L32iN | L32ai | L32r | L32e => {
                Some((observation.instruction.t, "l32i".into()))
            }
            _ => None,
        }
    }

    fn reads_register(observation: &InstructionObservation, register: u8) -> Option<bool> {
        use Op::*;
        let instruction = &observation.instruction;
        let reads = match instruction.op {
            Nop | NopN | Waiti | Isync | Rsync | Esync | Dsync | Excw | Memw | Extw | L32r => {
                vec![]
            }
            L8ui | L16ui | L16si | L32i | L32iN | L32ai | L32e | Lsi | Lsip => {
                vec![instruction.s]
            }
            S8i | S16i | S32i | S32iN | S32ri | S32e | S32nb | Ssi | Ssip | S32c1i => {
                vec![instruction.s, instruction.t]
            }
            Lsx | Lsxp | Ssx | Ssxp => vec![instruction.s, instruction.t],
            Add | AddN | Sub | Addx2 | Addx4 | Addx8 | Subx2 | Subx4 | Subx8 | And | Or | Xor
            | Min | Max | Minu | Maxu | Beq | Bne | Blt | Bge | Bltu | Bgeu | Bnone | Bany
            | Ball | Bnall | Bbc | Bbs => vec![instruction.s, instruction.t],
            Addi | AddiN | Addmi | Neg | Abs | Extui | Sext | Clamps | Slli | Srai | Srli
            | Beqz | BeqzN | Bnez | BnezN | Bltz | Bgez | Beqi | Bnei | Blti | Bgei | Bltui
            | Bgeui | Bbci | Bbsi | Loop | Loopnez | Loopgtz | Mov | MovN => {
                vec![instruction.s]
            }
            Movi | MoviN => vec![],
            _ => return None,
        };
        Some(reads.contains(&register))
    }
}

impl TimingSource for Esp32TimingSource {
    fn price(&self, observation: &InstructionObservation) -> Result<Price, TimingBlock> {
        let mut cycles = 0;
        let mut claims = Vec::new();
        let mut staged = Vec::new();
        self.exact(
            CostBinding::BlockBase {
                class: "straight-line".into(),
            },
            &mut cycles,
            &mut claims,
        )?;

        self.cache_cost(
            "instruction",
            &observation.fetch_memory,
            observation.pc,
            &self.instruction_lines,
            &self.last_instruction_fill,
            32,
            &mut cycles,
            &mut claims,
            &mut staged,
        )?;

        if observation.window_overflow_pair && observation.live_window_depth > 6 {
            self.exact(
                CostBinding::WindowExceptionPair { minimum_depth: 7 },
                &mut cycles,
                &mut claims,
            )?;
        }
        if observation.loop_back_edge_residue == Some(3) {
            self.exact(
                CostBinding::LoopAlignment { residue_mod_4: 3 },
                &mut cycles,
                &mut claims,
            )?;
        }

        if let Some((target, producer)) = &self.last_load {
            match Self::reads_register(observation, *target) {
                Some(true) => self.exact(
                    CostBinding::DependentLoadUse {
                        producer: producer.clone(),
                        consumer: "any-register-use".into(),
                    },
                    &mut cycles,
                    &mut claims,
                )?,
                Some(false) => {}
                None => {
                    return Err(TimingBlock {
                        claim_id: format!("load-use:{producer}"),
                        tier_candidate: "unexplained".into(),
                        reason: "consumer operation has no reviewed register dependency classifier"
                            .into(),
                    })
                }
            }
        }

        if let (Some(access), Some(memory)) = (observation.access, &observation.access_memory) {
            match memory {
                MemoryClass::Mmio { peripheral } => {
                    let operation = match access.kind {
                        AccessKind::Load => "read",
                        AccessKind::Store => "write",
                        AccessKind::Atomic => {
                            return Err(TimingBlock {
                                claim_id: format!("mmio:{:08x}", access.address),
                                tier_candidate: "unexplained".into(),
                                reason: "MMIO atomic access has no adopted timing class".into(),
                            })
                        }
                    };
                    self.exact(
                        CostBinding::Mmio {
                            address: access.address,
                            width: access.width,
                            operation: operation.into(),
                            peripheral: peripheral.clone(),
                            write_effect: None,
                        },
                        &mut cycles,
                        &mut claims,
                    )?;
                }
                MemoryClass::Flash | MemoryClass::Psram => {
                    self.cache_cost(
                        "data",
                        memory,
                        access.address,
                        &self.data_lines,
                        &self.last_data_fill,
                        64,
                        &mut cycles,
                        &mut claims,
                        &mut staged,
                    )?;
                }
                MemoryClass::Unknown => return Err(Self::memory_name(memory).unwrap_err()),
                _ => {}
            }
        }

        if let Some((register, producer)) = Self::load_target(observation) {
            staged.push(format!("load:{register}:{producer}"));
        } else {
            staged.push("load:none".into());
        }
        Ok(Price {
            cycles,
            claims,
            staged_mutations: staged,
        })
    }

    fn commit(&mut self, staged_mutations: &[String]) {
        for mutation in staged_mutations {
            let fields: Vec<_> = mutation.split(':').collect();
            match fields.as_slice() {
                ["cache", "instruction", memory, line] => {
                    let line = line.parse().expect("validated instruction cache line");
                    self.instruction_lines.insert(((*memory).into(), line));
                    self.last_instruction_fill = Some(((*memory).into(), line));
                }
                ["cache", "data", memory, line] => {
                    let line = line.parse().expect("validated data cache line");
                    self.data_lines.insert(((*memory).into(), line));
                    self.last_data_fill = Some(((*memory).into(), line));
                }
                ["load", "none"] => self.last_load = None,
                ["load", register, producer] => {
                    self.last_load = Some((
                        register.parse().expect("validated load target"),
                        (*producer).into(),
                    ));
                }
                _ => unreachable!("validated measured timing mutation"),
            }
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DeviceDeadline {
    At(u64),
    None,
    Unknown { device: String, reason: String },
}

#[cfg(test)]
mod tests {
    use super::*;
    use backend_api::test_claim;
    use std::collections::BTreeMap;
    use xtensa_lx7::decode::{Insn, Op};

    fn profile(entries: Vec<(CostBinding, &str, u64)>) -> ImportedTimingProfile {
        let mut claims = BTreeMap::new();
        let mut bindings = BTreeMap::new();
        for (binding, id, cycles) in entries {
            claims.insert(id.into(), test_claim(id, cycles));
            bindings.insert(binding, id.into());
        }
        ImportedTimingProfile {
            profile_sha256: [7; 32],
            claims,
            bindings,
        }
    }

    fn observation(op: Op, fetch_memory: MemoryClass) -> InstructionObservation {
        InstructionObservation {
            pc: 0x4037_0000,
            bytes: [0; 4],
            instruction: Insn {
                op,
                r: 0,
                s: 0,
                t: 0,
                imm: 0,
                imm2: 0,
                len: 3,
                raw: 0,
            },
            fetch_memory,
            access: None,
            access_memory: None,
            window_overflow_pair: false,
            live_window_depth: 1,
            loop_back_edge_residue: None,
        }
    }

    #[test]
    fn internal_sram_instruction_uses_the_adopted_base_claim() {
        let source = Esp32TimingSource::new(profile(vec![(
            CostBinding::BlockBase {
                class: "straight-line".into(),
            },
            "base",
            1,
        )]));
        let price = source
            .price(&observation(Op::Nop, MemoryClass::InternalSram))
            .unwrap();
        assert_eq!(price.cycles, 1);
        assert_eq!(price.claims[0].id, "base");
    }

    #[test]
    fn flash_lines_stage_first_then_subsequent_fill_state() {
        let mut source = Esp32TimingSource::new(profile(vec![
            (
                CostBinding::BlockBase {
                    class: "straight-line".into(),
                },
                "base",
                1,
            ),
            (
                CostBinding::Cache {
                    cache: "instruction".into(),
                    memory: "flash".into(),
                    event: "first-line-fill".into(),
                },
                "first",
                400,
            ),
            (
                CostBinding::Cache {
                    cache: "instruction".into(),
                    memory: "flash".into(),
                    event: "subsequent-line-fill".into(),
                },
                "next",
                266,
            ),
        ]));
        let mut first = observation(Op::Nop, MemoryClass::Flash);
        first.pc = 0x4200_0000;
        let first_price = source.price(&first).unwrap();
        assert_eq!(first_price.cycles, 401);
        source.commit(&first_price.staged_mutations);

        let mut next = first;
        next.pc += 32;
        let next_price = source.price(&next).unwrap();
        assert_eq!(next_price.cycles, 267);
        source.commit(&next_price.staged_mutations);

        assert_eq!(source.price(&next).unwrap().cycles, 1);
    }

    #[test]
    fn unreviewed_consumer_blocks_after_a_load() {
        let mut source = Esp32TimingSource::new(profile(vec![(
            CostBinding::BlockBase {
                class: "straight-line".into(),
            },
            "base",
            1,
        )]));
        let load = observation(Op::L32i, MemoryClass::InternalSram);
        let load_price = source.price(&load).unwrap();
        source.commit(&load_price.staged_mutations);

        let error = source
            .price(&observation(Op::Call0, MemoryClass::InternalSram))
            .unwrap_err();
        assert!(error.reason.contains("dependency classifier"));
    }
}
