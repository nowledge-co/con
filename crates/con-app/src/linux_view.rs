//! Linux terminal view backed by con's local Unix PTY + libghostty-vt
//! parser. Phase 4 styled-cell renderer: this view consumes the parsed
//! `ScreenSnapshot` from `con-ghostty` and paints each row as a GPUI
//! `StyledText` element with one `TextRun` per styled span. That keeps
//! prompt colors, ANSI palette, bold/italic/underline, and selection
//! inverse working without bringing the full D3D11 / DirectWrite stack
//! the Windows backend needs.
//!
//! Rendering trade-off: this is a CPU-side per-cell paint path, not a
//! real glyph atlas. It's good enough for shell prompts, vim/less, and
//! basic TUIs while the long-term GPUI-owned grid renderer matures, and
//! it avoids the previous "trim to plain text" downgrade that hid color
//! and layout state. The Windows D3D11 path remains the model for the
//! eventual native renderer.

use std::sync::Arc;

use con_ghostty::{
    GhosttyApp, GhosttySplitDirection, GhosttyTerminal, ScreenSnapshot, SurfaceSize, VtCell,
    VtCursor, ATTR_BOLD, ATTR_INVERSE, ATTR_ITALIC, ATTR_STRIKE, ATTR_UNDERLINE,
};
use gpui::*;
use gpui_component::ActiveTheme;

const DEFAULT_FONT_SIZE: f32 = 14.0;
const MIN_FONT_SIZE_PX: f32 = 12.0;
const DEFAULT_CELL_WIDTH_RATIO: f32 = 0.62;
const DEFAULT_CELL_HEIGHT_RATIO: f32 = 1.45;

/// Resolved logical font size used for both the cell-grid estimate
/// (`estimate_surface_size`) and the actual paint (`render`). Both
/// callers used to clamp differently — paint floored at 12 px,
/// estimate didn't — which let a sub-12 px config size the PTY grid
/// to cells smaller than the cells we actually drew, so text
/// overran the estimated column count and lines wrapped unexpectedly
/// on the alternate screen. Centralising here keeps them honest.
fn effective_font_size(configured: f32) -> f32 {
    let base = if configured > 0.0 {
        configured
    } else {
        DEFAULT_FONT_SIZE
    };
    base.max(MIN_FONT_SIZE_PX)
}

actions!(ghostty, [ConsumeTab, ConsumeTabPrev]);

#[allow(dead_code)]
pub struct GhosttyTitleChanged(pub Option<String>);
pub struct GhosttyProcessExited;
pub struct GhosttyFocusChanged;
pub struct GhosttySplitRequested(pub GhosttySplitDirection);

impl EventEmitter<GhosttyTitleChanged> for GhosttyView {}
impl EventEmitter<GhosttyProcessExited> for GhosttyView {}
impl EventEmitter<GhosttyFocusChanged> for GhosttyView {}
impl EventEmitter<GhosttySplitRequested> for GhosttyView {}

pub struct GhosttyView {
    app: Arc<GhosttyApp>,
    terminal: Option<Arc<GhosttyTerminal>>,
    focus_handle: FocusHandle,
    initial_cwd: Option<String>,
    initial_font_size: f32,
    initialized: bool,
    process_exit_emitted: bool,
    last_title: Option<String>,
    pending_write: Option<Vec<u8>>,
    snapshot: Option<ScreenSnapshot>,
    row_cache: Vec<CachedTerminalRow>,
    row_cache_generation: Option<u64>,
    row_cache_cursor: Option<VtCursor>,
    row_cache_style: Option<RowCacheStyleKey>,
    row_cache_shape: Option<(u16, u16)>,
    /// Latched after the first PTY snapshot that contained any
    /// printable content. Used to gate the "Waiting for shell
    /// prompt…" placeholder so it disappears the moment bash echoes
    /// its first prompt and never comes back — even when a TUI like
    /// htop / vim / less switches to the alternate screen and
    /// briefly leaves the grid empty before drawing its own UI.
    seen_any_output: bool,
    pane_bounds: Option<Bounds<Pixels>>,
    scale_factor: f32,
    last_surface_size: Option<(u32, u32, u16, u16)>,
}

pub fn init(_cx: &mut App) {}

