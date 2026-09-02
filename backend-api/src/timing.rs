use crate::CoreId;

/// Receipt-backed cost vocabulary.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum CostTier {
    Exact,
    Affine,
    Interval,
    Distribution,
    Unexplained,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum FlashMode {
    Qio,
    Other,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum PsramMode {
    OctalDtr,
    Other,
}

/// Register-derived configuration key for configuration-scoped receipts.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct ChipConfig {
    pub cpu_mhz: u16,
    pub flash_mode: FlashMode,
    pub flash_mhz: u16,
    pub psram_mode: PsramMode,
    pub psram_mhz: u16,
    pub icache_size_bytes: u32,
    pub icache_ways: u8,
    pub icache_line_bytes: u8,
    pub dcache_size_bytes: u32,
    pub dcache_ways: u8,
    pub dcache_line_bytes: u8,
}

impl ChipConfig {
    pub const RECEIPT_SCOPE: Self = Self {
        cpu_mhz: 240,
        flash_mode: FlashMode::Qio,
        flash_mhz: 80,
        psram_mode: PsramMode::OctalDtr,
        psram_mhz: 80,
        icache_size_bytes: 16 * 1024,
        icache_ways: 8,
        icache_line_bytes: 32,
        dcache_size_bytes: 32 * 1024,
        dcache_ways: 8,
        dcache_line_bytes: 64,
    };
}

/// Committed receipts that support the adopted cost classes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum ReceiptId {
    Idf61ToolchainDelta,
    OpcodeLadders,
    RegisterBlocks,
    HotHitAdoption,
    CacheBurstAdoptionA91d1d7,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum InstructionCost {
    Issue,
    Branch { taken: bool },
    Jump,
    JumpRegister,
    LoopSetup,
    Quotient,
    Remainder,
    AtomicStore,
    LoadUse,
    LiteralLoad,
    InstructionSync,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum MmioTier {
    Fast,
    Apb,
    Nrx,
    Rtc,
    Efuse,
}

/// External cache path used by a line-fill transaction.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum CacheKind {
    InstructionFlash,
    DataFlash,
    DataPsram,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum CacheFillPosition {
    First,
    Subsequent,
}

/// A named timing class. This type is suitable for generated JIT block data.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum CostClass {
    Instruction(InstructionCost),
    MmioRead(MmioTier),
    MmioWriteEnqueue,
    MmioWriteDrain(MmioTier),
    CacheLineFill {
        cache: CacheKind,
        position: CacheFillPosition,
    },
    LoopAlignment {
        body_residue: u8,
    },
    IndependentSramAccess,
    HotCacheHit,
    DmaAdditiveDelay,
    UnadoptedInstruction,
    UnknownMmio,
}

/// A scalar expression retained in block metadata for later JIT compilation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CostExpression {
    Exact(u64),
    Affine {
        slope: i64,
        intercept: i64,
        count: u32,
    },
}

impl CostExpression {
    pub fn evaluate(self) -> Option<u64> {
        match self {
            Self::Exact(cycles) => Some(cycles),
            Self::Affine {
                slope,
                intercept,
                count,
            } => slope
                .checked_mul(i64::from(count))?
                .checked_add(intercept)?
                .try_into()
                .ok(),
        }
    }
}

/// One independently receipted component of a block or instruction cost.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CostComponent {
    pub class: CostClass,
    pub tier: CostTier,
    pub expression: CostExpression,
    pub receipt: ReceiptId,
}

impl CostComponent {
    pub fn cycles(self) -> Option<u64> {
        self.expression.evaluate()
    }
}

/// Guest operation observed by the measured transaction engine.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Operation {
    Instruction(InstructionCost),
    MmioRead {
        tier: MmioTier,
    },
    MmioWrite {
        tier: MmioTier,
        buffer_has_room: bool,
    },
    CacheLineFill {
        cache: CacheKind,
        position: CacheFillPosition,
        line: u32,
    },
    LoopBackEdge {
        body_residue: u8,
    },
    IndependentSramAccess,
    HotCacheHit,
    DmaAdditiveDelay,
    UnadoptedInstruction,
    UnknownMmio {
        address: u32,
    },
}

