//! Terminal pane session: Renderer + VT parser + ConPTY.
//!
//! No child HWND is created. The renderer draws into an offscreen D3D11
//! texture; the caller reads back BGRA bytes each dirty frame and hands
//! them to GPUI as an `ImageSource::Render(Arc<RenderImage>)`. That puts
//! terminal content inside GPUI's own DirectComposition tree, which
//! eliminates the z-order problems the old WS_CHILD HWND had with
//! modals (settings / command palette painted under the HWND) and with
//! brand-new panes (the HWND would render one transparent frame before
//! its first Present).
//!
//! Thread model:
//! - All `Renderer` calls happen on GPUI's main thread via
//!   [`RenderSession::render_frame`] et al.
//! - The VT parser is fed from the ConPTY reader thread
//!   (`conpty.rs`) and snapshotted read-only on the main thread under
//!   its own Mutex.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

use anyhow::{Context, Result};
use parking_lot::Mutex;

use super::conpty::{ConPty, PtySize};
use super::render::{RenderOutcome, Renderer, RendererConfig, Selection, ThemeColors};
use super::vt::{ScreenSnapshot, VtScreen};

use super::render::CellMetrics;

/// Owns the D3D11 renderer, the VT parser, and the ConPTY child shell
/// for a single terminal pane. Exposes methods the GPUI view calls to
/// feed input, query state, and pull the latest rendered frame.
pub struct RenderSession {
    renderer: Mutex<Renderer>,
    vt: Arc<VtScreen>,
    conpty: Arc<ConPty>,
    config: Mutex<RendererConfig>,
    base_font_size_px: f32,
    dpi: AtomicU32,
    /// When a local user action mutates terminal state (typing, paste,
    /// mouse selection), the next render should prefer the freshest
    /// frame over the lowest-latency non-blocking staging drain. This
    /// avoids showing the stale pre-input frame for one more prepaint.
    low_latency_requested: AtomicBool,
    drag_anchor: Mutex<Option<(u16, u16)>>,
}

unsafe impl Send for RenderSession {}
unsafe impl Sync for RenderSession {}

/// Keyboard modifiers held at the time of a mouse event.
///
/// We don't import GPUI's `Modifiers` here because `con-ghostty` must
/// stay independent of the UI crate on Windows. The view layer copies
/// the three bits we care about (shift/alt/control) into this struct.
/// `platform` (the Win key / cmd key) is not reported in SGR and not
/// meaningful for xterm shift-bypass semantics, so it's deliberately
/// omitted.
#[derive(Debug, Default, Clone, Copy)]
pub struct MouseEventMods {
    pub shift: bool,
    pub alt: bool,
    pub control: bool,
}

impl RenderSession {
    /// Build a renderer + VT parser + ConPTY child shell.
    ///
    /// `wake` is invoked from the ConPTY reader thread after every
    /// chunk of bytes is fed into the VT parser. The view passes a
    /// closure that pokes a GPUI prepaint via `cx.notify()`, so freshly
    /// arrived shell output paints on the next frame instead of waiting
    /// for the next user input event. Without this hook, the prompt
    /// pwsh prints after `Enter` would sit in the grid until something
    /// else woke the view (mouse move, key press, focus change).
    pub fn new<W>(
        width_px: u32,
        height_px: u32,
        dpi: u32,
        config: RendererConfig,
        wake: W,
    ) -> Result<Self>
    where
        W: Fn() + Send + Sync + 'static,
    {
        let base_font_size_px = config.font_size_px;
        let current_dpi = if dpi == 0 { 96 } else { dpi };

        let mut renderer_config = config.clone();
        renderer_config.initial_width = width_px.max(1);
        renderer_config.initial_height = height_px.max(1);
        renderer_config.font_size_px = scale_font_size(base_font_size_px, current_dpi);

        log::info!(
            "RenderSession::new size={}x{} dpi={} font_px={:.2}",
            renderer_config.initial_width,
            renderer_config.initial_height,
            current_dpi,
            renderer_config.font_size_px,
        );

        let renderer = Renderer::new(&renderer_config).context("Renderer::new failed")?;
        let (cols, rows) = renderer.grid_for_dimensions(&renderer_config);
        log::info!("RenderSession: grid {cols}x{rows}");

        let vt = Arc::new(
            VtScreen::new(cols, rows, renderer_config.theme.as_ref())
                .context("VtScreen::new failed")?,
        );

        let vt_for_pty = vt.clone();
        let wake_for_pty: Arc<dyn Fn() + Send + Sync> = Arc::new(wake);
        let shell = super::conpty::default_shell_command();
        log::info!("RenderSession: spawning ConPTY shell={shell}");
        let conpty = ConPty::spawn(&shell, PtySize { cols, rows }, move |bytes| {
            vt_for_pty.feed(bytes);
            wake_for_pty();
        })
        .context("ConPty::spawn failed")?;
        let conpty = Arc::new(conpty);

        Ok(Self {
            renderer: Mutex::new(renderer),
            vt,
            conpty,
            config: Mutex::new(renderer_config),
            base_font_size_px,
            dpi: AtomicU32::new(current_dpi),
            low_latency_requested: AtomicBool::new(false),
            drag_anchor: Mutex::new(None),
        })
    }

