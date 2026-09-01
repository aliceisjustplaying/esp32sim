use backend_api::*;
use proptest::prelude::*;

fn fake_backend(costs: &[u64]) -> FakeBackend {
    let program = costs
        .iter()
        .enumerate()
        .map(|(index, &cycles)| FakeInstruction {
            pc: 0x4000_0400 + index as u32 * 3,
            cost: Ok(test_claim(&format!("instruction-{index}"), cycles)),
            output: (index % 3 == 0).then(|| vec![index as u8]),
        })
        .collect();
    let mut backend = FakeBackend::new(BackendConfig::default(), program).unwrap();
    backend
        .load(vec![Artifact::new(
            "timing-profile",
            ArtifactKind::TimingProfile,
            b"schema-2".to_vec(),
        )])
        .unwrap();
    backend.reset(ResetKind::PowerOn).unwrap();
    backend
}

fn request(deadline: u64) -> RunRequest {
    RunRequest {
        deadline,
        budget: RunBudget::default(),
        cancellation: CancellationFlag::new(),
    }
}

#[test]
fn version_and_capability_refusals_are_typed() {
    let mut incompatible = BackendConfig::default();
    incompatible.requested_adapter.major = 2;
    assert!(matches!(
        FakeBackend::new(incompatible, vec![]),
        Err(BackendError::InvalidConfig(_))
    ));
    let mut dual = BackendConfig::default();
    dual.core_count = 2;
    assert!(matches!(
        FakeBackend::new(dual, vec![]),
        Err(BackendError::UnsupportedCapability(capability)) if capability == "measured-dual-core"
    ));
}

#[test]
fn artifacts_are_hash_checked_before_load() {
    let mut backend = FakeBackend::new(BackendConfig::default(), vec![]).unwrap();
    let mut artifact = Artifact::new("profile", ArtifactKind::TimingProfile, vec![1, 2, 3]);
    artifact.sha256[0] ^= 1;
    assert!(matches!(
        backend.load(vec![artifact]),
        Err(BackendError::InvalidArtifact(message)) if message.contains("hash mismatch")
    ));
}

#[test]
fn pending_instruction_survives_cycle_slice() {
    let mut backend = fake_backend(&[10]);
    let first = backend.run_until(request(4)).unwrap();
    assert_eq!(first.end_cycle, 4);
    assert_eq!(first.completed_instructions, 0);
    assert_eq!(first.pending_instruction.unwrap().completion, 10);
    let second = backend.run_until(request(10)).unwrap();
    assert_eq!(second.completed_instructions, 1);
    assert_eq!(second.pending_instruction, None);
}

#[test]
fn zero_instruction_budget_withholds_ready_commit() {
    let mut backend = fake_backend(&[5]);
    let first = backend
        .run_until(RunRequest {
            deadline: 5,
            budget: RunBudget {
                max_instructions: 0,
                ..RunBudget::default()
            },
            cancellation: CancellationFlag::new(),
        })
        .unwrap();
    assert_eq!(first.end_cycle, 0);
    assert!(matches!(
        first.stop,
        RunStop::BudgetExhausted(BudgetKind::Instructions)
    ));

    let mut backend = fake_backend(&[5]);
    backend.run_until(request(3)).unwrap();
    let ready = backend
        .run_until(RunRequest {
            deadline: 5,
            budget: RunBudget {
                max_instructions: 0,
                ..RunBudget::default()
            },
            cancellation: CancellationFlag::new(),
        })
        .unwrap();
    assert_eq!(ready.end_cycle, 5);
    assert!(ready.pending_instruction.is_some());
    assert!(matches!(
        ready.stop,
        RunStop::BudgetExhausted(BudgetKind::Instructions)
    ));
    let committed = backend.run_until(request(5)).unwrap();
    assert_eq!(committed.completed_instructions, 1);
}

#[test]
fn same_cycle_input_precedes_cpu_commit() {
    let mut backend = fake_backend(&[5]);
    backend
        .inject(InputEvent {
            epoch: 1,
            cycle: 5,
            caller_sequence: 7,
            payload: InputPayload::Bytes(vec![42]),
        })
        .unwrap();
    let slice = backend.run_until(request(5)).unwrap();
    let kinds: Vec<_> = slice
        .ledger
        .entries
        .iter()
        .map(|entry| &entry.kind)
        .collect();
    let input = kinds
        .iter()
        .position(|kind| matches!(kind, LedgerKind::InputApplied { .. }))
        .unwrap();
    let commit = kinds
        .iter()
        .position(|kind| matches!(kind, LedgerKind::InstructionCommit { .. }))
        .unwrap();
    assert!(input < commit);
}

