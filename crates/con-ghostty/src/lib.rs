//! Rust wrapper around libghostty's embedded C API.
//!
//! Backend selection per target:
//!
//! | target | backend | source |
//! |---|---|---|
//! | macOS | full libghostty (Metal + AppKit NSView) | `terminal.rs` + `ffi.rs` |
//! | Windows | libghostty-vt + ConPTY + D3D11 + DirectWrite, hosted in a `WS_CHILD` HWND | `windows/` |
//! | Linux | local backend scaffold (Unix PTY + future GPUI-owned renderer) | `linux/` |
//! | other | no-op stub (UI compiles, terminal pane shows placeholder) | `stub.rs` |
//!
//! All backends expose the same public type names — `GhosttyApp`,
//! `GhosttyTerminal`, `TerminalColors`, etc. — so cross-platform UI
//! code in `con-app` consumes them without per-callsite cfg gates.

// Suppress warnings from objc 0.2's `sel_impl!` and `class!` macros.
#![allow(unexpected_cfgs)]

#[cfg(target_os = "macos")]
pub mod ffi;
#[cfg(target_os = "macos")]
pub mod terminal;

// `stub` defines the shared shape (TerminalColors, GhosttySplitDirection,
// MouseButton, etc.) — also used by the Windows backend's facade as the
// concrete type of cross-cutting values. On macOS the stub module isn't
// compiled because all types come from `terminal.rs`.
#[cfg(not(target_os = "macos"))]
pub mod stub;

#[cfg(target_os = "linux")]
pub mod linux;

#[cfg(target_os = "windows")]
pub mod windows;

// ── Re-exports per platform ────────────────────────────────────────────

#[cfg(target_os = "macos")]
pub use terminal::{
    CommandFinishedSignal, CommandRecord, GhosttyApp, GhosttyConfigPatch, GhosttySplitDirection,
    GhosttyScrollbar, GhosttySurfaceEvent, GhosttyTerminal, MouseButton, SurfaceSize,
    TerminalColors, TerminalState,
};

#[cfg(target_os = "windows")]
pub use stub::{
    CommandFinishedSignal, CommandRecord, GhosttyConfigPatch, GhosttySplitDirection,
    GhosttySurfaceEvent, MouseButton, SurfaceSize, TerminalColors,
};
#[cfg(target_os = "windows")]
pub use windows::{WindowsGhosttyApp as GhosttyApp, WindowsGhosttyTerminal as GhosttyTerminal};

#[cfg(target_os = "linux")]
pub use linux::{LinuxGhosttyApp as GhosttyApp, LinuxGhosttyTerminal as GhosttyTerminal};
#[cfg(target_os = "linux")]
pub use stub::{
    CommandFinishedSignal, CommandRecord, GhosttyConfigPatch, GhosttySplitDirection,
    GhosttySurfaceEvent, MouseButton, SurfaceSize, TerminalColors,
};

#[cfg(all(
    not(target_os = "macos"),
    not(target_os = "windows"),
    not(target_os = "linux")
))]
pub use stub::{
    CommandFinishedSignal, CommandRecord, GhosttyApp, GhosttyConfigPatch, GhosttySplitDirection,
    GhosttySurfaceEvent, GhosttyTerminal, MouseButton, SurfaceSize, TerminalColors,
};
