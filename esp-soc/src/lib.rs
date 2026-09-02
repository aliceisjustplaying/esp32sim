//! The machine around a chip, written once: two `Soc` implementations (ESP32-S3, ESP32-C3) plug
//! their cores, memory map, peripherals and interrupt controller into `Machine<S>`, which owns
//! the scheduler, device time, console, action scripts, the web UI protocol, real-time pacing,
//! the image loaders and the board model.
pub mod board;
pub mod elf;
pub mod host;
pub mod image;
pub mod machine;
pub mod picture;
pub mod png;
pub mod soc;
pub mod web;

pub use board::{Board, BoardModel, NoBoard};
pub use machine::{Console, Debug, Machine, Realtime, RegTrace, Script, ScriptAction};
pub use soc::{CoreState, Soc, SocBus, Stop};