    /// Render one frame. `Rendered` returns freshly-read BGRA bytes;
    /// `Unchanged` means "nothing moved, reuse the last image".
    pub fn render_frame(&self) -> Result<RenderOutcome> {
        let renderer = self.renderer.lock();
        let config = self.config.lock().clone();
        let snapshot = self.vt.snapshot();
        let prefer_latest = self.low_latency_requested.swap(false, Ordering::AcqRel);
        renderer.render(&snapshot, &config, prefer_latest)
    }

    /// Apply a new physical-pixel size. Idempotent for same dimensions.
    pub fn resize(&self, width_px: u32, height_px: u32) -> Result<()> {
        if width_px == 0 || height_px == 0 {
            return Ok(());
        }
        let mut renderer = self.renderer.lock();
        renderer
            .resize(width_px, height_px)
            .context("Renderer::resize failed")?;
        let config = self.config.lock();
        let (cols, rows) = renderer.grid_for_dimensions(&config);
        drop(config);
        let cell_w = (width_px / cols.max(1) as u32).max(1);
        let cell_h = (height_px / rows.max(1) as u32).max(1);
        drop(renderer);

        self.vt
            .resize(cols, rows, cell_w, cell_h)
            .context("VtScreen::resize failed")?;
        self.conpty
            .resize(PtySize { cols, rows })
            .context("ConPty::resize failed")?;
        log::debug!(
            "RenderSession::resize -> {width_px}x{height_px} grid={cols}x{rows} cell={cell_w}x{cell_h}"
        );
        Ok(())
    }

    /// Live update of the user-visible theme + window opacity.
    ///
    /// `theme` (when present) replaces libghostty's default fg/bg/palette
    /// so SGR colors resolve to the user's palette without restarting
    /// the pane. `background_opacity` is stored on the renderer config
    /// and read on every frame — the renderer rewrites the sentinel
    /// alpha=0 default-bg cells to `opacity*255` and pre-multiplies the
    /// clear color, so the cell grid composites over Mica / DComp at the
    /// requested level. Bumping the VT generation forces the next
    /// prepaint to repaint with the new colors / opacity.
    pub fn set_appearance(&self, theme: Option<&ThemeColors>, background_opacity: f32) {
        let clamped_opacity = background_opacity.clamp(0.0, 1.0);
        let mut config = self.config.lock();
        let opacity_changed = (config.background_opacity - clamped_opacity).abs() > f32::EPSILON;
        config.background_opacity = clamped_opacity;
        if let Some(theme) = theme {
            // Margins (pixels outside the cell grid) paint from
            // `clear_color`, so a theme switch that only rewrites the
            // palette would leave the border showing the previous
            // theme's background. Mirror what `WindowsGhosttyApp::
            // update_appearance` does at session construction.
            config.clear_color = [
                theme.bg[0] as f32 / 255.0,
                theme.bg[1] as f32 / 255.0,
                theme.bg[2] as f32 / 255.0,
                1.0,
            ];
            config.theme = Some(theme.clone());
            // `set_theme` bumps the VT generation itself, so the next
            // prepaint re-runs draw_cells with the new palette + new
            // clear_color + any new opacity.
            self.vt.set_theme(theme);
        } else {
            config.theme = None;
            // An opacity-only change doesn't touch the VT screen, so
            // the renderer's `needs_draw` gate (keyed on
            // snapshot.generation ⨁ selection) would otherwise serve
            // a stale cached frame until the next VT byte arrives.
            // Force a generation bump so the change is visible now.
            if opacity_changed {
                self.vt.bump_generation();
            }
        }
    }

