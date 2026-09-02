//! Disposable wasm-emitting JIT feasibility spike.

pub mod emitter;

pub use emitter::{
    emit, emit_with_options, price_table_cost, CompileError, EmitOptions, EmittedModule,
    REGISTER_COUNT,
};
