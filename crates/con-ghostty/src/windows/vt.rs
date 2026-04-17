//! `libghostty-vt` FFI bindings — the cross-platform VT parser carved
//! out of Ghostty (PR ghostty-org/ghostty#8840 + follow-up commits).
//!
//! Upstream public surface lives at `include/ghostty/vt.h`
//! (umbrella) plus per-module headers under `include/ghostty/vt/`.
//! Symbol prefix is `ghostty_*`. The C API has hand-written headers
//! (not bindgen-generated upstream) and follows a typed get/set pattern
//! (`ghostty_terminal_get(term, KEY, &out)`) instead of one accessor
//! per field. We mirror just the slice we need.
//!
//! Key API characteristics:
//!
//! - The terminal is **not** thread-safe. We wrap it in a `Mutex` and
//!   serialize all FFI calls through that.
//! - `ghostty_terminal_vt_write` is non-reentrant — callbacks fired
//!   during it must not call `vt_write` back on the same terminal.
//! - Render state is a separate opaque `GhosttyRenderState` updated
//!   from the terminal under the embedder's lock.
//!
//! libghostty-vt is built from the pinned Ghostty source by `build.rs`
//! via `zig build` and statically linked. `GHOSTTY_STATIC` is defined
//! at compile time so the `GHOSTTY_API` visibility macro is a no-op.

#![allow(non_camel_case_types, dead_code)]

use std::os::raw::{c_int, c_void};
use std::sync::Arc;

use parking_lot::Mutex;

// ── Raw FFI ────────────────────────────────────────────────────────────

/// Opaque terminal handle (`GhosttyTerminal` in C — typedef'd to a
/// `GhosttyTerminalImpl*`).
pub type GhosttyTerminal = *mut c_void;

/// Opaque render state (`GhosttyRenderState`).
pub type GhosttyRenderState = *mut c_void;

/// Opaque row iterator.
pub type GhosttyRowIterator = *mut c_void;

/// Opaque per-row cells iterator.
pub type GhosttyRowCells = *mut c_void;

/// `GhosttyResult` — 0 on success, non-zero error codes otherwise. We
/// translate to `Result<(), c_int>` at the call site.
pub type GhosttyResult = c_int;

/// Constructor options for a terminal.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct GhosttyTerminalOptions {
    pub cols: u16,
    pub rows: u16,
    pub max_scrollback: usize,
}

/// Typed-get keys (`GHOSTTY_TERMINAL_DATA_*`). We expose only the few
/// we read from Rust; the upstream enum has dozens.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub enum GhosttyTerminalData {
    Cols = 0,
    Rows = 1,
    CursorX = 2,
    CursorY = 3,
    CursorVisible = 4,
    Title = 20,
    Pwd = 21,
    // Upstream defines many more. Keep this list synced when adding a
    // new accessor — the numeric values are stable per the upstream
    // ABI introspection helper `ghostty_type_json()`.
}

/// Typed-set option keys (`GHOSTTY_TERMINAL_OPT_*`). Only the ones we
/// register callbacks for.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub enum GhosttyTerminalOpt {
    Userdata = 0,
    WritePty = 1,
    Bell = 2,
    TitleChanged = 3,
    // ...
}

/// `GhosttyAllocator` — NULL means "use the upstream default
/// allocator". We always pass NULL.
pub type GhosttyAllocator = c_void;

unsafe extern "C" {
    pub fn ghostty_terminal_new(
        allocator: *const GhosttyAllocator,
        out_terminal: *mut GhosttyTerminal,
        options: GhosttyTerminalOptions,
    ) -> GhosttyResult;

    pub fn ghostty_terminal_free(terminal: GhosttyTerminal);

    pub fn ghostty_terminal_reset(terminal: GhosttyTerminal);

    pub fn ghostty_terminal_resize(
        terminal: GhosttyTerminal,
        cols: u16,
        rows: u16,
        cell_width_px: u32,
        cell_height_px: u32,
    ) -> GhosttyResult;

    pub fn ghostty_terminal_vt_write(terminal: GhosttyTerminal, data: *const u8, len: usize);

    pub fn ghostty_terminal_get(
        terminal: GhosttyTerminal,
        key: GhosttyTerminalData,
        out: *mut c_void,
    ) -> GhosttyResult;

    pub fn ghostty_terminal_set(
        terminal: GhosttyTerminal,
        key: GhosttyTerminalOpt,
        value: *const c_void,
    ) -> GhosttyResult;
}

// ── Safe wrapper ───────────────────────────────────────────────────────

