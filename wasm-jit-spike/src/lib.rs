//! Disposable wasm-emitting JIT feasibility spike.

pub mod emitter;
mod system;

pub use emitter::{
    emit, emit_with_options, price_table_cost, CompileError, EmitOptions, EmittedModule,
    REGISTER_COUNT,
};
pub use system::{
    emit_host_memory, emit_sram, emit_windowed, HostMemoryClass, WindowFallback, WindowState,
    FALLBACK_OFFSET, PHYSICAL_AR_OFFSET, PHYSICAL_REGISTER_COUNT, POSTED_WRITES_OFFSET, PS_OFFSET,
    SRAM_IMAGE_OFFSET, WINDOWBASE_OFFSET, WINDOWSTART_OFFSET,
};