impl GhosttyView {
    pub fn new(
        app: Arc<GhosttyApp>,
        cwd: Option<String>,
        font_size: f32,
        cx: &mut Context<Self>,
    ) -> Self {
        Self {
            app,
            terminal: Some(Arc::new(GhosttyTerminal::new())),
            focus_handle: cx.focus_handle(),
            initial_cwd: cwd,
            initial_font_size: font_size,
            initialized: false,
            process_exit_emitted: false,
            last_title: None,
            pending_write: None,
            snapshot: None,
            row_cache: Vec::new(),
            row_cache_generation: None,
            row_cache_cursor: None,
            row_cache_style: None,
            row_cache_shape: None,
            seen_any_output: false,
            pane_bounds: None,
            scale_factor: 1.0,
            last_surface_size: None,
        }
    }

    pub fn terminal(&self) -> Option<&Arc<GhosttyTerminal>> {
        self.terminal.as_ref()
    }

    pub fn write_or_queue(&mut self, data: &[u8]) {
        if let Some(terminal) = &self.terminal {
            if self.initialized && terminal.is_attached() {
                terminal.write_to_pty(data);
                return;
            }
        }

        self.pending_write
            .get_or_insert_with(Vec::new)
            .extend_from_slice(data);
    }

    pub fn title(&self) -> Option<String> {
        self.terminal.as_ref().and_then(|terminal| terminal.title())
    }

    pub fn current_dir(&self) -> Option<String> {
        self.terminal
            .as_ref()
            .and_then(|terminal| terminal.current_dir())
            .or_else(|| self.initial_cwd.clone())
    }

    pub fn is_alive(&self) -> bool {
        self.terminal
            .as_ref()
            .is_some_and(|terminal| terminal.is_alive())
    }

    pub fn surface_ready(&self) -> bool {
        self.initialized
    }

    #[allow(dead_code)]
    pub fn selection_text(&self) -> Option<String> {
        None
    }

    pub fn shutdown_surface(&mut self) {
        if let Some(terminal) = &self.terminal {
            terminal.request_close();
        }
        self.initialized = false;
        self.process_exit_emitted = false;
        self.last_title = None;
        self.pending_write = None;
        self.snapshot = None;
        self.row_cache.clear();
        self.row_cache_generation = None;
        self.row_cache_cursor = None;
        self.row_cache_style = None;
        self.row_cache_shape = None;
        self.seen_any_output = false;
        self.last_surface_size = None;
    }

    pub fn set_surface_focus_state(&mut self, focused: bool) {
        if let Some(terminal) = &self.terminal {
            terminal.set_focus(focused);
        }
    }

    pub fn ensure_initialized_for_control(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let _ = self.ensure_session(cx);
        if let Some(bounds) = self.pane_bounds {
            let _ = self.sync_surface_size(bounds, window.scale_factor());
        }
    }

    pub fn set_visible(&self, _visible: bool) {}

    pub fn sync_window_background_blur(&self) {}

    pub fn drain_surface_state(&mut self, cx: &mut Context<Self>) -> bool {
        let mut changed = self.ensure_session(cx);

        let Some(terminal) = self.terminal.as_ref().cloned() else {
            return changed;
        };

        if terminal.take_needs_render() {
            changed |= self.refresh_snapshot();
        }

        let title = terminal.title();
        if title != self.last_title {
            self.last_title = title.clone();
            changed = true;
            cx.emit(GhosttyTitleChanged(title));
        }

        if !terminal.is_alive() && !self.process_exit_emitted {
            self.process_exit_emitted = true;
            changed = true;
            cx.emit(GhosttyProcessExited);
        }

        if changed {
            cx.notify();
        }

        changed
    }

    pub fn pump_deferred_work(&mut self, cx: &mut Context<Self>) -> bool {
        let mut changed = self.ensure_session(cx);

        if let Some(terminal) = self.terminal.as_ref().cloned() {
            // Only re-snapshot when libghostty-vt actually has new
            // output. The previous code also fell through to
            // `refresh_snapshot()` on every poll tick whenever the
            // shell was alive — that re-ran the full FFI walk
            // 60×/s for nothing, ate measurable CPU on busy panes
            // (htop / vim), and also drowned out the per-PTY-write
            // wake signal we explicitly want to react to.
            if terminal.take_needs_render() {
                changed |= self.refresh_snapshot();
            }

            let title = terminal.title();
            if title != self.last_title {
                self.last_title = title.clone();
                changed = true;
                cx.emit(GhosttyTitleChanged(title));
            }

            if !terminal.is_alive() && !self.process_exit_emitted {
                self.process_exit_emitted = true;
                changed = true;
                cx.emit(GhosttyProcessExited);
            }
        }

        if changed {
            cx.notify();
        }

        changed
    }

