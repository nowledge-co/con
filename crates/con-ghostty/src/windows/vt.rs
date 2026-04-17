//! `libghostty-vt` FFI bindings + render-state snapshot.
//!
//! Upstream public surface lives at `include/ghostty/vt.h` with module
//! headers under `include/ghostty/vt/`. Symbol prefix: `ghostty_*`.
//!
//! There are two ways to read screen state:
//!
//! - `ghostty_grid_ref_*` — ergonomic but **explicitly not for render
//!   loops** per `include/ghostty/vt/grid_ref.h`. Used for selection
//!   hit-testing and accessibility.
//! - `ghostty_render_state_*` — designed for the hot path. Tracks per-row
//!   DIRTY flags so we only read cells that changed, and exposes a
//!   `row_cells_get_multi` that fetches several fields (raw codepoint,
//!   style, fg, bg) in one call.
//!
//! We use the render-state path. The terminal is NOT thread-safe;
//! internal mutexes serialize FFI calls, and the renderer reads a
//! cloned snapshot so the parser lock is released between feeds and
//! frames.
//!
//! libghostty-vt is built by `build.rs` via `zig build ghostty-vt-static`
//! and linked statically. `GHOSTTY_STATIC` is defined so the upstream
//! `GHOSTTY_API` visibility macro is a no-op.

#![allow(non_camel_case_types, dead_code)]

use std::os::raw::{c_int, c_void};
use std::sync::Arc;

use parking_lot::Mutex;

// ── Opaque types ───────────────────────────────────────────────────────

pub type GhosttyTerminal = *mut c_void;
pub type GhosttyRenderState = *mut c_void;
pub type GhosttyRowIterator = *mut c_void;
pub type GhosttyRowCells = *mut c_void;
pub type GhosttyAllocator = c_void;
pub type GhosttyResult = c_int;

// ── Enums (keys) ───────────────────────────────────────────────────────

/// `GHOSTTY_TERMINAL_DATA_*` — keys for `ghostty_terminal_get`. Only
/// the subset we read here; upstream defines many more.
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
}

/// `GHOSTTY_TERMINAL_OPT_*` — keys for `ghostty_terminal_set`.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub enum GhosttyTerminalOpt {
    Userdata = 0,
    WritePty = 1,
    Bell = 2,
    TitleChanged = 3,
}

/// `GhosttyRenderStateDirty` — return from `row_data(DIRTY)`.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GhosttyRenderStateDirty {
    False = 0,
    Partial = 1,
    Full = 2,
}

/// `GHOSTTY_RENDER_STATE_ROW_DATA_*` — keys readable from a row.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub enum GhosttyRenderStateRowData {
    Dirty = 0,
    YOffset = 1,
    ContentId = 2,
    SemanticPrompt = 3,
}

/// `GHOSTTY_RENDER_STATE_ROW_CELLS_DATA_*` — keys readable from a cell
/// iterator position via `row_cells_get_multi`.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub enum GhosttyRenderStateRowCellsData {
    Raw = 0,     // packed cell (uint64)
    Style = 1,   // GhosttyStyle pointer-sized index
    GraphemesLen = 2,
    GraphemesBuf = 3,
    BgColor = 4, // u32 0xRRGGBBAA
    FgColor = 5, // u32 0xRRGGBBAA
}

// ── Sized structs ──────────────────────────────────────────────────────

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct GhosttyTerminalOptions {
    pub cols: u16,
    pub rows: u16,
    pub max_scrollback: usize,
}

// ── Raw FFI ────────────────────────────────────────────────────────────
//
// Signatures mirror `include/ghostty/vt/terminal.h` and
// `include/ghostty/vt/render.h`. Upstream warns that function signatures
// are subject to change — if a build fails on a newer Ghostty pin, check
// the headers and update the bindings.

