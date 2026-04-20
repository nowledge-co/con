//! `libghostty-vt` FFI bindings + render-state snapshot.
//!
//! Rewritten to match the **actual** upstream API at GHOSTTY_REV
//! `ca7516bea60190ee2e9a4f9182b61d318d107c6e` — `include/ghostty/vt/*.h`.
//! Key lifecycle:
//!
//! 1. `terminal = ghostty_terminal_new(NULL_alloc, opts)`
//! 2. `state    = ghostty_render_state_new(NULL_alloc)`
//! 3. `iter     = ghostty_render_state_row_iterator_new(NULL_alloc)`
//! 4. `cells    = ghostty_render_state_row_cells_new(NULL_alloc)`
//!
//! Per-frame:
//!   - `ghostty_render_state_update(state, terminal)` to refresh
//!   - `ghostty_render_state_get(state, DATA_ROW_ITERATOR, &iter)` to bind iterator
//!   - while `row_iterator_next(iter)` is true:
//!       - `row_get(iter, DIRTY, &dirty)`, skip if false
//!       - `row_get(iter, CELLS, &cells)` to bind cells iterator to the current row
//!       - while `row_cells_next(cells)` is true:
//!           - `row_cells_get(cells, RAW|STYLE|BG|FG, &out)`
//!
//! All `_next` functions return `bool`. The `_get` family uses an enum
//! key and writes to a typed `void*` out; key→type contract is per
//! upstream header comments.
//!
//! libghostty-vt is NOT thread-safe; we serialize via a Mutex. The
//! renderer reads a cloned `ScreenSnapshot` so the parser lock is
//! released between feeds and frames.

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
//
// Integer values mirror `include/ghostty/vt/terminal.h` and `render.h`
// at the pinned revision. Keep in sync on GHOSTTY_REV bumps.

/// `GHOSTTY_TERMINAL_DATA_*` keys for `ghostty_terminal_get`.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub enum GhosttyTerminalData {
    Invalid = 0,
    Cols = 1,
    Rows = 2,
    CursorX = 3,
    CursorY = 4,
    CursorVisible = 7,
    Title = 12,
}

/// `GHOSTTY_RENDER_STATE_DATA_*` keys for `ghostty_render_state_get`.
/// `RowIterator` (4) binds an existing row-iterator handle to this state.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub enum GhosttyRenderStateData {
    Invalid = 0,
    Cols = 1,
    Rows = 2,
    Dirty = 3,
    RowIterator = 4,
    ColorBackground = 5,
    ColorForeground = 6,
    ColorCursor = 7,
    ColorCursorHasValue = 8,
    ColorPalette = 9,
    CursorVisualStyle = 10,
    CursorVisible = 11,
    CursorBlinking = 12,
    CursorPasswordInput = 13,
    CursorViewportHasValue = 14,
    CursorViewportX = 15,
    CursorViewportY = 16,
    CursorViewportWideTail = 17,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GhosttyRenderStateDirty {
    False = 0,
    Partial = 1,
    Full = 2,
}

/// `GHOSTTY_RENDER_STATE_ROW_DATA_*` keys for `ghostty_render_state_row_get`.
/// `Cells` (3) binds a cells iterator to the current row.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub enum GhosttyRenderStateRowData {
    Invalid = 0,
    Dirty = 1,
    Raw = 2,
    Cells = 3,
}

/// `GHOSTTY_RENDER_STATE_ROW_CELLS_DATA_*` keys.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub enum GhosttyRenderStateRowCellsData {
    Invalid = 0,
    Raw = 1,
    Style = 2,
    GraphemesLen = 3,
    GraphemesBuf = 4,
    BgColor = 5,
    FgColor = 6,
}

/// `GHOSTTY_CELL_DATA_*` keys for `ghostty_cell_get`. Integer values
/// per `include/ghostty/vt/screen.h` at the pinned revision — the RAW
/// we get from row_cells is an **opaque `GhosttyCell` u64 handle**, not
/// a packed codepoint. Reading the codepoint requires this accessor.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub enum GhosttyCellData {
    Invalid = 0,
    Codepoint = 1,
    ContentTag = 2,
    Wide = 3,
    HasText = 4,
    HasStyling = 5,
    StyleId = 6,
    HasHyperlink = 7,
    Protected = 8,
    SemanticContent = 9,
    ColorPalette = 10,
    ColorRgb = 11,
}