    fn ensure_session(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(terminal) = self.terminal.as_ref() else {
            return false;
        };

        if terminal.is_attached() {
            self.initialized = true;
            return false;
        }

        let options = self.app.default_pty_options(self.initial_cwd.as_deref());
        match terminal.spawn_with_options(options) {
            Ok(()) => {
                self.initialized = true;
                self.process_exit_emitted = false;
                if let Some(pending) = self.pending_write.take() {
                    terminal.write_to_pty(&pending);
                }
                self.last_title = terminal.title();
                let _ = self.refresh_snapshot();
                cx.notify();
                true
            }
            Err(err) => {
                log::error!("failed to start linux shell: {err}");
                false
            }
        }
    }

    fn refresh_snapshot(&mut self) -> bool {
        let Some(terminal) = self.terminal.as_ref() else {
            return false;
        };
        let Some(snapshot) = terminal.snapshot() else {
            return false;
        };
        // Generation alone is enough: libghostty-vt bumps the screen
        // generation on every parser feed that changed grid state.
        // The previous code also did a `prev.cells == snapshot.cells`
        // deep-compare on every refresh — that was a 50–200 KB Vec
        // compare per frame on busy panes and never short-circuited
        // (callers only invoke this when `take_needs_render()`
        // already returned true), so it was pure cost.
        if self
            .snapshot
            .as_ref()
            .is_some_and(|prev| prev.generation == snapshot.generation)
        {
            return false;
        }
        // Latch once the parser has handed us any printable cell.
        // Used to suppress the "Waiting for shell prompt…" placeholder
        // for the lifetime of the PTY session — important for TUIs
        // (htop, vim, less, fzf, …) that switch to the alternate
        // screen and leave the grid empty for ~hundreds of ms before
        // drawing their UI. Without this latch the placeholder would
        // briefly flash over a black backdrop on every alt-screen
        // entry and look like a regression in shell readiness.
        if !self.seen_any_output && snapshot.cells.iter().any(|c| c.codepoint != 0) {
            self.seen_any_output = true;
        }
        self.snapshot = Some(snapshot);
        true
    }

    fn sync_surface_size(&mut self, bounds: Bounds<Pixels>, scale_factor: f32) -> bool {
        self.pane_bounds = Some(bounds);
        self.scale_factor = scale_factor;

        let Some(terminal) = self.terminal.as_ref() else {
            return false;
        };

        let size = self.estimate_surface_size(bounds, scale_factor);
        let signature = (size.width_px, size.height_px, size.columns, size.rows);
        if self.last_surface_size == Some(signature) {
            return false;
        }

        terminal.resize_surface(size);
        self.last_surface_size = Some(signature);
        false
    }

    fn estimate_surface_size(&self, bounds: Bounds<Pixels>, scale_factor: f32) -> SurfaceSize {
        let width_px = ((f32::from(bounds.size.width) * scale_factor).ceil() as u32).max(1);
        let height_px = ((f32::from(bounds.size.height) * scale_factor).ceil() as u32).max(1);

        // Until the real grid renderer lands we estimate the PTY grid
        // from the configured mono font size so shells and TUIs do
        // not stay stuck at the initial 80x24 forever. Run through
        // the same `effective_font_size` clamp `render()` uses so
        // the grid we ask the PTY for matches the cell size we
        // actually paint at — picking different floors here would
        // make text overrun the estimated column count and lines
        // wrap unexpectedly on the alternate screen.
        let font_size_px = effective_font_size(self.initial_font_size) * scale_factor;
        let cell_width_px = (font_size_px * DEFAULT_CELL_WIDTH_RATIO).round().max(7.0) as u32;
        let cell_height_px = (font_size_px * DEFAULT_CELL_HEIGHT_RATIO).round().max(14.0) as u32;
        let columns = (width_px / cell_width_px.max(1))
            .max(1)
            .min(u32::from(u16::MAX)) as u16;
        let rows = (height_px / cell_height_px.max(1))
            .max(1)
            .min(u32::from(u16::MAX)) as u16;

        SurfaceSize {
            columns,
            rows,
            width_px,
            height_px,
            cell_width_px,
            cell_height_px,
        }
    }