#[test]
fn input_injected_across_pending_slice_applies_at_its_cycle() {
    let mut backend = fake_backend(&[10]);
    let first = backend.run_until(request(3)).unwrap();
    assert_eq!(first.pending_instruction.unwrap().completion, 10);
    backend
        .inject(InputEvent {
            epoch: 1,
            cycle: 6,
            caller_sequence: 1,
            payload: InputPayload::Bytes(vec![0x55]),
        })
        .unwrap();
    let second = backend.run_until(request(8)).unwrap();
    assert_eq!(second.end_cycle, 8);
    assert!(second.pending_instruction.is_some());
    let input = second
        .ledger
        .entries
        .iter()
        .find(|entry| matches!(entry.kind, LedgerKind::InputApplied { .. }))
        .unwrap();
    assert_eq!(input.cycle, 6);
    let final_slice = backend.run_until(request(10)).unwrap();
    assert_eq!(final_slice.completed_instructions, 1);
}

#[test]
fn reset_request_precedes_same_cycle_input_regardless_of_caller_order() {
    let mut backend = fake_backend(&[]);
    backend
        .inject(InputEvent {
            epoch: 1,
            cycle: 5,
            caller_sequence: 1,
            payload: InputPayload::Bytes(vec![1]),
        })
        .unwrap();
    backend
        .inject(InputEvent {
            epoch: 1,
            cycle: 5,
            caller_sequence: 2,
            payload: InputPayload::Reset(ResetKind::Watchdog),
        })
        .unwrap();
    let slice = backend.run_until(request(5)).unwrap();
    assert_eq!(slice.stop, RunStop::ResetRequested(ResetKind::Watchdog));
    assert!(backend.drain_events(usize::MAX).unwrap().events.is_empty());
}

#[test]
fn ccompare_detects_wrap_and_full_u32_period() {
    let mut wrapping = fake_backend(&[]);
    wrapping.set_ccompare(0, 3);
    wrapping.run_until(request(u32::MAX as u64)).unwrap();
    let wrapped = wrapping.run_until(request((u32::MAX as u64) + 4)).unwrap();
    assert!(wrapped.ledger.entries.iter().any(|entry| {
        entry.cycle == (1u64 << 32) + 3
            && matches!(entry.kind, LedgerKind::CcompareAssert { comparator: 0 })
    }));

    let mut full_period = fake_backend(&[]);
    full_period.set_ccompare(0, 0);
    let slice = full_period.run_until(request(1u64 << 32)).unwrap();
    assert!(slice.ledger.entries.iter().any(|entry| {
        entry.cycle == 1u64 << 32
            && matches!(entry.kind, LedgerKind::CcompareAssert { comparator: 0 })
    }));
    assert_eq!(full_period.ccount(), 0);
}

#[test]
fn unknown_cost_blocks_without_state_change() {
    let block = TimingBlock {
        claim_id: "unknown-op".into(),
        tier_candidate: "unexplained".into(),
        reason: "unsupported operation".into(),
    };
    let mut backend = FakeBackend::new(
        BackendConfig::default(),
        vec![FakeInstruction {
            pc: 0x4000_0400,
            cost: Err(block.clone()),
            output: None,
        }],
    )
    .unwrap();
    backend.load(vec![]).unwrap();
    backend.reset(ResetKind::PowerOn).unwrap();
    let before = backend.canonical_ledger();
    let result = backend.run_until(request(100)).unwrap();
    assert_eq!(result.end_cycle, 0);
    assert_eq!(result.pending_instruction, None);
    assert_eq!(result.stop, RunStop::TimingBlocked(block));
    assert_eq!(backend.canonical_ledger(), before);
}

#[test]
fn reset_advances_epoch_and_zeroes_time() {
    let mut backend = fake_backend(&[10]);
    backend.run_until(request(4)).unwrap();
    let reset = backend.reset(ResetKind::Software).unwrap();
    assert_eq!(reset.epoch, 2);
    assert_eq!(reset.cycle, 0);
    assert_eq!(backend.ccount(), 0);
}

proptest! {
    #[test]
    fn slice_invariance_is_byte_exact(
        costs in prop::collection::vec(1u64..40, 1..50),
        raw_cuts in prop::collection::vec(0u64..1000, 0..80),
    ) {
        let total: u64 = costs.iter().sum();
        let mut whole = fake_backend(&costs);
        whole.run_until(request(total)).unwrap();
        let whole_ledger = whole.canonical_ledger();
        let whole_events = whole.drain_events(usize::MAX).unwrap().events;

        let mut partitioned = fake_backend(&costs);
        let mut cuts: Vec<u64> = raw_cuts.into_iter().map(|cut| cut.min(total)).collect();
        cuts.push(total);
        cuts.sort_unstable();
        cuts.dedup();
        for cut in cuts {
            partitioned.run_until(request(cut)).unwrap();
        }
        prop_assert_eq!(partitioned.canonical_ledger(), whole_ledger);
        prop_assert_eq!(partitioned.drain_events(usize::MAX).unwrap().events, whole_events);
    }
}