/// Opaque cell snapshot returned by `row_cells_get(RAW, ...)`.
/// `typedef uint64_t GhosttyCell;` upstream.
pub type GhosttyCell = u64;

/// Packed 16-bit terminal mode — see `include/ghostty/vt/modes.h`.
/// Bits 0–14 hold the numeric mode value; bit 15 is the ANSI flag
/// (1 = ANSI, 0 = DEC private). Constructed via [`ghostty_mode`].
pub type GhosttyMode = u16;

/// Pack a mode value + ANSI flag into a [`GhosttyMode`]. Mirrors the
/// inline `ghostty_mode_new` helper the C header ships.
#[inline]
pub const fn ghostty_mode(value: u16, ansi: bool) -> GhosttyMode {
    (value & 0x7FFF) | ((ansi as u16) << 15)
}

// Pre-packed DEC private modes we care about on Windows. Keep the
// numeric values synced with `modes.h`.
pub const MODE_NORMAL_MOUSE: GhosttyMode = ghostty_mode(1000, false);
pub const MODE_BUTTON_MOUSE: GhosttyMode = ghostty_mode(1002, false);
pub const MODE_ANY_MOUSE: GhosttyMode = ghostty_mode(1003, false);
pub const MODE_X10_MOUSE: GhosttyMode = ghostty_mode(9, false);
pub const MODE_SGR_MOUSE: GhosttyMode = ghostty_mode(1006, false);
pub const MODE_ALT_SCROLL: GhosttyMode = ghostty_mode(1007, false);
/// DEC private mode 2004 — bracketed paste. Apps that set this want
/// pasted text wrapped in `ESC[200~ … ESC[201~` so the line editor can
/// distinguish typed-from-pasted input (e.g. to bypass auto-indent).
pub const MODE_BRACKETED_PASTE: GhosttyMode = ghostty_mode(2004, false);
/// DEC private mode 1 — DECCKM (cursor key mode). When set, arrow
/// keys send `ESC O [ABCD]` (application mode) instead of the default
/// `ESC [ [ABCD]` (cursor mode). Interactive readers like readline
/// and vim set this to distinguish their keymap lookup.
pub const MODE_DECCKM: GhosttyMode = ghostty_mode(1, false);

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct GhosttyTerminalOptions {
    pub cols: u16,
    pub rows: u16,
    pub max_scrollback: usize,
}

/// `GhosttyColorRgb` — R,G,B bytes per upstream `color.h`.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct GhosttyColorRgb {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

// ── Style (`style.h`) ──────────────────────────────────────────────────
//
// `row_cells_get(STYLE, out)` writes a `GhosttyStyle` by value. Caller
// sets `.size = sizeof(GhosttyStyle)` first so the library knows how
// many bytes it may write (versioned-struct forward-compat pattern).

/// `GhosttyStyleColor` — tagged color (None | Palette | Rgb). The
/// union is laid out as `u64` here because we only care about the tag
/// for now; per-cell fg/bg also come in via the cheaper FG_COLOR /
/// BG_COLOR accessor on row_cells, so we don't need to decode the
/// union value at read-cell time.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct GhosttyStyleColor {
    pub tag: u32,
    pub _pad: u32,
    pub value: u64,
}

/// `GhosttyStyle` — SGR-derived attributes for the current cell.
/// Layout matches `include/ghostty/vt/style.h` at GHOSTTY_REV; the
/// `size` prefix lets upstream add trailing fields without breaking
/// older callers.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct GhosttyStyle {
    pub size: usize,
    pub fg_color: GhosttyStyleColor,
    pub bg_color: GhosttyStyleColor,
    pub underline_color: GhosttyStyleColor,
    pub bold: bool,
    pub italic: bool,
    pub faint: bool,
    pub blink: bool,
    pub inverse: bool,
    pub invisible: bool,
    pub strikethrough: bool,
    pub overline: bool,
    pub underline: c_int,
}

impl GhosttyStyle {
    fn new() -> Self {
        Self {
            size: std::mem::size_of::<GhosttyStyle>(),
            fg_color: GhosttyStyleColor::default(),
            bg_color: GhosttyStyleColor::default(),
            underline_color: GhosttyStyleColor::default(),
            bold: false,
            italic: false,
            faint: false,
            blink: false,
            inverse: false,
            invisible: false,
            strikethrough: false,
            overline: false,
            underline: 0,
        }
    }
}