    fn handle_key_down(&mut self, event: &KeyDownEvent, cx: &mut Context<Self>) -> bool {
        let Some(terminal) = self.terminal.as_ref() else {
            return false;
        };

        let keystroke = &event.keystroke;
        if keystroke.modifiers.platform {
            return false;
        }

        if keystroke.modifiers.control
            && keystroke.modifiers.shift
            && !keystroke.modifiers.alt
            && keystroke.key == "v"
        {
            if let Some(text) = cx
                .read_from_clipboard()
                .and_then(|item| item.text().map(|s| s.to_string()))
                .filter(|text| !text.is_empty())
            {
                if terminal.is_bracketed_paste() {
                    let mut wrapped = String::with_capacity(text.len() + 12);
                    wrapped.push_str("\x1b[200~");
                    wrapped.push_str(&text);
                    wrapped.push_str("\x1b[201~");
                    terminal.send_text(&wrapped);
                } else {
                    terminal.send_text(&text);
                }
                return true;
            }
        }

        let decckm = terminal.is_decckm();
        if let Some(bytes) = encode_special_key(&keystroke.key, &keystroke.modifiers, decckm) {
            terminal.send_text(&bytes);
            return true;
        }

        if keystroke.modifiers.control
            && !keystroke.modifiers.alt
            && !keystroke.modifiers.shift
            && keystroke.key.len() == 1
        {
            if let Some(ch) = keystroke.key.chars().next() {
                if ch.is_ascii_alphabetic() {
                    let control = [ch.to_ascii_uppercase() as u8 - b'@'];
                    if let Ok(text) = std::str::from_utf8(&control) {
                        terminal.send_text(text);
                        return true;
                    }
                }
            }
        }

        if keystroke.modifiers.alt && !keystroke.modifiers.control && !keystroke.modifiers.shift {
            if let Some(ch) = keystroke.key_char.as_deref().filter(|ch| !ch.is_empty()) {
                let mut out = String::with_capacity(1 + ch.len());
                out.push('\x1b');
                out.push_str(ch);
                terminal.send_text(&out);
                return true;
            }
        }

        if let Some(text) = keystroke
            .key_char
            .as_deref()
            .filter(|text| !text.is_empty())
        {
            terminal.send_text(text);
            return true;
        }

        if keystroke.key.len() == 1 {
            terminal.send_text(&keystroke.key);
            return true;
        }

        false
    }

    fn sync_row_cache(
        &mut self,
        default_fg: Hsla,
        default_bg: Hsla,
        base_font: &Font,
        font_size: Pixels,
        line_height: Pixels,
    ) {
        let Some(snapshot) = self.snapshot.as_ref() else {
            self.row_cache.clear();
            self.row_cache_generation = None;
            self.row_cache_cursor = None;
            self.row_cache_style = None;
            self.row_cache_shape = None;
            return;
        };

        let style = RowCacheStyleKey {
            font_family: base_font.family.clone(),
            default_fg,
            default_bg,
            font_size,
            line_height,
        };
        let shape = (snapshot.cols, snapshot.rows);
        let force_full_rebuild = self.row_cache_style.as_ref() != Some(&style)
            || self.row_cache_shape != Some(shape)
            || self.row_cache.len() != usize::from(snapshot.rows);

        if force_full_rebuild {
            self.row_cache
                .resize_with(usize::from(snapshot.rows), CachedTerminalRow::default);
        }

        let mut rows_to_refresh =
            if force_full_rebuild || self.row_cache_generation != Some(snapshot.generation) {
                rows_needing_refresh(snapshot, self.row_cache_cursor, force_full_rebuild)
            } else {
                Vec::new()
            };

        rows_to_refresh.sort_unstable();
        rows_to_refresh.dedup();

        for row_idx in rows_to_refresh {
            let row_start = row_idx * usize::from(snapshot.cols);
            let row_end = row_start + usize::from(snapshot.cols);
            let Some(cells) = snapshot.cells.get(row_start..row_end) else {
                break;
            };
            let cursor_for_row = cursor_col_for_row(snapshot.cursor, row_idx);
            self.row_cache[row_idx] =
                build_terminal_row(cells, default_fg, default_bg, base_font, cursor_for_row);
        }

        self.row_cache_generation = Some(snapshot.generation);
        self.row_cache_cursor = Some(snapshot.cursor);
        self.row_cache_style = Some(style);
        self.row_cache_shape = Some(shape);
    }
}