    /// Notify of a DPI change. Rebuilds the glyph atlas at the new
    /// physical font size and re-derives the cell grid. Follow with a
    /// `resize` to match the new physical dimensions.
    pub fn set_dpi(&self, dpi: u32) -> Result<()> {
        let new_dpi = dpi.max(1);
        let prev = self.dpi.swap(new_dpi, Ordering::AcqRel);
        if prev == new_dpi {
            return Ok(());
        }
        let new_font = scale_font_size(self.base_font_size_px, new_dpi);
        let renderer = self.renderer.lock();
        renderer
            .rebuild_atlas(new_font)
            .context("rebuild_atlas on DPI change failed")?;
        let mut config = self.config.lock();
        config.font_size_px = new_font;
        log::info!("RenderSession::set_dpi {prev} -> {new_dpi} font_px={new_font:.2}");
        Ok(())
    }

    /// Current cell metrics (in physical pixels). Used by the GPUI view
    /// to translate mouse coordinates to cell addresses.
    pub fn metrics(&self) -> CellMetrics {
        self.renderer.lock().metrics()
    }

    pub fn vt(&self) -> &Arc<VtScreen> {
        &self.vt
    }

    pub fn is_alive(&self) -> bool {
        self.conpty.is_alive()
    }

    pub fn is_bracketed_paste(&self) -> bool {
        self.vt.is_bracketed_paste()
    }

    pub fn is_decckm(&self) -> bool {
        self.vt.is_decckm()
    }

    pub fn mouse_tracking_active(&self) -> bool {
        self.vt.mouse_tracking_active()
    }

    /// Send UTF-8 text to the child shell. Handles the ConPTY Enter
    /// quirk (shell expects CR, not LF).
    pub fn write_input(&self, text: &str) {
        self.request_low_latency_present();
        let bytes: std::borrow::Cow<[u8]> = if text.as_bytes().contains(&b'\n') {
            std::borrow::Cow::Owned(text.replace('\n', "\r").into_bytes())
        } else {
            std::borrow::Cow::Borrowed(text.as_bytes())
        };
        let _ = self.conpty.write(&bytes);
    }

    /// Raw PTY write — no CR/LF normalization. Used for bracketed-paste
    /// wrappers (ESC [200~ / ESC [201~) whose bytes mustn't be touched.
    pub fn write_pty_raw(&self, data: &[u8]) {
        self.request_low_latency_present();
        let _ = self.conpty.write(data);
    }

    /// Mouse-left-down at the given cell.
    ///
    /// Xterm convention: Shift bypasses mouse tracking so the user can
    /// always select text, even when a TUI has `set mouse=a` on. When
    /// tracking is off or Shift is held, we drive local selection;
    /// otherwise we emit an SGR button-press report and leave selection
    /// alone. Shift+click with an existing selection extends from the
    /// original anchor (matches every other terminal).
    pub fn mouse_down(&self, col: u16, row: u16, mods: MouseEventMods) {
        self.request_low_latency_present();
        if self.vt.mouse_tracking_active() && !mods.shift {
            self.report_sgr_button(0, col, row, mods, true);
            return;
        }
        if mods.shift {
            let renderer = self.renderer.lock();
            let existing_anchor = renderer.selection().map(|s| s.anchor).unwrap_or((col, row));
            *self.drag_anchor.lock() = Some(existing_anchor);
            renderer.set_selection(Some(Selection {
                anchor: existing_anchor,
                extent: (col, row),
            }));
            return;
        }
        *self.drag_anchor.lock() = Some((col, row));
        self.renderer.lock().set_selection(Some(Selection {
            anchor: (col, row),
            extent: (col, row),
        }));
    }

    /// Mouse-moved at the given cell while left button is held.
    ///
    /// When mouse tracking is active and the shell requested motion
    /// (BUTTON / ANY mode), we emit an SGR motion report with the
    /// motion bit (+32) set. Otherwise we extend the local drag.
    pub fn mouse_drag(&self, col: u16, row: u16, mods: MouseEventMods) {
        self.request_low_latency_present();
        if self.vt.mouse_tracking_active() && !mods.shift {
            // Button 0 (LMB) + 32 = motion-with-button bit per SGR spec.
            self.report_sgr_button(32, col, row, mods, true);
            return;
        }
        let anchor = *self.drag_anchor.lock();
        if let Some(anchor) = anchor {
            self.renderer.lock().set_selection(Some(Selection {
                anchor,
                extent: (col, row),
            }));
        }
    }

