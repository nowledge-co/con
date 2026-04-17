//! Rust wrapper around libghostty's embedded C API.
//!
//! On macOS this crate links the upstream libghostty static library and
//! exposes a thin wrapper over its public C API. On other targets the
//! types live in `stub.rs` as placeholder implementations — the public
//! type names are identical so cross-platform UI code compiles without
//! per-call-site `cfg` gates. A real Windows/Linux backend will replace
//! the stub with a working libghostty-vt + ConPTY (or forkpty) impl —
//! see `docs/impl/windows-port.md`.

// Suppress warnings from objc 0.2's `sel_impl!` and `class!` macros.
#![allow(unexpected_cfgs)]

#[cfg(target_os = "macos")]
pub mod ffi;
#[cfg(target_os = "macos")]
pub mod terminal;

#[cfg(not(target_os = "macos"))]
mod stub;

#[cfg(target_os = "macos")]
pub use terminal::{
    CommandFinishedSignal, CommandRecord, GhosttyApp, GhosttyConfigPatch, GhosttySplitDirection,
    GhosttySurfaceEvent, GhosttyTerminal, MouseButton, SurfaceSize, TerminalColors, TerminalState,
};

#[cfg(not(target_os = "macos"))]
pub use stub::{
    CommandFinishedSignal, CommandRecord, GhosttyApp, GhosttyConfigPatch, GhosttySplitDirection,
    GhosttySurfaceEvent, GhosttyTerminal, MouseButton, SurfaceSize, TerminalColors,
};
