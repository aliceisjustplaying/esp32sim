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

/// Tier expected after the missing evidence is collected.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum TierCandidate {
    Exact,
    Affine,
    Interval,
    Distribution,
    Unexplained,
}

/// Committed receipts that support the adopted cost classes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum ReceiptId {
    BeqzAdoption2bf3ffd,
    MmioWriteAdoptionE8a9f0e,
    CacheBurstAdoptionA91d1d7,
    Idf61ToolchainDelta,
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
    BranchZero {
        taken: bool,
    },
    SameValueMmioWriteRun {
        count: u32,
    },
    CacheLineFill {
        cache: CacheKind,
        position: CacheFillPosition,
    },
    WindowOverflowUnderflowPair,
    LoopAlignment {
        body_residue: u8,
    },
    InternalInstruction,
    UnknownMmio,
    Interrupt {
        level: InterruptLevel,
        phase: InterruptPhase,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum InterruptLevel {
    Level1,
    Level3,
    Other(u8),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum InterruptPhase {
    Entry,
    Resume,
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
    BranchZero {
        taken: bool,
    },
    SameValueMmioWriteRun {
        address: u32,
        value: u32,
        count: u32,
    },
    CacheLineFill {
        cache: CacheKind,
        position: CacheFillPosition,
        line: u32,
    },
    WindowOverflowUnderflowPair,
    LoopBackEdge {
        body_residue: u8,
    },
    InternalInstruction,
    UnknownMmio {
        address: u32,
    },
    Interrupt {
        level: InterruptLevel,
        phase: InterruptPhase,
    },
}

/// Typed state changes staged by pricing and committed after architectural
/// success. No mutation is string encoded.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TimingMutation {
    RecordMmioWrite {
        address: u32,
        value: u32,
        count: u32,
    },
    RecordCacheFill {
        core: CoreId,
        cache: CacheKind,
        line: u32,
    },
    RecordWindowPair {
        core: CoreId,
    },
    RecordLoopBackEdge {
        core: CoreId,
        body_residue: u8,
    },
    RecordInterrupt {
        core: CoreId,
        level: InterruptLevel,
        phase: InterruptPhase,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RefusalReason {
    FirstLinePoolingUnresolved,
    UnknownMmioRegister,
    CostNotAdopted,
    InvalidAffineDomain,
    CycleOverflow,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TimingRefusal {
    pub class: CostClass,
    pub tier_candidate: TierCandidate,
    pub reason: RefusalReason,
}

/// Price one operation without changing timing state.
pub fn price_operation(
    core: CoreId,
    operation: Operation,
) -> Result<(CostComponent, Option<TimingMutation>), TimingRefusal> {
    let exact = |class, cycles, receipt| CostComponent {
        class,
        tier: CostTier::Exact,
        expression: CostExpression::Exact(cycles),
        receipt,
    };
    match operation {
        Operation::BranchZero { taken } => {
            let class = CostClass::BranchZero { taken };
            Ok((
                exact(
                    class,
                    if taken { 3 } else { 1 },
                    ReceiptId::BeqzAdoption2bf3ffd,
                ),
                None,
            ))
        }
        Operation::SameValueMmioWriteRun {
            address,
            value,
            count,
        } => {
            let class = CostClass::SameValueMmioWriteRun { count };
            let component = CostComponent {
                class,
                tier: CostTier::Affine,
                expression: CostExpression::Affine {
                    slope: 3,
                    intercept: -8,
                    count,
                },
                receipt: ReceiptId::MmioWriteAdoptionE8a9f0e,
            };
            if component.cycles().is_none() {
                return Err(TimingRefusal {
                    class,
                    tier_candidate: TierCandidate::Affine,
                    reason: RefusalReason::InvalidAffineDomain,
                });
            }
            Ok((
                component,
                Some(TimingMutation::RecordMmioWrite {
                    address,
                    value,
                    count,
                }),
            ))
        }
        Operation::CacheLineFill {
            cache,
            position: CacheFillPosition::First,
            ..
        } => Err(TimingRefusal {
            class: CostClass::CacheLineFill {
                cache,
                position: CacheFillPosition::First,
            },
            tier_candidate: TierCandidate::Exact,
            reason: RefusalReason::FirstLinePoolingUnresolved,
        }),
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
                exact(class, cycles, ReceiptId::CacheBurstAdoptionA91d1d7),
                Some(TimingMutation::RecordCacheFill { core, cache, line }),
            ))
        }
        Operation::WindowOverflowUnderflowPair => {
            let class = CostClass::WindowOverflowUnderflowPair;
            Ok((
                exact(class, 35, ReceiptId::Idf61ToolchainDelta),
                Some(TimingMutation::RecordWindowPair { core }),
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
        Operation::InternalInstruction => Err(TimingRefusal {
            class: CostClass::InternalInstruction,
            tier_candidate: TierCandidate::Exact,
            reason: RefusalReason::CostNotAdopted,
        }),
        Operation::UnknownMmio { .. } => Err(TimingRefusal {
            class: CostClass::UnknownMmio,
            tier_candidate: TierCandidate::Unexplained,
            reason: RefusalReason::UnknownMmioRegister,
        }),
        Operation::Interrupt { level, phase } => {
            let class = CostClass::Interrupt { level, phase };
            let cycles = match (level, phase) {
                (InterruptLevel::Level1, InterruptPhase::Entry) => 227,
                (InterruptLevel::Level1, InterruptPhase::Resume) => 143,
                (InterruptLevel::Level3, InterruptPhase::Entry) => 222,
                (InterruptLevel::Level3, InterruptPhase::Resume) => 139,
                (InterruptLevel::Other(_), _) => {
                    return Err(TimingRefusal {
                        class,
                        tier_candidate: TierCandidate::Exact,
                        reason: RefusalReason::CostNotAdopted,
                    });
                }
            };
            Ok((
                exact(class, cycles, ReceiptId::Idf61ToolchainDelta),
                Some(TimingMutation::RecordInterrupt { core, level, phase }),
            ))
        }
    }
}