    /// Mouse-left-up at the given cell.
    ///
    /// Emits an SGR release when mouse tracking is active (unless Shift
    /// is held to keep selection). Otherwise clears a transient 1-cell
    /// selection — a click without drag shouldn't leave a lone cell
    /// highlighted.
    pub fn mouse_up(&self, col: u16, row: u16, mods: MouseEventMods) {
        self.request_low_latency_present();
        if self.vt.mouse_tracking_active() && !mods.shift {
            self.report_sgr_button(0, col, row, mods, false);
            return;
        }
        let anchor = self.drag_anchor.lock().take();
        if let Some(anchor) = anchor
            && anchor == (col, row)
        {
            self.renderer.lock().set_selection(None);
        }
    }

    fn report_sgr_button(
        &self,
        base_button: u8,
        col: u16,
        row: u16,
        mods: MouseEventMods,
        pressed: bool,
    ) {
        let col = col.saturating_add(1);
        let row = row.saturating_add(1);
        let mut cb = base_button;
        if mods.alt {
            cb |= 0x08;
        }
        if mods.control {
            cb |= 0x10;
        }
        let terminator = if pressed { 'M' } else { 'm' };
        let seq = format!("\x1b[<{cb};{col};{row}{terminator}");
        let _ = self.conpty.write(seq.as_bytes());
    }

    /// Cancel any in-flight drag (used on focus loss).
    pub fn cancel_drag(&self) {
        *self.drag_anchor.lock() = None;
    }

    /// SGR mouse-wheel report. Only used when the shell has enabled
    /// mouse tracking — see `mouse_tracking_active`. `col`/`row` are
    /// 1-based cell coordinates per the SGR spec. Alt/Ctrl are encoded
    /// into the button byte; Shift is handled upstream by the view,
    /// which bypasses reporting entirely when Shift is held so the user
    /// can scroll Con's own scrollback without the TUI seeing it.
    pub fn forward_wheel(&self, col: u16, row: u16, delta_y: f32, mods: MouseEventMods) {
        if delta_y.abs() < f32::EPSILON {
            return;
        }
        self.request_low_latency_present();
        let mut button: u8 = if delta_y < 0.0 { 64 } else { 65 };
        if mods.alt {
            button |= 0x08;
        }
        if mods.control {
            button |= 0x10;
        }
        let col = col.max(1);
        let row = row.max(1);
        let seq = format!("\x1b[<{button};{col};{row}M");
        let _ = self.conpty.write(seq.as_bytes());
    }

    pub fn has_selection(&self) -> bool {
        self.renderer.lock().selection().is_some()
    }

    /// Extract the current selection as text. Returns `None` when
    /// nothing is selected.
    pub fn selection_text(&self) -> Option<String> {
        let selection = self.renderer.lock().selection()?;
        let snapshot = self.vt.snapshot();
        Some(extract_selection_text(&snapshot, selection))
    }

    pub fn clear_selection(&self) {
        self.renderer.lock().set_selection(None);
    }

    pub fn dimensions_px(&self) -> (u32, u32) {
        self.renderer.lock().dimensions_px()
    }

    fn request_low_latency_present(&self) {
        self.low_latency_requested.store(true, Ordering::Release);
    }
}

fn scale_font_size(logical_px: f32, dpi: u32) -> f32 {
    logical_px * (dpi as f32) / 96.0
}

fn extract_selection_text(snapshot: &ScreenSnapshot, sel: Selection) -> String {
    let cols = snapshot.cols;
    if cols == 0 || snapshot.cells.is_empty() {
        return String::new();
    }
    let mut out = String::new();
    let rows = snapshot.rows;
    for row in 0..rows {
        let mut row_buf = String::new();
        let mut row_has_cell = false;
        let mut last_non_blank: i32 = -1;
        for col in 0..cols {
            if !sel.contains(col, row, cols) {
                continue;
            }
            row_has_cell = true;
            let idx = row as usize * cols as usize + col as usize;
            let cell = snapshot.cells.get(idx).copied().unwrap_or_default();
            let ch = if cell.codepoint == 0 {
                ' '
            } else {
                char::from_u32(cell.codepoint).unwrap_or(' ')
            };
            row_buf.push(ch);
            if cell.codepoint != 0 && cell.codepoint != 0x20 {
                last_non_blank = row_buf.chars().count() as i32 - 1;
            }
        }
        if !row_has_cell {
            continue;
        }
        if last_non_blank >= 0 {
            let trimmed: String = row_buf.chars().take(last_non_blank as usize + 1).collect();
            out.push_str(&trimmed);
        }
        out.push('\n');
    }
    if out.ends_with('\n') {
        out.pop();
    }
    out
}