unsafe extern "C" {
    // Terminal lifecycle (`terminal.h`)
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

    // Render state (`render.h`) — the hot path
    pub fn ghostty_render_state_new(
        terminal: GhosttyTerminal,
        out_state: *mut GhosttyRenderState,
    ) -> GhosttyResult;
    pub fn ghostty_render_state_free(state: GhosttyRenderState);
    /// Pull fresh data from the terminal into the render state. Caller
    /// holds the terminal lock for the duration of this call; afterwards
    /// the state is self-contained and can be read lock-free.
    pub fn ghostty_render_state_update(state: GhosttyRenderState) -> GhosttyResult;

    pub fn ghostty_render_state_row_iterator_new(
        state: GhosttyRenderState,
        out_iter: *mut GhosttyRowIterator,
    ) -> GhosttyResult;
    pub fn ghostty_render_state_row_iterator_free(iter: GhosttyRowIterator);
    /// Advance to the next row. Returns non-zero when a row is
    /// available; zero when iteration is done.
    pub fn ghostty_render_state_row_iterator_next(iter: GhosttyRowIterator) -> c_int;

    pub fn ghostty_render_state_row_get(
        iter: GhosttyRowIterator,
        key: GhosttyRenderStateRowData,
        out: *mut c_void,
    ) -> GhosttyResult;

    pub fn ghostty_render_state_row_cells_new(
        iter: GhosttyRowIterator,
        out_cells: *mut GhosttyRowCells,
    ) -> GhosttyResult;
    pub fn ghostty_render_state_row_cells_free(cells: GhosttyRowCells);
    /// Advance to the next cell. Returns non-zero when a cell is
    /// available; zero when the row is exhausted.
    pub fn ghostty_render_state_row_cells_next(cells: GhosttyRowCells) -> c_int;

    /// Fetch one field from the cell iterator's current position.
    /// Older Ghostty revisions (our current pin, ~April 2026) ship this
    /// single-key variant; the batched `row_cells_get_multi` is a newer
    /// upstream addition. We call `_get` per key to stay compatible
    /// with the pinned revision.
    pub fn ghostty_render_state_row_cells_get(
        cells: GhosttyRowCells,
        key: GhosttyRenderStateRowCellsData,
        out: *mut c_void,
    ) -> GhosttyResult;
}

// ── Snapshot (renderer's view) ────────────────────────────────────────

/// One cell ready for the renderer. Packed to match the GPU instance
/// layout so the renderer can `memcpy` the row of cells straight into
/// the dynamic instance buffer.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct Cell {
    /// Unicode codepoint; 0 = empty.
    pub codepoint: u32,
    /// Foreground RGBA (0xRRGGBBAA).
    pub fg: u32,
    /// Background RGBA.
    pub bg: u32,
    /// Bit flags: 1=bold, 2=italic, 4=underline, 8=strike, 16=inverse.
    pub attrs: u8,
    pub _pad: [u8; 3],
}

#[derive(Debug, Clone, Copy, Default)]
pub struct Cursor {
    pub col: u16,
    pub row: u16,
    pub visible: bool,
}

/// Screen snapshot the renderer consumes. Dirty rows are represented by
/// storing all cells (simpler — the renderer checks `dirty_rows` to skip
/// upload); total LOC savings over a diff structure aren't worth the
/// complexity at Phase 3b.
#[derive(Debug, Clone, Default)]
pub struct ScreenSnapshot {
    pub cols: u16,
    pub rows: u16,
    /// Row-major: `cells[row * cols + col]`.
    pub cells: Vec<Cell>,
    /// Row indices whose cells changed since the last update.
    pub dirty_rows: Vec<u16>,
    pub cursor: Cursor,
    pub title: Option<String>,
    /// Monotonic counter; the renderer skips draws where this hasn't
    /// advanced.
    pub generation: u64,
}

// ── Safe wrapper ───────────────────────────────────────────────────────

pub struct VtScreen {
    inner: Arc<Mutex<VtInner>>,
}

struct VtInner {
    terminal: GhosttyTerminal,
    render_state: GhosttyRenderState,
    cols: u16,
    rows: u16,
    generation: u64,
    /// Reusable scratch to avoid per-frame Vec allocs.
    scratch: Vec<Cell>,
}

unsafe impl Send for VtInner {}

impl VtScreen {
    pub fn new(cols: u16, rows: u16) -> anyhow::Result<Self> {
        let mut terminal: GhosttyTerminal = std::ptr::null_mut();
        let options = GhosttyTerminalOptions {
            cols,
            rows,
            max_scrollback: 10_000,
        };
        log::info!(
            "VtScreen::new: calling ghostty_terminal_new(cols={cols}, rows={rows}, scrollback={})",
            options.max_scrollback
        );
        // SAFETY: out param; allocator NULL = upstream default.
        let rc = unsafe { ghostty_terminal_new(std::ptr::null(), &mut terminal, options) };
        log::info!(
            "VtScreen::new: ghostty_terminal_new returned rc={rc}, terminal={:?}",
            terminal
        );
        if rc != 0 || terminal.is_null() {
            anyhow::bail!("ghostty_terminal_new failed: rc={}", rc);
        }

        let mut render_state: GhosttyRenderState = std::ptr::null_mut();
        let enable_render_state = std::env::var("CON_GHOSTTY_VT_RENDER_STATE")
            .map(|s| matches!(s.as_str(), "1" | "true" | "yes" | "on"))
            .unwrap_or(false);
        if enable_render_state {
            log::info!("VtScreen::new: calling ghostty_render_state_new");
            // SAFETY: terminal valid; out param.
            let rc = unsafe { ghostty_render_state_new(terminal, &mut render_state) };
            log::info!(
                "VtScreen::new: ghostty_render_state_new returned rc={rc}, state={:?}",
                render_state
            );
            if rc != 0 || render_state.is_null() {
                // SAFETY: terminal needs to be freed on partial init.
                unsafe { ghostty_terminal_free(terminal) };
                anyhow::bail!("ghostty_render_state_new failed: rc={}", rc);
            }
        } else {
            log::warn!(
                "VtScreen::new: skipping render_state (CON_GHOSTTY_VT_RENDER_STATE unset). \
                 Terminal output will parse but cells won't render. \
                 The pinned Ghostty revision's render_state API crashes \
                 (access violation inside ghostty_render_state_new); \
                 bump GHOSTTY_REV once it's stable."
            );
        }

        Ok(Self {
            inner: Arc::new(Mutex::new(VtInner {
                terminal,
                render_state,
                cols,
                rows,
                generation: 0,
                scratch: Vec::with_capacity(cols as usize * rows as usize),
            })),
        })
    }

