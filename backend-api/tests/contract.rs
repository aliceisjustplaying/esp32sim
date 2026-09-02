use backend_api::contract_suite::{assert_backend_contract, assert_receipt_correlation};
use backend_api::{
    Backend, CacheFillPosition, CacheKind, CoreId, CostClass, CostTier, DeadlineError,
    DeadlineModel, ExecutionOutcome, FakeBackend, Operation, RefusalReason, TraceEvent,
    VirtualCycle,
};

#[test]
fn configuration_outside_receipt_scope_is_named_in_refusal() {
    let mut config = backend_api::ChipConfig::RECEIPT_SCOPE;
    config.flash_mhz = 40;
    let refusal = backend_api::price_operation(
        config,
        CoreId::Core0,
        Operation::BranchZero { taken: false },
    )
    .expect_err("unreceipted configuration must fail closed");
    assert_eq!(refusal.configuration, Some(config));
}

fn event(core: CoreId, pc: u32, operation: Operation) -> TraceEvent {
    TraceEvent {
        core,
        pc,
        operation,
        outcome: ExecutionOutcome::Committed,
    }
}

#[test]
fn fake_backend_passes_shared_contract() {
    assert_backend_contract::<FakeBackend>();
    assert_receipt_correlation::<FakeBackend>();
}

#[test]
fn scheduler_carries_both_cores() {
    assert_eq!(CoreId::ALL, [CoreId::Core0, CoreId::Core1]);
    let backend = FakeBackend::default();
    assert_eq!(backend.engine().state().cores.len(), 2);
}

#[test]
fn first_line_fill_refuses_with_tier_candidate() {
    let mut backend = FakeBackend::default();
    let refusal = backend
        .execute(event(
            CoreId::Core0,
            0x4200_0000,
            Operation::CacheLineFill {
                cache: CacheKind::InstructionFlash,
                position: CacheFillPosition::First,
                line: 0x4200_0000,
            },
        ))
        .expect_err("first-line cost is not adopted");
    assert_eq!(
        refusal.class,
        CostClass::CacheLineFill {
            cache: CacheKind::InstructionFlash,
            position: CacheFillPosition::First,
        }
    );
    assert_eq!(refusal.tier_candidate, CostTier::Exact);
    assert_eq!(refusal.reason, RefusalReason::FirstLinePoolingUnresolved);
}

#[test]
fn unknown_mmio_blocks_trace_total() {
    let mut backend = FakeBackend::default();
    let trace = [event(
        CoreId::Core0,
        0x4038_0000,
        Operation::UnknownMmio {
            address: 0x6000_0000,
        },
    )];
    let refusal = backend
        .run_trace(&trace)
        .expect_err("unknown MMIO cannot produce a total");
    assert_eq!(refusal.class, CostClass::UnknownMmio);
    assert_eq!(refusal.tier_candidate, CostTier::Unexplained);
}

#[test]
fn faulted_instruction_commits_no_timing_state() {
    let mut backend = FakeBackend::default();
    let before = backend.engine().state().clone();
    let receipt = backend
        .execute(TraceEvent {
            core: CoreId::Core0,
            pc: 0x4200_0000,
            operation: Operation::CacheLineFill {
                cache: CacheKind::DataFlash,
                position: CacheFillPosition::Subsequent,
                line: 0x3c00_0040,
            },
            outcome: ExecutionOutcome::Faulted,
        })
        .expect("the operation itself has adopted timing");
    assert_eq!(receipt.entry, None);
    assert_eq!(backend.engine().state(), &before);
    assert!(backend.engine().ledger().is_empty());
}

#[test]
fn identical_trace_has_byte_identical_ledger_twice() {
    let trace = [
        event(
            CoreId::Core0,
            0x4200_0000,
            Operation::BranchZero { taken: true },
        ),
        event(
            CoreId::Core1,
            0x4200_1000,
            Operation::LoopBackEdge { body_residue: 3 },
        ),
    ];
    let first = FakeBackend::default()
        .run_trace(&trace)
        .expect("trace is adopted");
    let second = FakeBackend::default()
        .run_trace(&trace)
        .expect("trace is adopted");
    assert_eq!(first.canonical_ledger, second.canonical_ledger);
}

#[derive(Default)]
struct Deadlines {
    now: VirtualCycle,
    pending: Vec<VirtualCycle>,
    applied: Vec<VirtualCycle>,
}

impl DeadlineModel for Deadlines {
    fn next_deadline(&self) -> Option<VirtualCycle> {
        self.pending.first().copied()
    }

    fn advance_to(&mut self, cycle: VirtualCycle) -> Result<(), DeadlineError> {
        if cycle < self.now {
            return Err(DeadlineError::TimeReversed {
                current: self.now,
                requested: cycle,
            });
        }
        let split = self.pending.partition_point(|deadline| *deadline <= cycle);
        self.applied.extend(self.pending.drain(..split));
        self.now = cycle;
        Ok(())
    }
}

#[test]
fn deadline_contract_is_future_facing_and_monotonic() {
    let mut model = Deadlines {
        pending: vec![10, 20, 30],
        ..Deadlines::default()
    };
    assert_eq!(model.next_deadline(), Some(10));
    model.advance_to(20).expect("forward advance succeeds");
    assert_eq!(model.applied, [10, 20]);
    assert_eq!(model.next_deadline(), Some(30));
    assert_eq!(
        model.advance_to(19),
        Err(DeadlineError::TimeReversed {
            current: 20,
            requested: 19,
        })
    );
}
