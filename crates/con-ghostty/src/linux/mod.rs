//! Linux terminal backend.
//!
//! Unlike macOS, con cannot embed Ghostty's renderer directly on Linux
//! today. The long-term Linux lane therefore matches the Windows
//! strategy in one important way: con owns the backend integration
//! locally instead of waiting on an upstream embedding surface.
//!
//! The concrete Linux path is:
//!
//! - Unix PTY/session ownership in `pty.rs`
//! - Linux facade types in `backend.rs`
//! - GPUI-hosted terminal rendering in `con-app/src/linux_view.rs`
//!
//! The renderer itself is still pending, so the current Linux view
//! remains a placeholder card. But this module now owns real Linux-only
//! backend types and PTY lifecycle scaffolding instead of re-exporting
//! the generic non-macOS stub.

pub mod backend;
pub mod pty;

pub use backend::{LinuxGhosttyApp, LinuxGhosttyTerminal};