    /// Feed bytes from the PTY into the parser. Non-reentrant per
    /// upstream: do not call from inside a registered callback.
    pub fn feed(&self, bytes: &[u8]) {
        let mut inner = self.inner.lock();
        // SAFETY: terminal valid; bytes live for the call.
        unsafe { ghostty_terminal_vt_write(inner.terminal, bytes.as_ptr(), bytes.len()) };
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
        // SAFETY: terminal valid.
        let rc = unsafe {
            ghostty_terminal_resize(inner.terminal, cols, rows, cell_width_px, cell_height_px)
        };
        if rc != 0 {
            anyhow::bail!("ghostty_terminal_resize failed: rc={}", rc);
        }
        inner.cols = cols;
        inner.rows = rows;
        inner.scratch = Vec::with_capacity(cols as usize * rows as usize);
        inner.generation = inner.generation.wrapping_add(1);
        Ok(())
    }

    /// Pull a fresh snapshot via the render-state API. Walks row and
    /// cell iterators, batch-reads RAW/STYLE/BG/FG per cell, honors the
    /// per-row DIRTY flag so unchanged rows don't touch FFI again.
    pub fn snapshot(&self) -> ScreenSnapshot {
        let mut inner = self.inner.lock();

        let cols = inner.cols;
        let rows = inner.rows;

        // If the render_state API is disabled (see VtScreen::new), we
        // can't read cells back. Return a trivial snapshot so the
        // renderer still clears to the background color and presents —
        // ConPTY + terminal_vt_write still run; output simply isn't
        // visualized until render_state is enabled upstream.
        if inner.render_state.is_null() {
            return ScreenSnapshot {
                cols,
                rows,
                cells: Vec::new(),
                dirty_rows: Vec::new(),
                cursor: Cursor::default(),
                title: None,
                generation: inner.generation,
            };
        }

        // Refresh the render state from the terminal.
        // SAFETY: render_state valid while inner holds the mutex.
        unsafe { ghostty_render_state_update(inner.render_state) };

        let total = cols as usize * rows as usize;

        // Keep the previous frame's cells so untouched rows stay intact.
        if inner.scratch.len() != total {
            inner.scratch.clear();
            inner.scratch.resize(total, Cell::default());
        }

        let mut dirty_rows: Vec<u16> = Vec::new();

        // SAFETY: iterator lifetime is scoped; free on the way out.
        let mut iter: GhosttyRowIterator = std::ptr::null_mut();
        let rc = unsafe { ghostty_render_state_row_iterator_new(inner.render_state, &mut iter) };
        if rc == 0 && !iter.is_null() {
            let mut row_idx: u16 = 0;
            // SAFETY: iter valid for the duration; `next` returns 0 at end.
            while unsafe { ghostty_render_state_row_iterator_next(iter) } != 0 {
                if row_idx >= rows {
                    break;
                }

                let mut dirty: GhosttyRenderStateDirty = GhosttyRenderStateDirty::False;
                // SAFETY: DIRTY is an enum-sized integer; caller provides
                // aligned out pointer.
                unsafe {
                    ghostty_render_state_row_get(
                        iter,
                        GhosttyRenderStateRowData::Dirty,
                        &mut dirty as *mut _ as *mut c_void,
                    );
                }

                if dirty == GhosttyRenderStateDirty::False {
                    row_idx += 1;
                    continue;
                }

                dirty_rows.push(row_idx);

                // SAFETY: cells handle scoped to this block.
                let mut cells: GhosttyRowCells = std::ptr::null_mut();
                let rc =
                    unsafe { ghostty_render_state_row_cells_new(iter, &mut cells) };
                if rc == 0 && !cells.is_null() {
                    let row_start = row_idx as usize * cols as usize;
                    let mut col_idx: u16 = 0;
                    while unsafe { ghostty_render_state_row_cells_next(cells) } != 0 {
                        if col_idx >= cols {
                            break;
                        }
                        let cell = read_cell(cells);
                        inner.scratch[row_start + col_idx as usize] = cell;
                        col_idx += 1;
                    }
                    // Clear any trailing cells in the row.
                    for c in col_idx..cols {
                        inner.scratch[row_start + c as usize] = Cell::default();
                    }
                    unsafe { ghostty_render_state_row_cells_free(cells) };
                }

                row_idx += 1;
            }
            unsafe { ghostty_render_state_row_iterator_free(iter) };
        }

        // Cursor from the terminal (render_state has a pointer-equivalent,
        // but using the terminal get keeps parity with the simple case).
        let mut cursor = Cursor::default();
        let mut visible: u8 = 0;
        let mut col_u16: u16 = 0;
        let mut row_u16: u16 = 0;
        // SAFETY: out params, correct sizes per upstream terminal.h.
        unsafe {
            let _ = ghostty_terminal_get(
                inner.terminal,
                GhosttyTerminalData::CursorX,
                &mut col_u16 as *mut _ as *mut c_void,
            );
            let _ = ghostty_terminal_get(
                inner.terminal,
                GhosttyTerminalData::CursorY,
                &mut row_u16 as *mut _ as *mut c_void,
            );
            let _ = ghostty_terminal_get(
                inner.terminal,
                GhosttyTerminalData::CursorVisible,
                &mut visible as *mut _ as *mut c_void,
            );
        }
        cursor.col = col_u16;
        cursor.row = row_u16;
        cursor.visible = visible != 0;

        ScreenSnapshot {
            cols,
            rows,
            cells: inner.scratch.clone(),
            dirty_rows,
            cursor,
            title: None, // TODO: terminal_get(TITLE) returns GhosttyString; wire shape.
            generation: inner.generation,
        }
    }