impl Focusable for GhosttyView {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for GhosttyView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let focus = self.focus_handle.clone();
        let entity = cx.entity().downgrade();
        let font_size_px = effective_font_size(self.initial_font_size);
        let line_height_px = (font_size_px * 1.45).round();
        let mono_font = Font {
            family: theme.mono_font_family.clone(),
            features: FontFeatures::default(),
            fallbacks: None,
            weight: FontWeight::NORMAL,
            style: FontStyle::Normal,
        };

        let status_message = if !self.initialized {
            Some("Launching Linux shell…")
        } else if !self.is_alive() {
            Some("Linux shell exited")
        } else if !self.seen_any_output {
            // Only show the "waiting for prompt" placeholder before
            // bash has echoed *anything* for the first time. Once
            // the latch flips, alt-screen TUIs like htop / vim that
            // briefly clear the grid stay silent instead of
            // flashing this placeholder over their startup gap.
            Some("Waiting for shell prompt…")
        } else {
            None
        };

        let foreground = theme.foreground;
        let status_color = foreground.opacity(0.5);
        self.sync_row_cache(
            foreground,
            theme.background,
            &mono_font,
            px(font_size_px),
            px(line_height_px),
        );

        let mut rows: Vec<AnyElement> = Vec::with_capacity(
            usize::from(self.snapshot.as_ref().map_or(0, |snapshot| snapshot.rows))
                + if status_message.is_some() { 1 } else { 0 },
        );
        if let Some(message) = status_message {
            rows.push(
                div()
                    .font_family(theme.mono_font_family.clone())
                    .text_size(px(font_size_px))
                    .line_height(px(line_height_px))
                    .text_color(status_color)
                    .child(message.to_string())
                    .into_any_element(),
            );
        }

        if let Some(snapshot) = self.snapshot.as_ref() {
            for row_idx in 0..usize::from(snapshot.rows) {
                if let Some(row) = self.row_cache.get(row_idx) {
                    rows.push(render_cached_terminal_row(
                        row,
                        px(font_size_px),
                        px(line_height_px),
                    ));
                }
            }
        }

        if rows.is_empty() {
            rows.push(
                div()
                    .font_family(theme.mono_font_family.clone())
                    .text_size(px(font_size_px))
                    .line_height(px(line_height_px))
                    .text_color(status_color)
                    .child("\u{00A0}".to_string())
                    .into_any_element(),
            );
        }

        let pane_opacity = self.app.background_opacity().clamp(0.0, 1.0);
        let terminal_content = div()
            .flex()
            .flex_col()
            .size_full()
            .min_w_0()
            .min_h_0()
            .overflow_hidden()
            .bg(theme.background.opacity(pane_opacity))
            .px(px(12.0))
            .py(px(10.0))
            .text_color(foreground)
            .items_start()
            .justify_start()
            .children(rows);

        div()
            .flex()
            .flex_col()
            .size_full()
            .min_w_0()
            .min_h_0()
            .track_focus(&self.focus_handle)
            .bg(theme.background.opacity(pane_opacity))
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                if !this.focus_handle.is_focused(window) {
                    return;
                }
                let _ = this.ensure_session(cx);
                if this.handle_key_down(event, cx) {
                    window.prevent_default();
                    cx.stop_propagation();
                    cx.notify();
                }
            }))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, _event: &MouseDownEvent, window, cx| {
                    window.focus(&focus, cx);
                    let _ = this.ensure_session(cx);
                    cx.emit(GhosttyFocusChanged);
                    cx.notify();
                }),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .size_full()
                    .min_w_0()
                    .min_h_0()
                    .overflow_hidden()
                    .on_children_prepainted(move |bounds_list: Vec<Bounds<Pixels>>, window, cx| {
                        let Some(bounds) = bounds_list.first().copied() else {
                            return;
                        };
                        let scale = window.scale_factor();
                        if let Some(view) = entity.upgrade() {
                            view.update(cx, |view, cx| {
                                let mut changed = view.ensure_session(cx);
                                changed |= view.sync_surface_size(bounds, scale);
                                if changed {
                                    cx.notify();
                                }
                            });
                        }
                    })
                    .child(terminal_content),
            )
    }
}

impl Drop for GhosttyView {
    fn drop(&mut self) {
        if let Some(terminal) = &self.terminal {
            terminal.request_close();
        }
    }
}

#[derive(Clone, Default)]
struct CachedTerminalRow {
    text: SharedString,
    runs: Vec<TextRun>,
}

