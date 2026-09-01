use crate::{Backend, TransactionEngine};

/// Contract test backend. It owns no scheduling logic and exercises the same
/// transaction engine used by real adapters.
#[derive(Clone, Debug, Default)]
pub struct FakeBackend {
    engine: TransactionEngine,
}

impl Backend for FakeBackend {
    fn engine(&self) -> &TransactionEngine {
        &self.engine
    }

    fn engine_mut(&mut self) -> &mut TransactionEngine {
        &mut self.engine
    }
}
