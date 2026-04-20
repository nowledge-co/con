//! Windows terminal backend.
//!
//! Layering (top-down):
//!
//! - [`backend::WindowsGhosttyApp`] / [`backend::WindowsGhosttyTerminal`] —
//!   public types that mirror the macOS surface and slot into
//!   `crate::stub` exports for cross-crate type parity.
//! - [`host_view`] — owns a child `WS_CHILD` `HWND` parented to GPUI's
//!   window, forwards `WM_*` messages, owns the D3D11 swapchain.
//! - [`render`] — D3D11 + DirectWrite renderer. Builds a glyph atlas,
//!   draws the cell grid, presents to the swapchain.
//! - [`vt`] — `libghostty-vt` FFI. Parses bytes from the PTY into a
//!   screen state machine; we read cells out of it for the renderer.
//! - [`conpty`] — `CreatePseudoConsole` + child shell process; pipes
//!   bytes between the shell and the VT parser.
//!
//! Architectural plan: `docs/impl/windows-port.md`.
//! Compositor strategy: WS_CHILD now (bring-up); upstream a
//! `WindowsWindow::attach_external_swap_chain_handle` GPUI PR in
//! parallel and swap to a `DCompositionCreateSurfaceHandle`-backed
//! swapchain when it merges. Either path keeps the VT/renderer/ConPTY
//! code identical — only the swapchain target changes.

pub mod backend;
pub mod clipboard;
pub mod conpty;
pub mod host_view;
pub mod render;
pub mod vt;

pub use backend::{WindowsGhosttyApp, WindowsGhosttyTerminal};
