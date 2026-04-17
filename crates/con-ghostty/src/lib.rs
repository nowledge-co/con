//! Rust wrapper around libghostty's embedded C API.
//!
//! libghostty is macOS-only upstream as of April 2026 — see
//! `docs/impl/windows-port.md` for the Windows porting plan. On non-macOS
//! targets this crate compiles to an empty shell so the workspace
//! resolves cleanly; the symbols below are simply unavailable.

// Suppress warnings from objc 0.2's `sel_impl!` and `class!` macros.
#![allow(unexpected_cfgs)]

#[cfg(target_os = "macos")]
pub mod ffi;
#[cfg(target_os = "macos")]
pub mod terminal;

#[cfg(target_os = "macos")]
pub use terminal::{
    CommandFinishedSignal, CommandRecord, GhosttyApp, GhosttySplitDirection, GhosttySurfaceEvent,
    GhosttyTerminal, MouseButton, SurfaceSize, TerminalColors, TerminalState,
};
