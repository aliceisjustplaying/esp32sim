use crate::{Backend, CoreId, ExecutionOutcome, Operation, TraceEvent};

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