/// Snapshot the renderer reads each frame. We own the data — no
/// borrowed pointers across the FFI boundary, so the parser lock can
/// be released between feeds and renders.
#[derive(Debug, Clone, Default)]
pub struct ScreenSnapshot {
    pub cols: u16,
    pub rows: u16,
    /// Row-major cell array; cells.len() == cols*rows when populated.
    /// Empty until the renderer-state extraction PR lands; the host
    /// view can still display the shell as a clear-color rect with
    /// title until then.
    pub cells: Vec<Cell>,
    pub cursor: Cursor,
    pub title: Option<String>,
    /// Monotonic; renderer skips frame if it matches last seen.
    pub generation: u64,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct Cell {
    pub codepoint: u32,
    pub fg: [u8; 3],
    pub bg: [u8; 3],
    /// 1=bold, 2=italic, 4=underline, 8=strike, 16=reverse.
    pub attrs: u8,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct Cursor {
    pub col: u16,
    pub row: u16,
    pub visible: bool,
}

/// Thread-safe handle to a `ghostty_terminal_t`. Internally serialized
/// (libghostty-vt itself is single-threaded — see upstream `terminal.h`).
pub struct VtScreen {
    inner: Arc<Mutex<VtInner>>,
}

struct VtInner {
    handle: GhosttyTerminal,
    cols: u16,
    rows: u16,
    generation: u64,
}

unsafe impl Send for VtInner {}

impl VtScreen {
    pub fn new(cols: u16, rows: u16) -> anyhow::Result<Self> {
        let mut handle: GhosttyTerminal = std::ptr::null_mut();
        let options = GhosttyTerminalOptions {
            cols,
            rows,
            max_scrollback: 10_000,
        };
        // SAFETY: out parameter; allocator NULL = default.
        let rc = unsafe { ghostty_terminal_new(std::ptr::null(), &mut handle, options) };
        if rc != 0 || handle.is_null() {
            anyhow::bail!("ghostty_terminal_new failed: rc={}", rc);
        }
        Ok(Self {
            inner: Arc::new(Mutex::new(VtInner {
                handle,
                cols,
                rows,
                generation: 0,
            })),
        })
    }

    /// Feed PTY bytes into the parser. Non-reentrant per upstream — do
    /// not call from inside a registered callback on the same terminal.
    pub fn feed(&self, bytes: &[u8]) {
        let mut inner = self.inner.lock();
        // SAFETY: handle valid for the lifetime of `inner`; bytes valid
        // for the call duration.
        unsafe { ghostty_terminal_vt_write(inner.handle, bytes.as_ptr(), bytes.len()) };
        inner.generation = inner.generation.wrapping_add(1);
    }

    pub fn resize(
        &self,
        cols: u16,
        rows: u16,
        cell_width_px: u32,
        cell_height_px: u32,
    ) -> anyhow::Result<()> {
        let mut inner = self.inner.lock();
        // SAFETY: handle valid; resize is a state mutation.
        let rc = unsafe {
            ghostty_terminal_resize(inner.handle, cols, rows, cell_width_px, cell_height_px)
        };
        if rc != 0 {
            anyhow::bail!("ghostty_terminal_resize failed: rc={}", rc);
        }
        inner.cols = cols;
        inner.rows = rows;
        inner.generation = inner.generation.wrapping_add(1);
        Ok(())
    }

    /// Snapshot the renderable state. Cell extraction iterates upstream's
    /// render-state row/cells iterators — staged for the focused glyph
    /// renderer PR to keep this PR's surface contained.
    pub fn snapshot(&self) -> ScreenSnapshot {
        let inner = self.inner.lock();

        let mut cursor = Cursor::default();
        let mut visible: u8 = 0;
        let mut col_u16: u16 = 0;
        let mut row_u16: u16 = 0;
        // SAFETY: out parameters of correct types per terminal.h get table.
        unsafe {
            let _ = ghostty_terminal_get(
                inner.handle,
                GhosttyTerminalData::CursorX,
                &mut col_u16 as *mut _ as *mut c_void,
            );
            let _ = ghostty_terminal_get(
                inner.handle,
                GhosttyTerminalData::CursorY,
                &mut row_u16 as *mut _ as *mut c_void,
            );
            let _ = ghostty_terminal_get(
                inner.handle,
                GhosttyTerminalData::CursorVisible,
                &mut visible as *mut _ as *mut c_void,
            );
        }
        cursor.col = col_u16;
        cursor.row = row_u16;
        cursor.visible = visible != 0;

        // Title is exposed as a borrowed `GhosttyString { data, len }`
        // by upstream. The exact struct shape is in
        // `include/ghostty/vt/types.h`. Until the renderer needs it
        // we leave it as None — the GPUI parent reads pane title via a
        // separate path.
        let title = None;

        ScreenSnapshot {
            cols: inner.cols,
            rows: inner.rows,
            cells: Vec::new(),
            cursor,
            title,
            generation: inner.generation,
        }
    }

    pub fn size(&self) -> (u16, u16) {
        let inner = self.inner.lock();
        (inner.cols, inner.rows)
    }
}

impl Drop for VtScreen {
    fn drop(&mut self) {
        if let Some(mutex) = Arc::get_mut(&mut self.inner) {
            let inner = mutex.get_mut();
            if !inner.handle.is_null() {
                // SAFETY: unique owner via Arc::get_mut check.
                unsafe { ghostty_terminal_free(inner.handle) };
                inner.handle = std::ptr::null_mut();
            }
        }
    }
}
