//! Windows terminal backend.
//!
//! Layering (top-down):
//!
//! - [`backend::WindowsGhosttyApp`] / [`backend::WindowsGhosttyTerminal`] —
//!   public types that mirror the macOS surface and slot into
//!   `crate::stub` exports for cross-crate type parity.
//! - [`host_view::RenderSession`] — owns the `Renderer`, VT parser, and
//!   ConPTY for a single pane. No child HWND: the renderer draws into
//!   an offscreen D3D11 texture and `render_frame` returns BGRA bytes
//!   the GPUI view paints as an `ImageSource::Render(Arc<RenderImage>)`.
//! - [`render`] — D3D11 + DirectWrite renderer. Builds a glyph atlas,
//!   draws the cell grid into an offscreen texture, and CPU-reads the
//!   backbuffer into BGRA for the caller.
//! - [`vt`] — `libghostty-vt` FFI. Parses bytes from the PTY into a
//!   screen state machine; we read cells out of it for the renderer.
//! - [`conpty`] — `CreatePseudoConsole` + child shell process; pipes
//!   bytes between the shell and the VT parser.
//!
//! Architectural plan: `docs/impl/windows-port.md`.

pub mod backend;
pub mod clipboard;
pub mod conpty;
pub mod host_view;
mod profile;
pub mod render;
pub mod vt;

pub use backend::{WindowsGhosttyApp, WindowsGhosttyTerminal};