#[derive(Clone, PartialEq)]
struct RowCacheStyleKey {
    font_family: SharedString,
    default_fg: Hsla,
    default_bg: Hsla,
    font_size: Pixels,
    line_height: Pixels,
}

fn cursor_col_for_row(cursor: VtCursor, row_idx: usize) -> Option<usize> {
    if cursor.visible && usize::from(cursor.row) == row_idx {
        Some(usize::from(cursor.col))
    } else {
        None
    }
}

fn rows_needing_refresh(
    snapshot: &ScreenSnapshot,
    previous_cursor: Option<VtCursor>,
    force_full_rebuild: bool,
) -> Vec<usize> {
    if force_full_rebuild {
        return (0..usize::from(snapshot.rows)).collect();
    }

    let mut rows = snapshot
        .dirty_rows
        .iter()
        .copied()
        .filter(|row| *row < snapshot.rows)
        .map(usize::from)
        .collect::<Vec<_>>();

    if let Some(previous) = previous_cursor {
        if previous.visible && previous.row < snapshot.rows {
            rows.push(usize::from(previous.row));
        }
    }
    if snapshot.cursor.visible && snapshot.cursor.row < snapshot.rows {
        rows.push(usize::from(snapshot.cursor.row));
    }

    rows
}

fn render_cached_terminal_row(
    row: &CachedTerminalRow,
    font_size: Pixels,
    line_height: Pixels,
) -> AnyElement {
    div()
        .text_size(font_size)
        .line_height(line_height)
        .child(StyledText::new(row.text.clone()).with_runs(row.runs.clone()))
        .into_any_element()
}

/// Build a single GPUI row element from a slice of `VtCell`s. We
/// collapse runs of cells that share `(fg, bg, attrs)` into one
/// `TextRun` so each row is a single `StyledText` element. That keeps
/// allocations bounded by the number of *style changes*, not the cell
/// count, while still preserving every SGR transition.
fn build_terminal_row(
    cells: &[VtCell],
    default_fg: Hsla,
    default_bg: Hsla,
    base_font: &Font,
    cursor_col: Option<usize>,
) -> CachedTerminalRow {
    // First pass: find the last column we have to keep. A column
    // matters if it has a real glyph, OR if it carries a non-default
    // background / underline / strikethrough / inverse style, OR if
    // it sits under the cursor. Trailing default-styled blanks past
    // that column are dropped so we don't emit hundreds of empty
    // cells per row, but trailing *styled* blanks (status bars,
    // selection highlights, full-width fills) survive — those carry
    // visual information in their background color and dropping them
    // would collapse the line paint width.
    let last_meaningful_col = cells
        .iter()
        .enumerate()
        .rposition(|(col_idx, cell)| {
            let glyph_present =
                cell.codepoint != 0 && char::from_u32(cell.codepoint).is_some_and(|ch| ch != ' ');
            let styled_blank = (cell.bg & 0xFF) != 0
                || (cell.attrs & (ATTR_INVERSE | ATTR_UNDERLINE | ATTR_STRIKE)) != 0;
            let cursor_here = cursor_col == Some(col_idx);
            glyph_present || styled_blank || cursor_here
        })
        // `rposition` already returns the index relative to `cells`,
        // not `iter().enumerate()`'s output. Map it back to a slice
        // length via +1.
        .map(|idx| idx + 1)
        .unwrap_or(0);

    let kept = &cells[..last_meaningful_col];

    let mut text = String::with_capacity(kept.len());
    let mut runs: Vec<TextRun> = Vec::new();
    let mut last_signature: Option<(u32, u32, u8, bool)> = None;
    let mut active_run_len: usize = 0;
    let mut active_style: Option<RowStyle> = None;

    fn flush_run(
        runs: &mut Vec<TextRun>,
        active_style: &mut Option<RowStyle>,
        active_run_len: &mut usize,
    ) {
        if *active_run_len == 0 || active_style.is_none() {
            return;
        }
        let style = active_style.take().expect("active style");
        runs.push(TextRun {
            len: *active_run_len,
            font: style.font,
            color: style.fg,
            background_color: style.bg,
            underline: style.underline,
            strikethrough: style.strikethrough,
        });
        *active_run_len = 0;
    }

    for (col_idx, cell) in kept.iter().enumerate() {
        let is_cursor = cursor_col == Some(col_idx);
        let signature = (cell.fg, cell.bg, cell.attrs, is_cursor);
        let style = RowStyle::from_cell(cell, default_fg, default_bg, base_font, is_cursor);

        let glyph: char = match cell.codepoint {
            0 => ' ',
            cp => char::from_u32(cp).unwrap_or('\u{FFFD}'),
        };

        if Some(signature) != last_signature {
            flush_run(&mut runs, &mut active_style, &mut active_run_len);
            active_style = Some(style);
            last_signature = Some(signature);
        }

        text.push(glyph);
        active_run_len += glyph.len_utf8();
    }

    flush_run(&mut runs, &mut active_style, &mut active_run_len);

    let text = if text.is_empty() {
        let mut fallback = String::with_capacity(1);
        fallback.push('\u{00A0}');
        runs.push(TextRun {
            len: '\u{00A0}'.len_utf8(),
            font: base_font.clone(),
            color: default_fg,
            background_color: None,
            underline: None,
            strikethrough: None,
        });
        fallback
    } else {
        text
    };

    CachedTerminalRow {
        text: text.into(),
        runs,
    }
}