/// Typed state changes staged by pricing and committed after architectural
/// success. No mutation is string encoded.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TimingMutation {
    RecordMmioWrite,
    RecordCacheFill {
        core: CoreId,
        cache: CacheKind,
        line: u32,
    },
    RecordLoopBackEdge {
        core: CoreId,
        body_residue: u8,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RefusalReason {
    UnknownMmioRegister,
    CostNotAdopted,
    InvalidAffineDomain,
    CycleOverflow,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TimingRefusal {
    pub class: CostClass,
    pub tier_candidate: CostTier,
    pub reason: RefusalReason,
    pub configuration: Option<ChipConfig>,
}

/// Price one operation without changing timing state.
pub fn price_operation(
    config: ChipConfig,
    core: CoreId,
    operation: Operation,
) -> Result<(CostComponent, Option<TimingMutation>), TimingRefusal> {
    if config != ChipConfig::RECEIPT_SCOPE {
        return Err(TimingRefusal {
            class: operation.cost_class(),
            tier_candidate: CostTier::Unexplained,
            reason: RefusalReason::CostNotAdopted,
            configuration: Some(config),
        });
    }
    let exact = |class, cycles, receipt| CostComponent {
        class,
        tier: CostTier::Exact,
        expression: CostExpression::Exact(cycles),
        receipt,
    };
    match operation {
        Operation::Instruction(kind) => {
            let class = CostClass::Instruction(kind);
            let cycles = match kind {
                InstructionCost::Issue => 1,
                InstructionCost::Branch { taken } => {
                    if taken {
                        3
                    } else {
                        1
                    }
                }
                InstructionCost::Jump => 3,
                InstructionCost::JumpRegister => 6,
                InstructionCost::LoopSetup => 5,
                InstructionCost::Quotient => 4,
                InstructionCost::Remainder => 5,
                InstructionCost::AtomicStore => 6,
                InstructionCost::LoadUse => 1,
                InstructionCost::LiteralLoad | InstructionCost::InstructionSync => {
                    return Err(TimingRefusal {
                        class,
                        tier_candidate: CostTier::Interval,
                        reason: RefusalReason::CostNotAdopted,
                        configuration: None,
                    });
                }
            };
            let receipt = match kind {
                InstructionCost::Issue => ReceiptId::Idf61ToolchainDelta,
                _ => ReceiptId::OpcodeLadders,
            };
            Ok((exact(class, cycles, receipt), None))
        }
        Operation::MmioRead { tier } => {
            let class = CostClass::MmioRead(tier);
            match tier {
                MmioTier::Fast => Ok((exact(class, 9, ReceiptId::RegisterBlocks), None)),
                MmioTier::Apb => Ok((exact(class, 15, ReceiptId::RegisterBlocks), None)),
                MmioTier::Nrx => Ok((exact(class, 18, ReceiptId::RegisterBlocks), None)),
                MmioTier::Rtc | MmioTier::Efuse => Err(TimingRefusal {
                    class,
                    tier_candidate: CostTier::Distribution,
                    reason: RefusalReason::CostNotAdopted,
                    configuration: None,
                }),
            }
        }
        Operation::MmioWrite {
            tier,
            buffer_has_room,
        } => {
            let (class, cycles, candidate) = if buffer_has_room {
                (CostClass::MmioWriteEnqueue, 1, CostTier::Exact)
            } else {
                let class = CostClass::MmioWriteDrain(tier);
                match tier {
                    MmioTier::Fast => (class, 4, CostTier::Exact),
                    MmioTier::Apb => (class, 15, CostTier::Exact),
                    MmioTier::Nrx => (class, 0, CostTier::Interval),
                    MmioTier::Rtc | MmioTier::Efuse => (class, 0, CostTier::Distribution),
                }
            };
            if candidate != CostTier::Exact {
                return Err(TimingRefusal {
                    class,
                    tier_candidate: candidate,
                    reason: RefusalReason::CostNotAdopted,
                    configuration: None,
                });
            }
            Ok((
                exact(class, cycles, ReceiptId::RegisterBlocks),
                Some(TimingMutation::RecordMmioWrite),
            ))
        }
        Operation::CacheLineFill {
            cache,
            position: CacheFillPosition::First,
            line,
        } => {
            let class = CostClass::CacheLineFill {
                cache,
                position: CacheFillPosition::First,
            };
            let cycles = match cache {
                CacheKind::InstructionFlash => 203,
                CacheKind::DataFlash => 114,
                CacheKind::DataPsram => 81,
            };
            Ok((
                exact(class, cycles, ReceiptId::Idf61ToolchainDelta),
                Some(TimingMutation::RecordCacheFill { core, cache, line }),
            ))
        }
        Operation::CacheLineFill {
            cache,
            position: CacheFillPosition::Subsequent,
            line,
        } => {
            let class = CostClass::CacheLineFill {
                cache,
                position: CacheFillPosition::Subsequent,
            };
            let cycles = match cache {
                CacheKind::InstructionFlash => 266,
                CacheKind::DataFlash => 473,
                CacheKind::DataPsram => 170,
            };
            Ok((
                CostComponent {
                    class,
                    tier: CostTier::Interval,
                    expression: CostExpression::Exact(cycles),
                    receipt: ReceiptId::CacheBurstAdoptionA91d1d7,
                },
                Some(TimingMutation::RecordCacheFill { core, cache, line }),
            ))
        }
        Operation::LoopBackEdge { body_residue } => {
            let class = CostClass::LoopAlignment { body_residue };
            Ok((
                exact(
                    class,
                    u64::from(body_residue == 3),
                    ReceiptId::Idf61ToolchainDelta,
                ),
                Some(TimingMutation::RecordLoopBackEdge { core, body_residue }),
            ))
        }
        Operation::IndependentSramAccess => Ok((
            exact(
                CostClass::IndependentSramAccess,
                0,
                ReceiptId::Idf61ToolchainDelta,
            ),
            None,
        )),
        Operation::HotCacheHit => Ok((
            exact(CostClass::HotCacheHit, 0, ReceiptId::HotHitAdoption),
            None,
        )),
        Operation::DmaAdditiveDelay => Ok((
            exact(CostClass::DmaAdditiveDelay, 0, ReceiptId::RegisterBlocks),
            None,
        )),
        Operation::UnadoptedInstruction => Err(TimingRefusal {
            class: CostClass::UnadoptedInstruction,
            tier_candidate: CostTier::Exact,
            reason: RefusalReason::CostNotAdopted,
            configuration: None,
        }),
        Operation::UnknownMmio { .. } => Err(TimingRefusal {
            class: CostClass::UnknownMmio,
            tier_candidate: CostTier::Unexplained,
            reason: RefusalReason::UnknownMmioRegister,
            configuration: None,
        }),
    }
}

impl Operation {
    fn cost_class(self) -> CostClass {
        match self {
            Self::Instruction(kind) => CostClass::Instruction(kind),
            Self::MmioRead { tier } => CostClass::MmioRead(tier),
            Self::MmioWrite {
                tier,
                buffer_has_room,
            } => {
                if buffer_has_room {
                    CostClass::MmioWriteEnqueue
                } else {
                    CostClass::MmioWriteDrain(tier)
                }
            }
            Self::CacheLineFill {
                cache, position, ..
            } => CostClass::CacheLineFill { cache, position },
            Self::LoopBackEdge { body_residue } => CostClass::LoopAlignment { body_residue },
            Self::IndependentSramAccess => CostClass::IndependentSramAccess,
            Self::HotCacheHit => CostClass::HotCacheHit,
            Self::DmaAdditiveDelay => CostClass::DmaAdditiveDelay,
            Self::UnadoptedInstruction => CostClass::UnadoptedInstruction,
            Self::UnknownMmio { .. } => CostClass::UnknownMmio,
        }
    }
}
