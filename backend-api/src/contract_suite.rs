//! Backend-neutral contract cases invoked by every version-1 implementation.

use crate::*;

fn request(deadline: u64) -> RunRequest {
    RunRequest {
        deadline,
        budget: RunBudget::default(),
        cancellation: CancellationFlag::new(),
    }
}

/// Runs the shared behavioral contract. The factory must return a loaded,
/// reset backend whose first instructions each cost five cycles.
pub fn run_shared_contract(factory: &mut dyn FnMut() -> Box<dyn Backend>) {
    let mut cancelled = factory();
    let cancellation = CancellationFlag::new();
    cancellation.cancel();
    let cancelled_slice = cancelled
        .run_until(RunRequest {
            deadline: 5,
            budget: RunBudget::default(),
            cancellation,
        })
        .unwrap();
    assert_eq!(cancelled_slice.end_cycle, 0);
    assert_eq!(cancelled_slice.pending_instruction, None);
    assert!(cancelled_slice.ledger.entries.is_empty());
    assert!(matches!(
        cancelled_slice.stop,
        RunStop::BudgetExhausted(BudgetKind::WallCancellation)
    ));

    let mut output_saturated = factory();
    let output_stop = output_saturated
        .run_until(RunRequest {
            deadline: 5,
            budget: RunBudget {
                max_output_events: 0,
                ..RunBudget::default()
            },
            cancellation: CancellationFlag::new(),
        })
        .unwrap();
    assert_eq!(output_stop.end_cycle, 0);
    assert_eq!(output_stop.pending_instruction, None);
    assert!(output_stop.ledger.entries.is_empty());
    assert!(matches!(
        output_stop.stop,
        RunStop::BudgetExhausted(BudgetKind::OutputEvents)
    ));

    let mut sliced = factory();
    let first = sliced.run_until(request(3)).unwrap();
    assert_eq!(first.end_cycle, 3);
    assert_eq!(first.completed_instructions, 0);
    assert_eq!(first.pending_instruction.unwrap().completion, 5);
    let second = sliced.run_until(request(5)).unwrap();
    assert_eq!(second.completed_instructions, 1);

    let mut withheld = factory();
    withheld.run_until(request(3)).unwrap();
    let ready = withheld
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

    let mut mid_pending = factory();
    mid_pending.run_until(request(2)).unwrap();
    mid_pending
        .inject(InputEvent {
            epoch: 1,
            cycle: 4,
            caller_sequence: 1,
            payload: InputPayload::Bytes(vec![0x44]),
        })
        .unwrap();
    let injected = mid_pending.run_until(request(4)).unwrap();
    assert!(injected.pending_instruction.is_some());
    assert!(injected.ledger.entries.iter().any(|entry| {
        entry.cycle == 4 && matches!(entry.kind, LedgerKind::InputApplied { .. })
    }));

    let mut ordered = factory();
    ordered
        .inject(InputEvent {
            epoch: 1,
            cycle: 5,
            caller_sequence: 1,
            payload: InputPayload::Bytes(vec![0x55]),
        })
        .unwrap();
    let boundary = ordered.run_until(request(5)).unwrap();
    let input = boundary
        .ledger
        .entries
        .iter()
        .position(|entry| matches!(entry.kind, LedgerKind::InputApplied { .. }))
        .unwrap();
    let commit = boundary
        .ledger
        .entries
        .iter()
        .position(|entry| matches!(entry.kind, LedgerKind::InstructionCommit { .. }))
        .unwrap();
    assert!(input < commit);

    let mut atomic_budget = factory();
    atomic_budget.run_until(request(4)).unwrap();
    atomic_budget
        .inject(InputEvent {
            epoch: 1,
            cycle: 5,
            caller_sequence: 1,
            payload: InputPayload::Bytes(vec![0x66]),
        })
        .unwrap();
    let stopped = atomic_budget
        .run_until(RunRequest {
            deadline: 5,
            budget: RunBudget {
                max_ledger_entries: 1,
                ..RunBudget::default()
            },
            cancellation: CancellationFlag::new(),
        })
        .unwrap();
    assert_eq!(stopped.end_cycle, 5);
    assert!(stopped.ledger.entries.is_empty());
    assert!(stopped.pending_instruction.is_some());
    assert!(matches!(
        stopped.stop,
        RunStop::BudgetExhausted(BudgetKind::LedgerEntries)
    ));
    let resumed = atomic_budget.run_until(request(5)).unwrap();
    assert_eq!(resumed.completed_instructions, 1);
    assert_eq!(
        resumed
            .ledger
            .entries
            .iter()
            .filter(|entry| matches!(entry.kind, LedgerKind::InputApplied { .. }))
            .count(),
        1
    );

    let mut byte_bounded = factory();
    assert!(matches!(
        byte_bounded.inject(InputEvent {
            epoch: 1,
            cycle: 1,
            caller_sequence: 1,
            payload: InputPayload::Bytes(vec![0; MAX_QUEUED_INPUT_BYTES + 1]),
        }),
        Err(BackendError::InvalidInput(message)) if message.contains("byte limit")
    ));

    let mut reset_first = factory();
    reset_first
        .inject(InputEvent {
            epoch: 1,
            cycle: 5,
            caller_sequence: 1,
            payload: InputPayload::Bytes(vec![1]),
        })
        .unwrap();
    reset_first
        .inject(InputEvent {
            epoch: 1,
            cycle: 5,
            caller_sequence: 2,
            payload: InputPayload::Reset(ResetKind::Watchdog),
        })
        .unwrap();
    assert_eq!(
        reset_first.run_until(request(5)).unwrap().stop,
        RunStop::ResetRequested(ResetKind::Watchdog)
    );
    assert!(reset_first
        .drain_events(usize::MAX)
        .unwrap()
        .events
        .is_empty());

    let mut whole = factory();
    let whole_slice = whole.run_until(request(30)).unwrap();
    let whole_ledger = canonical_ledger_bytes(&whole_slice.ledger.entries);
    let whole_events = whole.drain_events(usize::MAX).unwrap().events;

    let mut partitioned = factory();
    let mut entries = Vec::new();
    for deadline in [1, 4, 5, 8, 13, 21, 30] {
        entries.extend(
            partitioned
                .run_until(request(deadline))
                .unwrap()
                .ledger
                .entries,
        );
    }
    assert_eq!(canonical_ledger_bytes(&entries), whole_ledger);
    assert_eq!(
        partitioned.drain_events(usize::MAX).unwrap().events,
        whole_events
    );

    let mut reset = factory();
    reset.run_until(request(4)).unwrap();
    let receipt = reset.reset(ResetKind::Software).unwrap();
    assert_eq!(receipt.epoch, 2);
    assert_eq!(receipt.cycle, 0);
    assert!(reset.capabilities().measured_single_core);
    reset.close().unwrap();
    assert!(matches!(
        reset.run_until(request(0)),
        Err(BackendError::Closed)
    ));
}