/// Resolved per-cell style ready to emit as a `TextRun`.
struct RowStyle {
    font: Font,
    fg: Hsla,
    bg: Option<Hsla>,
    underline: Option<UnderlineStyle>,
    strikethrough: Option<StrikethroughStyle>,
}

impl RowStyle {
    fn from_cell(
        cell: &VtCell,
        default_fg: Hsla,
        default_bg: Hsla,
        base_font: &Font,
        is_cursor: bool,
    ) -> Self {
        let mut font = base_font.clone();
        if cell.attrs & ATTR_BOLD != 0 {
            font.weight = FontWeight::BOLD;
        }
        if cell.attrs & ATTR_ITALIC != 0 {
            font.style = FontStyle::Italic;
        }

        let mut fg = vt_color_to_hsla(cell.fg).unwrap_or(default_fg);
        let mut bg = vt_color_to_hsla(cell.bg);

        if cell.attrs & ATTR_INVERSE != 0 {
            let resolved_bg = bg.unwrap_or(default_bg);
            bg = Some(fg);
            fg = resolved_bg;
        }

        if is_cursor {
            // Block cursor (focused): swap the *currently resolved*
            // fg / bg so the glyph under the cursor stays legible,
            // like xterm and Ghostty's own default. Crucially we
            // operate on `fg` / `bg` (post `ATTR_INVERSE`) and *not*
            // on the raw `cell.fg` / `cell.bg`, so an inverse cell
            // under the cursor de-inverts to draw the block over
            // the already-inverted content rather than collapsing
            // into a single color and turning the glyph invisible
            // (selected rows in htop, vim status lines, less search
            // highlights all hit this exact case).
            let cursor_bg = fg;
            let cursor_fg = bg.unwrap_or(default_bg);
            fg = cursor_fg;
            bg = Some(cursor_bg);
        }

        let underline = if cell.attrs & ATTR_UNDERLINE != 0 {
            Some(UnderlineStyle {
                color: Some(fg),
                thickness: px(1.0),
                wavy: false,
            })
        } else {
            None
        };

        let strikethrough = if cell.attrs & ATTR_STRIKE != 0 {
            Some(StrikethroughStyle {
                color: Some(fg),
                thickness: px(1.0),
            })
        } else {
            None
        };

        Self {
            font,
            fg,
            bg,
            underline,
            strikethrough,
        }
    }
}

/// Decode the VT cell color (0xRRGGBBAA — alpha=0 means "default"
/// per `con-ghostty/src/vt.rs::read_cell`) into a GPUI `Hsla`.
fn vt_color_to_hsla(packed: u32) -> Option<Hsla> {
    let a = (packed & 0xFF) as u8;
    if a == 0 {
        return None;
    }
    let r = ((packed >> 24) & 0xFF) as f32 / 255.0;
    let g = ((packed >> 16) & 0xFF) as f32 / 255.0;
    let b = ((packed >> 8) & 0xFF) as f32 / 255.0;
    let a = a as f32 / 255.0;
    Some(Rgba { r, g, b, a }.into())
}

fn xterm_modifier_param(modifiers: &Modifiers) -> Option<u8> {
    let mask = u8::from(modifiers.shift)
        | (u8::from(modifiers.alt) << 1)
        | (u8::from(modifiers.control) << 2);
    if mask == 0 {
        None
    } else {
        Some(1 + mask)
    }
}

