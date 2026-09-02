//! Disposable wasm-emitting JIT feasibility spike.

pub mod emitter;

pub use emitter::{emit, price_table_cost, CompileError, EmittedModule, REGISTER_COUNT};