// Attr bits packed into `Cell.attrs`. Kept in sync with the HLSL
// pixel shader's interpretation (bit 0 = bold, 1 = italic, 2 =
// underline, 3 = strike, 4 = inverse).
pub const ATTR_BOLD: u8 = 1 << 0;
pub const ATTR_ITALIC: u8 = 1 << 1;
pub const ATTR_UNDERLINE: u8 = 1 << 2;
pub const ATTR_STRIKE: u8 = 1 << 3;
pub const ATTR_INVERSE: u8 = 1 << 4;

// ── Raw FFI ────────────────────────────────────────────────────────────

unsafe extern "C" {
    // Terminal (`terminal.h`)
    pub fn ghostty_terminal_new(
        allocator: *const GhosttyAllocator,
        out_terminal: *mut GhosttyTerminal,
        options: GhosttyTerminalOptions,
    ) -> GhosttyResult;
    pub fn ghostty_terminal_free(terminal: GhosttyTerminal);
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

    /// Query whether a terminal mode is currently set. `out_value` is a
    /// `bool` (1 byte). Returns `GHOSTTY_SUCCESS` on success.
    pub fn ghostty_terminal_mode_get(
        terminal: GhosttyTerminal,
        mode: GhosttyMode,
        out_value: *mut bool,
    ) -> GhosttyResult;

    // Render state (`render.h`)
    pub fn ghostty_render_state_new(
        allocator: *const GhosttyAllocator,
        out_state: *mut GhosttyRenderState,
    ) -> GhosttyResult;
    pub fn ghostty_render_state_free(state: GhosttyRenderState);
    pub fn ghostty_render_state_update(
        state: GhosttyRenderState,
        terminal: GhosttyTerminal,
    ) -> GhosttyResult;
    pub fn ghostty_render_state_get(
        state: GhosttyRenderState,
        key: GhosttyRenderStateData,
        out: *mut c_void,
    ) -> GhosttyResult;

    pub fn ghostty_render_state_row_iterator_new(
        allocator: *const GhosttyAllocator,
        out_iter: *mut GhosttyRowIterator,
    ) -> GhosttyResult;
    pub fn ghostty_render_state_row_iterator_free(iter: GhosttyRowIterator);
    /// Returns `bool` per upstream signature. Rust `bool` is 1 byte —
    /// matches MSVC/gcc/clang C99 `_Bool` layout.
    pub fn ghostty_render_state_row_iterator_next(iter: GhosttyRowIterator) -> bool;
    pub fn ghostty_render_state_row_get(
        iter: GhosttyRowIterator,
        key: GhosttyRenderStateRowData,
        out: *mut c_void,
    ) -> GhosttyResult;

    pub fn ghostty_render_state_row_cells_new(
        allocator: *const GhosttyAllocator,
        out_cells: *mut GhosttyRowCells,
    ) -> GhosttyResult;
    pub fn ghostty_render_state_row_cells_free(cells: GhosttyRowCells);
    pub fn ghostty_render_state_row_cells_next(cells: GhosttyRowCells) -> bool;
    pub fn ghostty_render_state_row_cells_get(
        cells: GhosttyRowCells,
        key: GhosttyRenderStateRowCellsData,
        out: *mut c_void,
    ) -> GhosttyResult;

    // Cell accessor (`screen.h`). Decodes fields out of the opaque
    // `GhosttyCell` u64 we get from row_cells RAW.
    pub fn ghostty_cell_get(
        cell: GhosttyCell,
        key: GhosttyCellData,
        out: *mut c_void,
    ) -> GhosttyResult;
}

// ── Snapshot (renderer's view) ─────────────────────────────────────────

#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct Cell {
    pub codepoint: u32,
    /// Foreground RGBA (0xRRGGBBAA).
    pub fg: u32,
    /// Background RGBA.
    pub bg: u32,
    pub attrs: u8,
    pub _pad: [u8; 3],
}

#[derive(Debug, Clone, Copy, Default)]
pub struct Cursor {
    pub col: u16,
    pub row: u16,
    pub visible: bool,
}

