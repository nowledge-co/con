// Suppress warnings from objc 0.2's `sel_impl!` and `class!` macros.
#![allow(unexpected_cfgs)]

pub mod ffi;
pub mod terminal;

pub use terminal::{
    CommandFinishedSignal, CommandRecord, GhosttyApp, GhosttyTerminal, MouseButton, SurfaceSize,
    TerminalColors, TerminalState,
};
