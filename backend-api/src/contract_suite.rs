use crate::{
    Backend, CacheFillPosition, CacheKind, CoreId, ExecutionOutcome, InterruptLevel,
    InterruptPhase, Operation, ReceiptId, TraceEvent,
};

/// Run backend-neutral invariants against one adapter implementation.
pub fn assert_backend_contract<B: Backend + Default>() {
    let mut backend = B::default();
    let trace = [
        TraceEvent {
            core: CoreId::Core0,
            pc: 0x4200_0000,
            operation: Operation::BranchZero { taken: false },
            outcome: ExecutionOutcome::Committed,
        },
        TraceEvent {
            core: CoreId::Core1,
            pc: 0x4200_1000,
            operation: Operation::BranchZero { taken: true },
            outcome: ExecutionOutcome::Committed,
        },
    ];
    let report = backend
        .run_trace(&trace)
        .expect("contract trace has adopted timing");
    assert_eq!(report.total_cycles, 4);
    assert_eq!(backend.engine().state().cores[0].committed_instructions, 1);
    assert_eq!(backend.engine().state().cores[1].committed_instructions, 1);
}

/// Replay every currently adopted cost class through a backend's shared
/// transaction engine and assert its exact ledger values.
pub fn assert_receipt_correlation<B: Backend + Default>() {
    let operations = [
        Operation::BranchZero { taken: true },
        Operation::BranchZero { taken: false },
        Operation::SameValueMmioWriteRun {
            address: 0x600c_001c,
            value: 1,
            count: 16,
        },
        Operation::CacheLineFill {
            cache: CacheKind::InstructionFlash,
            position: CacheFillPosition::Subsequent,
            line: 1,
        },
        Operation::CacheLineFill {
            cache: CacheKind::DataFlash,
            position: CacheFillPosition::Subsequent,
            line: 2,
        },
        Operation::CacheLineFill {
            cache: CacheKind::DataPsram,
            position: CacheFillPosition::Subsequent,
            line: 3,
        },
        Operation::WindowOverflowUnderflowPair,
        Operation::LoopBackEdge { body_residue: 3 },
        Operation::Interrupt {
            level: InterruptLevel::Level1,
            phase: InterruptPhase::Entry,
        },
        Operation::Interrupt {
            level: InterruptLevel::Level1,
            phase: InterruptPhase::Resume,
        },
        Operation::Interrupt {
            level: InterruptLevel::Level3,
            phase: InterruptPhase::Entry,
        },
        Operation::Interrupt {
            level: InterruptLevel::Level3,
            phase: InterruptPhase::Resume,
        },
    ];
    let trace: Vec<_> = operations
        .into_iter()
        .enumerate()
        .map(|(index, operation)| TraceEvent {
            core: CoreId::Core0,
            pc: 0x4000_0400 + index as u32 * 4,
            operation,
            outcome: ExecutionOutcome::Committed,
        })
        .collect();
    let report = B::default()
        .run_trace(&trace)
        .expect("all replayed costs are adopted");
    let cycles: Vec<_> = report
        .ledger
        .iter()
        .map(|entry| entry.completion - entry.start)
        .collect();
    assert_eq!(cycles, [3, 1, 40, 266, 473, 170, 35, 1, 227, 143, 222, 139]);
    assert_eq!(report.total_cycles, 1_720);
    assert_eq!(
        report.ledger[0].components[0].receipt,
        ReceiptId::BeqzAdoption2bf3ffd
    );
    assert_eq!(
        report.ledger[2].components[0].receipt,
        ReceiptId::MmioWriteAdoptionE8a9f0e
    );
    assert_eq!(
        report.ledger[3].components[0].receipt,
        ReceiptId::CacheBurstAdoptionA91d1d7
    );
    assert_eq!(
        report.ledger[8].components[0].receipt,
        ReceiptId::Idf61ToolchainDelta
    );
}