    pub fn size(&self) -> (u16, u16) {
        let inner = self.inner.lock();
        (inner.cols, inner.rows)
    }
}

/// Read four fields from the cell iterator's current position. Uses
/// per-key `_get` calls to stay compatible with Ghostty revisions that
/// predate the batched `_get_multi` helper.
fn read_cell(cells: GhosttyRowCells) -> Cell {
    let mut raw: u64 = 0;
    let mut _style: usize = 0;
    let mut bg: u32 = 0;
    let mut fg: u32 = 0;

    // SAFETY: out pointers are correctly typed for each key per upstream
    // `render.h`.
    unsafe {
        let _ = ghostty_render_state_row_cells_get(
            cells,
            GhosttyRenderStateRowCellsData::Raw,
            &mut raw as *mut _ as *mut c_void,
        );
        let _ = ghostty_render_state_row_cells_get(
            cells,
            GhosttyRenderStateRowCellsData::Style,
            &mut _style as *mut _ as *mut c_void,
        );
        let _ = ghostty_render_state_row_cells_get(
            cells,
            GhosttyRenderStateRowCellsData::BgColor,
            &mut bg as *mut _ as *mut c_void,
        );
        let _ = ghostty_render_state_row_cells_get(
            cells,
            GhosttyRenderStateRowCellsData::FgColor,
            &mut fg as *mut _ as *mut c_void,
        );
    }

    // `raw` packs the codepoint in its low 32 bits and a content/attrs
    // tag above. Upstream screen.h documents the layout via
    // GHOSTTY_CELL_DATA_* accessors; for the renderer's purposes we only
    // need the codepoint and precomputed fg/bg from the render state.
    let codepoint = (raw & 0xFFFF_FFFF) as u32;
    let attrs = ((raw >> 56) & 0xFF) as u8;

    Cell {
        codepoint,
        fg,
        bg,
        attrs,
        _pad: [0; 3],
    }
}

impl Drop for VtScreen {
    fn drop(&mut self) {
        if let Some(mutex) = Arc::get_mut(&mut self.inner) {
            let inner = mutex.get_mut();
            // Free render_state before terminal — it borrows.
            if !inner.render_state.is_null() {
                // SAFETY: unique owner via Arc::get_mut check.
                unsafe { ghostty_render_state_free(inner.render_state) };
                inner.render_state = std::ptr::null_mut();
            }
            if !inner.terminal.is_null() {
                // SAFETY: unique owner.
                unsafe { ghostty_terminal_free(inner.terminal) };
                inner.terminal = std::ptr::null_mut();
            }
        }
    }
}