#[derive(Debug, Clone, Default)]
pub struct ScreenSnapshot {
    pub cols: u16,
    pub rows: u16,
    pub cells: Vec<Cell>,
    pub dirty_rows: Vec<u16>,
    pub cursor: Cursor,
    pub title: Option<String>,
    pub generation: u64,
}

// ── Safe wrapper ───────────────────────────────────────────────────────

pub struct VtScreen {
    inner: Arc<Mutex<VtInner>>,
}

struct VtInner {
    terminal: GhosttyTerminal,
    render_state: GhosttyRenderState,
    row_iter: GhosttyRowIterator,
    row_cells: GhosttyRowCells,
    cols: u16,
    rows: u16,
    generation: u64,
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
            "VtScreen::new: ghostty_terminal_new(cols={cols}, rows={rows}, scrollback={})",
            options.max_scrollback
        );
        // SAFETY: out param; allocator NULL = upstream default.
        let rc = unsafe { ghostty_terminal_new(std::ptr::null(), &mut terminal, options) };
        if rc != 0 || terminal.is_null() {
            anyhow::bail!("ghostty_terminal_new failed: rc={rc}");
        }
        log::info!("VtScreen::new: terminal={terminal:?}");

        let mut render_state: GhosttyRenderState = std::ptr::null_mut();
        let mut row_iter: GhosttyRowIterator = std::ptr::null_mut();
        let mut row_cells: GhosttyRowCells = std::ptr::null_mut();

        let enable_render_state = std::env::var("CON_GHOSTTY_VT_RENDER_STATE")
            .map(|s| matches!(s.as_str(), "0" | "false" | "no" | "off"))
            .map(|disabled| !disabled)
            .unwrap_or(true);

        if enable_render_state {
            log::info!("VtScreen::new: ghostty_render_state_new(NULL_alloc)");
            // SAFETY: out param; allocator NULL = default.
            let rc =
                unsafe { ghostty_render_state_new(std::ptr::null(), &mut render_state) };
            if rc != 0 || render_state.is_null() {
                // SAFETY: terminal owned; free on partial init failure.
                unsafe { ghostty_terminal_free(terminal) };
                anyhow::bail!("ghostty_render_state_new failed: rc={rc}");
            }
            log::info!("VtScreen::new: render_state={render_state:?}");

            log::info!("VtScreen::new: ghostty_render_state_row_iterator_new(NULL_alloc)");
            // SAFETY: out param.
            let rc = unsafe {
                ghostty_render_state_row_iterator_new(std::ptr::null(), &mut row_iter)
            };
            if rc != 0 || row_iter.is_null() {
                unsafe { ghostty_render_state_free(render_state) };
                unsafe { ghostty_terminal_free(terminal) };
                anyhow::bail!("ghostty_render_state_row_iterator_new failed: rc={rc}");
            }
            log::info!("VtScreen::new: row_iter={row_iter:?}");

            log::info!("VtScreen::new: ghostty_render_state_row_cells_new(NULL_alloc)");
            // SAFETY: out param.
            let rc = unsafe {
                ghostty_render_state_row_cells_new(std::ptr::null(), &mut row_cells)
            };
            if rc != 0 || row_cells.is_null() {
                unsafe { ghostty_render_state_row_iterator_free(row_iter) };
                unsafe { ghostty_render_state_free(render_state) };
                unsafe { ghostty_terminal_free(terminal) };
                anyhow::bail!("ghostty_render_state_row_cells_new failed: rc={rc}");
            }
            log::info!("VtScreen::new: row_cells={row_cells:?}");
        } else {
            log::warn!(
                "VtScreen::new: render_state disabled via \
                 CON_GHOSTTY_VT_RENDER_STATE=0 — terminal output will \
                 parse but cells won't render."
            );
        }

        Ok(Self {
            inner: Arc::new(Mutex::new(VtInner {
                terminal,
                render_state,
                row_iter,
                row_cells,
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
            anyhow::bail!("ghostty_terminal_resize failed: rc={rc}");
        }
        inner.cols = cols;
        inner.rows = rows;
        inner.scratch = Vec::with_capacity(cols as usize * rows as usize);
        inner.generation = inner.generation.wrapping_add(1);
        Ok(())
    }

    pub fn snapshot(&self) -> ScreenSnapshot {
        let mut inner = self.inner.lock();

        let cols = inner.cols;
        let rows = inner.rows;

        if inner.render_state.is_null() {
            // Render-state path disabled — return empty snapshot. The
            // renderer still clears the pane to the background.
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

        // SAFETY: state + terminal valid for the lifetime of `inner`.
        let rc =
            unsafe { ghostty_render_state_update(inner.render_state, inner.terminal) };
        if rc != 0 {
            log::warn!("ghostty_render_state_update rc={rc}");
        }

        // Palette defaults. Cells with no explicit SGR color report
        // FG_COLOR / BG_COLOR as (0,0,0) — the renderer is expected to
        // substitute the terminal's default foreground/background from
        // the render state. Without this, the pwsh banner (and any
        // unstyled text) renders black-on-black.
        let mut default_fg = GhosttyColorRgb { r: 0xCC, g: 0xCC, b: 0xCC };
        let mut default_bg = GhosttyColorRgb::default();
        // SAFETY: out params typed as GhosttyColorRgb per render.h.
        unsafe {
            let _ = ghostty_render_state_get(
                inner.render_state,
                GhosttyRenderStateData::ColorForeground,
                &mut default_fg as *mut _ as *mut c_void,
            );
            let _ = ghostty_render_state_get(
                inner.render_state,
                GhosttyRenderStateData::ColorBackground,
                &mut default_bg as *mut _ as *mut c_void,
            );
        }

        let total = cols as usize * rows as usize;
        if inner.scratch.len() != total {
            inner.scratch.clear();
            inner.scratch.resize(total, Cell::default());
        }

        let mut dirty_rows: Vec<u16> = Vec::new();

        // Bind the row iterator to the current state.
        // SAFETY: state + iter valid.
        let rc = unsafe {
            ghostty_render_state_get(
                inner.render_state,
                GhosttyRenderStateData::RowIterator,
                &mut inner.row_iter as *mut _ as *mut c_void,
            )
        };
        if rc != 0 {
            log::warn!("ghostty_render_state_get(ROW_ITERATOR) rc={rc}");
            return empty_snapshot(cols, rows, inner.generation);
        }

        let mut row_idx: u16 = 0;
        // SAFETY: row_iter valid; `_next` returns bool.
        while unsafe { ghostty_render_state_row_iterator_next(inner.row_iter) } {
            if row_idx >= rows {
                break;
            }

            let mut dirty = GhosttyRenderStateDirty::False;
            // SAFETY: DIRTY out param is sized for the enum.
            unsafe {
                let _ = ghostty_render_state_row_get(
                    inner.row_iter,
                    GhosttyRenderStateRowData::Dirty,
                    &mut dirty as *mut _ as *mut c_void,
                );
            }

            if dirty == GhosttyRenderStateDirty::False {
                row_idx += 1;
                continue;
            }
            dirty_rows.push(row_idx);

            // Bind the cells iterator to the current row.
            // SAFETY: iter + cells valid.
            let rc = unsafe {
                ghostty_render_state_row_get(
                    inner.row_iter,
                    GhosttyRenderStateRowData::Cells,
                    &mut inner.row_cells as *mut _ as *mut c_void,
                )
            };
            if rc != 0 {
                log::warn!("row_get(CELLS) rc={rc} at row {row_idx}");
                row_idx += 1;
                continue;
            }

            let row_start = row_idx as usize * cols as usize;
            let mut col_idx: u16 = 0;
            // SAFETY: cells valid; `_next` returns bool.
            while unsafe { ghostty_render_state_row_cells_next(inner.row_cells) } {
                if col_idx >= cols {
                    break;
                }
                inner.scratch[row_start + col_idx as usize] =
                    read_cell(inner.row_cells, default_fg, default_bg);
                col_idx += 1;
            }
            // Clear trailing cells in the row.
            for c in col_idx..cols {
                inner.scratch[row_start + c as usize] = Cell::default();
            }

            row_idx += 1;
        }

        // Cursor read from the render state keys (not the terminal, to
        // stay consistent with the render snapshot).
        let mut visible: bool = false;
        let mut col_u16: u16 = 0;
        let mut row_u16: u16 = 0;
        // SAFETY: out params sized per upstream render.h.
        unsafe {
            let _ = ghostty_render_state_get(
                inner.render_state,
                GhosttyRenderStateData::CursorViewportX,
                &mut col_u16 as *mut _ as *mut c_void,
            );
            let _ = ghostty_render_state_get(
                inner.render_state,
                GhosttyRenderStateData::CursorViewportY,
                &mut row_u16 as *mut _ as *mut c_void,
            );
            let _ = ghostty_render_state_get(
                inner.render_state,
                GhosttyRenderStateData::CursorVisible,
                &mut visible as *mut _ as *mut c_void,
            );
        }

        let non_empty = inner
            .scratch
            .iter()
            .filter(|c| c.codepoint != 0 && c.codepoint != 0x20)
            .count();
        log::trace!(
            "snapshot: gen={} dirty_rows={} non_empty_cells={}/{} cursor=({},{})vis={}",
            inner.generation,
            dirty_rows.len(),
            non_empty,
            inner.scratch.len(),
            col_u16,
            row_u16,
            visible,
        );

        ScreenSnapshot {
            cols,
            rows,
            cells: inner.scratch.clone(),
            dirty_rows,
            cursor: Cursor {
                col: col_u16,
                row: row_u16,
                visible,
            },
            title: None,
            generation: inner.generation,
        }
    }

    pub fn size(&self) -> (u16, u16) {
        let inner = self.inner.lock();
        (inner.cols, inner.rows)
    }

    /// Returns `true` when at least one mouse-tracking mode is set
    /// (X10 / normal / button / any). Host-view mouse handlers gate
    /// mouse reporting on this so wheel / click / move don't leak
    /// escape sequences into shells that didn't ask for them.
    pub fn mouse_tracking_active(&self) -> bool {
        self.mode_active(MODE_NORMAL_MOUSE)
            || self.mode_active(MODE_BUTTON_MOUSE)
            || self.mode_active(MODE_ANY_MOUSE)
            || self.mode_active(MODE_X10_MOUSE)
    }

    /// SGR (1006) mouse format is the extended coord encoding.
    /// Callers use it to choose the report syntax; the default
    /// xterm legacy mouse report uses a different byte layout.
    pub fn is_sgr_mouse(&self) -> bool {
        self.mode_active(MODE_SGR_MOUSE)
    }

    /// Alt-screen scroll (1007): when set, mouse wheel in alt-screen
    /// apps is translated to arrow keys (up/down) rather than SGR
    /// reports. Apps like less / vim opt in.
    pub fn is_alt_scroll(&self) -> bool {
        self.mode_active(MODE_ALT_SCROLL)
    }

    /// Bracketed-paste mode (2004). When `true`, paste operations
    /// should wrap the payload in `ESC[200~ … ESC[201~` so the shell
    /// can treat it as a single paste.
    pub fn is_bracketed_paste(&self) -> bool {
        self.mode_active(MODE_BRACKETED_PASTE)
    }

    /// DECCKM (mode 1). When `true`, arrow keys must be encoded in
    /// application-cursor form (`ESC O A/B/C/D`) rather than the
    /// default cursor form (`ESC [ A/B/C/D`).
    pub fn is_decckm(&self) -> bool {
        self.mode_active(MODE_DECCKM)
    }

    /// Generic mode query — returns `false` when the FFI call fails
    /// or the mode isn't set. Never panics.
    pub fn mode_active(&self, mode: GhosttyMode) -> bool {
        let inner = self.inner.lock();
        if inner.terminal.is_null() {
            return false;
        }
        let mut on: bool = false;
        // SAFETY: terminal valid; `on` is a 1-byte C `_Bool`.
        let rc = unsafe { ghostty_terminal_mode_get(inner.terminal, mode, &mut on) };
        rc == 0 && on
    }
}

fn empty_snapshot(cols: u16, rows: u16, generation: u64) -> ScreenSnapshot {
    ScreenSnapshot {
        cols,
        rows,
        cells: Vec::new(),
        dirty_rows: Vec::new(),
        cursor: Cursor::default(),
        title: None,
        generation,
    }
}

fn read_cell(
    cells: GhosttyRowCells,
    default_fg: GhosttyColorRgb,
    default_bg: GhosttyColorRgb,
) -> Cell {
    // RAW here is an **opaque `GhosttyCell` u64 snapshot**, not a packed
    // codepoint. Decode fields via `ghostty_cell_get(cell, KEY, &out)`
    // per `screen.h`. Previous code bitshifted RAW directly and produced
    // nonsense codepoints (U+015C etc. for the "PowerShell" banner).
    let mut raw: GhosttyCell = 0;
    let mut style = GhosttyStyle::new();
    // BG_COLOR / FG_COLOR write a `GhosttyColorRgb` (3 bytes: R, G, B)
    // to the out pointer — NOT a packed u32.
    let mut bg = GhosttyColorRgb::default();
    let mut fg = GhosttyColorRgb::default();

    // SAFETY: out params typed per upstream contract (RAW=GhosttyCell u64,
    // STYLE=GhosttyStyle by value with `size` set to sizeof, BG/FG=GhosttyColorRgb).
    unsafe {
        let _ = ghostty_render_state_row_cells_get(
            cells,
            GhosttyRenderStateRowCellsData::Raw,
            &mut raw as *mut _ as *mut c_void,
        );
        let _ = ghostty_render_state_row_cells_get(
            cells,
            GhosttyRenderStateRowCellsData::Style,
            &mut style as *mut _ as *mut c_void,
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

    // Gate codepoint decode on HAS_TEXT — blank cells carry a bogus
    // grapheme-tag codepoint we'd otherwise rasterize.
    let mut has_text: bool = false;
    let mut codepoint: u32 = 0;
    // SAFETY: `has_text` is a C `_Bool` (1 byte); `codepoint` is uint32.
    unsafe {
        let _ = ghostty_cell_get(
            raw,
            GhosttyCellData::HasText,
            &mut has_text as *mut _ as *mut c_void,
        );
        if has_text {
            let _ = ghostty_cell_get(
                raw,
                GhosttyCellData::Codepoint,
                &mut codepoint as *mut _ as *mut c_void,
            );
        }
    }

    // Substitute the palette's default fg/bg when the cell reports
    // (0,0,0) — ghostty's convention for "unstyled, use default". A
    // proper fix reads STYLE_ID and inspects the color mode, but this
    // heuristic is good enough for first-pass rendering (we lose
    // explicit-black on a non-black bg, which is rare).
    let is_default = |c: GhosttyColorRgb| c.r == 0 && c.g == 0 && c.b == 0;
    let fg = if is_default(fg) { default_fg } else { fg };
    let bg = if is_default(bg) { default_bg } else { bg };

    // Pack RGB into the 0xRRGGBBAA u32 our HLSL `unpackRGBA` expects
    // (high byte = R, low byte = A). A=0xFF (opaque).
    let pack = |c: GhosttyColorRgb| -> u32 {
        ((c.r as u32) << 24) | ((c.g as u32) << 16) | ((c.b as u32) << 8) | 0xFF
    };

    // Pack style flags into the attrs byte the renderer (and HLSL
    // pixel shader) interpret. Underline is an `int` upstream
    // (0 = none, 1 = single, 2 = double, 3 = curly, ...); any non-zero
    // value enables our single underline rendering for now.
    let mut attrs: u8 = 0;
    if style.bold {
        attrs |= ATTR_BOLD;
    }
    if style.italic {
        attrs |= ATTR_ITALIC;
    }
    if style.underline != 0 {
        attrs |= ATTR_UNDERLINE;
    }
    if style.strikethrough {
        attrs |= ATTR_STRIKE;
    }
    if style.inverse {
        attrs |= ATTR_INVERSE;
    }

    Cell {
        codepoint,
        fg: pack(fg),
        bg: pack(bg),
        attrs,
        _pad: [0; 3],
    }
}

impl Drop for VtScreen {
    fn drop(&mut self) {
        if let Some(mutex) = Arc::get_mut(&mut self.inner) {
            let inner = mutex.get_mut();
            // Free in reverse-creation order: cells, iter, state, terminal.
            // SAFETY: unique owner via Arc::get_mut.
            if !inner.row_cells.is_null() {
                unsafe { ghostty_render_state_row_cells_free(inner.row_cells) };
                inner.row_cells = std::ptr::null_mut();
            }
            if !inner.row_iter.is_null() {
                unsafe { ghostty_render_state_row_iterator_free(inner.row_iter) };
                inner.row_iter = std::ptr::null_mut();
            }
            if !inner.render_state.is_null() {
                unsafe { ghostty_render_state_free(inner.render_state) };
                inner.render_state = std::ptr::null_mut();
            }
            if !inner.terminal.is_null() {
                unsafe { ghostty_terminal_free(inner.terminal) };
                inner.terminal = std::ptr::null_mut();
            }
        }
    }
}