fn encode_special_key(key: &str, modifiers: &Modifiers, decckm: bool) -> Option<String> {
    let m = xterm_modifier_param(modifiers);

    let tilde = |code: u8| match m {
        Some(m) => format!("\x1b[{};{}~", code, m),
        None => format!("\x1b[{}~", code),
    };

    let csi1 = |final_byte: char, ss3_when_plain: bool, decckm_arrow: bool| match m {
        Some(m) => format!("\x1b[1;{}{}", m, final_byte),
        None if decckm_arrow && decckm => format!("\x1bO{}", final_byte),
        None if ss3_when_plain => format!("\x1bO{}", final_byte),
        None => format!("\x1b[{}", final_byte),
    };

    Some(match key {
        "up" => csi1('A', false, true),
        "down" => csi1('B', false, true),
        "right" => csi1('C', false, true),
        "left" => csi1('D', false, true),
        "home" => csi1('H', false, false),
        "end" => csi1('F', false, false),
        "pageup" => tilde(5),
        "pagedown" => tilde(6),
        "insert" => tilde(2),
        "delete" => tilde(3),
        "f1" => csi1('P', true, false),
        "f2" => csi1('Q', true, false),
        "f3" => csi1('R', true, false),
        "f4" => csi1('S', true, false),
        "f5" => tilde(15),
        "f6" => tilde(17),
        "f7" => tilde(18),
        "f8" => tilde(19),
        "f9" => tilde(20),
        "f10" => tilde(21),
        "f11" => tilde(23),
        "f12" => tilde(24),
        "enter" | "return" => "\r".into(),
        "escape" => "\x1b".into(),
        "backspace" if modifiers.alt && !modifiers.control && !modifiers.platform => {
            "\x1b\x7f".into()
        }
        "backspace" => "\x7f".into(),
        "tab" if modifiers.shift && !modifiers.control && !modifiers.platform => "\x1b[Z".into(),
        "tab" => "\t".into(),
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::{build_terminal_row, rows_needing_refresh, vt_color_to_hsla};
    use con_ghostty::{ScreenSnapshot, VtCell, VtCursor, ATTR_BOLD, ATTR_INVERSE, ATTR_UNDERLINE};
    use gpui::{Font, FontFeatures, FontStyle, FontWeight, Hsla, Rgba};

    fn base_font() -> Font {
        Font {
            family: "monospace".into(),
            features: FontFeatures::default(),
            fallbacks: None,
            weight: FontWeight::NORMAL,
            style: FontStyle::Normal,
        }
    }

    fn fg() -> Hsla {
        Rgba {
            r: 1.0,
            g: 1.0,
            b: 1.0,
            a: 1.0,
        }
        .into()
    }

    fn bg() -> Hsla {
        Rgba {
            r: 0.0,
            g: 0.0,
            b: 0.0,
            a: 1.0,
        }
        .into()
    }

    fn make_cell(ch: char, attrs: u8, fg: u32, bg: u32) -> VtCell {
        VtCell {
            codepoint: ch as u32,
            fg,
            bg,
            attrs,
            _pad: [0; 3],
        }
    }

    #[test]
    fn vt_color_zero_alpha_means_default() {
        assert_eq!(vt_color_to_hsla(0x000000_00), None);
        assert!(vt_color_to_hsla(0x112233_FF).is_some());
    }

    #[test]
    fn renders_row_without_panicking() {
        let cells = [
            make_cell('h', 0, 0, 0),
            make_cell('i', ATTR_BOLD | ATTR_UNDERLINE, 0xFF0000FF, 0),
            make_cell(' ', 0, 0, 0),
            make_cell('!', ATTR_INVERSE, 0, 0),
        ];
        let _no_cursor = build_terminal_row(&cells, fg(), bg(), &base_font(), None);
        let _with_cursor = build_terminal_row(&cells, fg(), bg(), &base_font(), Some(2));
    }

    #[test]
    fn refresh_rows_include_old_and_new_cursor_rows() {
        let snapshot = ScreenSnapshot {
            cols: 4,
            rows: 3,
            cells: vec![Default::default(); 12],
            dirty_rows: vec![1],
            cursor: VtCursor {
                col: 2,
                row: 2,
                visible: true,
            },
            title: None,
            generation: 7,
        };

        let mut rows = rows_needing_refresh(
            &snapshot,
            Some(VtCursor {
                col: 1,
                row: 0,
                visible: true,
            }),
            false,
        );
        rows.sort_unstable();
        rows.dedup();

        assert_eq!(rows, vec![0, 1, 2]);
    }
}
