use con_terminal::{CursorShape, Grid, Pty, PtyEvent, PtySize, TerminalTheme};
use gpui::*;
use parking_lot::Mutex;
use std::sync::Arc;
use vte::Parser;

use gpui_component::ActiveTheme;

#[derive(Clone, Copy, PartialEq)]
struct TextStyle {
    fg: u32,
    bold: bool,
    italic: bool,
    underline: bool,
    strikethrough: bool,
    dim: bool,
}

/// Terminal view — renders the grid and handles input.
///
/// Dynamically resizes the grid and PTY to fill available space.
/// The canvas prepaint callback measures the available bounds,
/// calculates cols/rows from cell dimensions, and resizes if
/// the terminal dimensions have changed.
/// Selection anchor point (row, col)
#[derive(Debug, Clone, Copy, PartialEq)]
struct SelectionPoint {
    row: usize,
    col: usize,
}

/// Active text selection
#[derive(Debug, Clone, Copy)]
struct Selection {
    start: SelectionPoint,
    end: SelectionPoint,
}

impl Selection {
    fn ordered(&self) -> (SelectionPoint, SelectionPoint) {
        if self.start.row < self.end.row
            || (self.start.row == self.end.row && self.start.col <= self.end.col)
        {
            (self.start, self.end)
        } else {
            (self.end, self.start)
        }
    }

    #[allow(dead_code)]
    fn contains(&self, row: usize, col: usize) -> bool {
        let (start, end) = self.ordered();
        if row < start.row || row > end.row {
            return false;
        }
        if row == start.row && row == end.row {
            return col >= start.col && col <= end.col;
        }
        if row == start.row {
            return col >= start.col;
        }
        if row == end.row {
            return col <= end.col;
        }
        true
    }
}

/// Emitted when the user clicks "Explain" on a command block
pub struct ExplainCommand {
    pub command: String,
    pub output: String,
}

/// Emitted when Ctrl+D is pressed — workspace should close the pane
pub struct ClosePaneRequest;

/// Emitted when the terminal gains focus (mouse click) so the workspace
/// can re-render and update pane focus tracking via sync_focus().
pub struct FocusChanged;

impl EventEmitter<ExplainCommand> for TerminalView {}
impl EventEmitter<ClosePaneRequest> for TerminalView {}
impl EventEmitter<FocusChanged> for TerminalView {}

/// Emitted when the current shell input changes (for suggestion engine).
pub struct InputChanged {
    pub input: Option<String>,
}

impl EventEmitter<InputChanged> for TerminalView {}

pub struct TerminalView {
    grid: Arc<Mutex<Grid>>,
    pty: Arc<Mutex<Pty>>,
    _parser: Arc<Mutex<Parser>>,
    focus_handle: FocusHandle,
    font_size: f32,
    cell_width: f32,
    cell_height: f32,
    selection: Option<Selection>,
    selecting: bool,
    terminal_origin: Arc<Mutex<(f32, f32)>>,
    cursor_blink_visible: bool,
    cursor_blink_epoch: std::time::Instant,
    hovered_row: Option<usize>,
    /// Last dimensions we resized to — prevents oscillation when canvas
    /// measures slightly different bounds on each render.
    last_resized_dims: Arc<Mutex<(usize, usize)>>,
    /// Ghost text suggestion shown after cursor (dim, Tab to accept)
    suggestion: Option<String>,
    /// Last input we sent to the suggestion engine (avoids re-requesting)
    last_suggestion_input: Option<String>,
}

impl TerminalView {
    #[allow(dead_code)]
    pub fn new(
        cols: usize,
        rows: usize,
        font_size: f32,
        scrollback_lines: usize,
        cx: &mut Context<Self>,
    ) -> Self {
        Self::with_options(cols, rows, font_size, scrollback_lines, &TerminalTheme::default(), None, cx)
    }

    pub fn with_theme(
        cols: usize,
        rows: usize,
        font_size: f32,
        scrollback_lines: usize,
        theme: &TerminalTheme,
        cx: &mut Context<Self>,
    ) -> Self {
        Self::with_options(cols, rows, font_size, scrollback_lines, theme, None, cx)
    }

    pub fn with_options(
        cols: usize,
        rows: usize,
        font_size: f32,
        scrollback_lines: usize,
        theme: &TerminalTheme,
        cwd: Option<&str>,
        cx: &mut Context<Self>,
    ) -> Self {
        let grid = Arc::new(Mutex::new(Grid::with_theme(cols, rows, scrollback_lines, theme)));
        let pty_size = PtySize {
            rows: rows as u16,
            cols: cols as u16,
        };
        // Try spawning with requested cwd; fall back to default ($HOME) if it fails
        let pty = match Pty::spawn_in(pty_size, cwd) {
            Ok(pty) => pty,
            Err(e) if cwd.is_some() => {
                log::warn!("PTY spawn failed in {:?}, falling back to $HOME: {}", cwd, e);
                Pty::spawn(pty_size).unwrap_or_else(|e2| {
                    panic!("Fatal: PTY spawn failed even with default cwd: {}", e2);
                })
            }
            Err(e) => panic!("Fatal: PTY spawn failed: {}", e),
        };
        let pty_events = pty.events().clone();
        let pty = Arc::new(Mutex::new(pty));
        let parser = Arc::new(Mutex::new(Parser::new()));

        let cell_width = font_size * 0.6;
        let cell_height = font_size * 1.4;

        // Spawn IO processing loop
        let grid_for_io = grid.clone();
        let parser_for_io = parser.clone();
        let pty_for_io = pty.clone();
        cx.spawn(async move |this, cx| {
            loop {
                match pty_events.try_recv() {
                    Ok(PtyEvent::Output(data)) => {
                        let mut grid = grid_for_io.lock();
                        let mut parser = parser_for_io.lock();
                        parser.advance(&mut *grid, &data);
                        drop(parser);
                        // Send any pending responses (DA, DSR) back to PTY
                        let responses = grid.drain_responses();
                        let current_input = grid.current_input();
                        drop(grid);
                        if !responses.is_empty() {
                            let mut pty = pty_for_io.lock();
                            for response in responses {
                                let _ = pty.write(&response);
                            }
                        }
                        this.update(cx, |view, cx| {
                            // Emit input change for suggestion engine
                            let changed = view.last_suggestion_input != current_input;
                            if changed {
                                view.last_suggestion_input = current_input.clone();
                                // Clear stale suggestion when input changes
                                view.suggestion = None;
                                cx.emit(InputChanged { input: current_input });
                            }
                            cx.notify();
                        }).ok();
                    }
                    Ok(PtyEvent::Exit(_)) => break,
                    Err(crossbeam_channel::TryRecvError::Empty) => {
                        cx.background_executor()
                            .timer(std::time::Duration::from_millis(4))
                            .await;
                    }
                    Err(crossbeam_channel::TryRecvError::Disconnected) => break,
                }
            }
        })
        .detach();

        // Cursor blink timer — triggers re-render every 500ms
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(std::time::Duration::from_millis(500))
                    .await;
                if this.update(cx, |_, cx| cx.notify()).is_err() {
                    break;
                }
            }
        })
        .detach();

        Self {
            grid,
            pty,
            _parser: parser,
            focus_handle: cx.focus_handle(),
            font_size,
            cell_width,
            cell_height,
            selection: None,
            selecting: false,
            terminal_origin: Arc::new(Mutex::new((0.0, 0.0))),
            cursor_blink_visible: true,
            cursor_blink_epoch: std::time::Instant::now(),
            hovered_row: None,
            last_resized_dims: Arc::new(Mutex::new((cols, rows))),
            suggestion: None,
            last_suggestion_input: None,
        }
    }

    pub fn grid(&self) -> &Arc<Mutex<Grid>> {
        &self.grid
    }

    pub fn title(&self) -> Option<String> {
        self.grid.lock().title.clone()
    }

    fn mouse_to_grid(&self, position: Point<Pixels>) -> SelectionPoint {
        let (ox, oy) = *self.terminal_origin.lock();
        let x: f32 = f32::from(position.x) - ox;
        let y: f32 = f32::from(position.y) - oy;
        let col = (x / self.cell_width).max(0.0) as usize;
        let row = (y / self.cell_height).max(0.0) as usize;
        SelectionPoint { row, col }
    }

    /// Expand selection to word boundaries at the given position
    fn select_word(&self, point: SelectionPoint) -> Selection {
        let grid = self.grid.lock();
        let row = point.row.min(grid.rows.saturating_sub(1));
        let cols = grid.cols;

        // Find word boundaries (alphanumeric + underscore)
        let is_word_char = |c: char| c.is_alphanumeric() || c == '_' || c == '-' || c == '.';

        let ch = grid.visible_cell(row, point.col.min(cols.saturating_sub(1))).c;
        let mut start_col = point.col.min(cols.saturating_sub(1));
        let mut end_col = start_col;

        if is_word_char(ch) {
            // Expand left
            while start_col > 0 && is_word_char(grid.visible_cell(row, start_col - 1).c) {
                start_col -= 1;
            }
            // Expand right
            while end_col + 1 < cols && is_word_char(grid.visible_cell(row, end_col + 1).c) {
                end_col += 1;
            }
        }

        Selection {
            start: SelectionPoint { row, col: start_col },
            end: SelectionPoint { row, col: end_col },
        }
    }

    fn selected_text(&self) -> Option<String> {
        let selection = self.selection?;
        let (start, end) = selection.ordered();
        let grid = self.grid.lock();
        let mut text = String::new();
        for row in start.row..=end.row.min(grid.rows - 1) {
            let col_start = if row == start.row { start.col } else { 0 };
            let col_end = if row == end.row {
                end.col.min(grid.cols - 1)
            } else {
                grid.cols - 1
            };
            for col in col_start..=col_end {
                text.push(grid.visible_cell(row, col).c);
            }
            if row != end.row {
                text.push('\n');
            }
        }
        // Trim trailing whitespace from each line
        let trimmed: Vec<&str> = text.lines().map(|l| l.trim_end()).collect();
        Some(trimmed.join("\n"))
    }

    pub fn write_to_pty(&self, data: &[u8]) {
        let mut pty = self.pty.lock();
        let _ = pty.write(data);
    }

    /// Set a ghost text suggestion to display after the cursor.
    /// Pass None to clear.
    pub fn set_suggestion(&mut self, suggestion: Option<String>, cx: &mut Context<Self>) {
        self.suggestion = suggestion;
        cx.notify();
    }
}

impl Focusable for TerminalView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for TerminalView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // Cursor blink: 500ms on, 500ms off. Reset on input.
        let elapsed = self.cursor_blink_epoch.elapsed().as_millis();
        self.cursor_blink_visible = (elapsed / 500) % 2 == 0;

        let grid = self.grid.lock();
        let rows = grid.rows;
        let cols = grid.cols;
        let cell_w = self.cell_width;
        let cell_h = self.cell_height;

        // The split container has overflow_hidden() so any text outside
        // the pane bounds is clipped at the CSS level. We render the full
        // grid — the canvas prepaint will resize on the next frame if the
        // available space differs from the grid dimensions.
        let render_cols = cols;
        let render_rows = rows;

        // Snapshot grid for rendering
        struct CellInfo {
            row: usize,
            col: usize,
            ch: char,
            fg: u32,
            bg: u32,
            bold: bool,
            italic: bool,
            underline: bool,
            strikethrough: bool,
            dim: bool,
        }

        let default_bg = grid.default_style().bg.to_u32();
        let mut cells: Vec<CellInfo> = Vec::new();
        let cursor_row = grid.cursor.row;
        let cursor_col = grid.cursor.col;
        let cursor_visible = grid.cursor.visible && self.cursor_blink_visible;
        let cursor_shape = grid.cursor.shape;

        for row in 0..render_rows {
            for col in 0..render_cols {
                let cell = grid.visible_cell(row, col);
                let (fg, bg) = if cell.style.inverse {
                    (cell.style.bg.to_u32(), cell.style.fg.to_u32())
                } else {
                    (cell.style.fg.to_u32(), cell.style.bg.to_u32())
                };
                if cell.c != ' ' || bg != default_bg {
                    cells.push(CellInfo {
                        row,
                        col,
                        ch: cell.c,
                        fg,
                        bg,
                        bold: cell.style.bold,
                        italic: cell.style.italic,
                        underline: cell.style.underline,
                        strikethrough: cell.style.strikethrough,
                        dim: cell.style.dim,
                    });
                }
            }
        }
        let scrollback_offset = grid.scrollback_offset;
        let in_scrollback = scrollback_offset > 0;
        let visible_blocks = grid.visible_command_blocks();
        let at_prompt = grid.at_shell_prompt();
        drop(grid);

        let suggestion_text = if at_prompt && !in_scrollback {
            self.suggestion.clone()
        } else {
            None
        };

        // Build text overlays using GPUI text engine
        let mut text_divs: Vec<Div> = Vec::new();
        let mut run_row = usize::MAX;
        let mut run_col = 0usize;
        let mut run_text = String::new();
        let mut run_style = TextStyle {
            fg: 0,
            bold: false,
            italic: false,
            underline: false,
            strikethrough: false,
            dim: false,
        };

        let font_sz = self.font_size;
        let flush_run =
            |divs: &mut Vec<Div>, row: usize, col: usize, text: &str, style: &TextStyle| {
                if !text.is_empty() {
                    divs.push(make_text_div(row, col, text, style, cell_w, cell_h, font_sz));
                }
            };

        for cell in &cells {
            let cell_style = TextStyle {
                fg: cell.fg,
                bold: cell.bold,
                italic: cell.italic,
                underline: cell.underline,
                strikethrough: cell.strikethrough,
                dim: cell.dim,
            };
            if cell.ch == ' ' {
                flush_run(&mut text_divs, run_row, run_col, &run_text, &run_style);
                run_text.clear();
                continue;
            }
            if cell.row != run_row
                || cell.col != run_col + run_text.len()
                || cell_style != run_style
            {
                flush_run(&mut text_divs, run_row, run_col, &run_text, &run_style);
                run_text.clear();
                run_row = cell.row;
                run_col = cell.col;
                run_style = cell_style;
            }
            run_text.push(cell.ch);
        }
        flush_run(&mut text_divs, run_row, run_col, &run_text, &run_style);

        // Canvas for backgrounds and cursor
        let cells_for_canvas: Vec<(usize, usize, u32)> = cells
            .iter()
            .filter(|c| c.bg != default_bg)
            .map(|c| (c.row, c.col, c.bg))
            .collect();

        let focus = self.focus_handle.clone();

        // Resize detection: capture handles for the canvas prepaint callback.
        // We compare against last_resized_dims (not current grid state) to
        // prevent oscillation when layout gives slightly different bounds on
        // each render cycle.
        let grid_for_resize = self.grid.clone();
        let pty_for_resize = self.pty.clone();
        let terminal_origin_for_canvas = self.terminal_origin.clone();
        let last_resized = self.last_resized_dims.clone();
        let entity_id = cx.entity_id();
        let selection_for_canvas = self.selection;
        let cursor_color = cx.theme().primary;
        let selection_color = cx.theme().selection;

        let mut terminal = div()
            .relative()
            .size_full()
            .bg(cx.theme().background)
            .track_focus(&self.focus_handle(cx))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                    window.focus(&focus, cx);
                    let point = this.mouse_to_grid(event.position);
                    if event.click_count >= 3 {
                        // Triple-click: select entire line
                        let grid = this.grid.lock();
                        let row = point.row.min(grid.rows.saturating_sub(1));
                        let cols = grid.cols;
                        drop(grid);
                        this.selection = Some(Selection {
                            start: SelectionPoint { row, col: 0 },
                            end: SelectionPoint { row, col: cols.saturating_sub(1) },
                        });
                        this.selecting = false;
                        if let Some(text) = this.selected_text() {
                            cx.write_to_clipboard(ClipboardItem::new_string(text));
                        }
                    } else if event.click_count == 2 {
                        // Double-click: select word
                        this.selection = Some(this.select_word(point));
                        this.selecting = false;
                        if let Some(text) = this.selected_text() {
                            cx.write_to_clipboard(ClipboardItem::new_string(text));
                        }
                    } else {
                        this.selection = Some(Selection {
                            start: point,
                            end: point,
                        });
                        this.selecting = true;
                    }
                    cx.emit(FocusChanged);
                    cx.notify();
                }),
            )
            .on_mouse_move(cx.listener(|this, event: &MouseMoveEvent, _window, cx| {
                if this.selecting {
                    let point = this.mouse_to_grid(event.position);
                    if let Some(ref mut sel) = this.selection {
                        sel.end = point;
                    }
                    cx.notify();
                }
                // Track hovered row for command block actions
                let point = this.mouse_to_grid(event.position);
                let new_row = Some(point.row);
                if this.hovered_row != new_row {
                    this.hovered_row = new_row;
                    cx.notify();
                }
            }))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, event: &MouseUpEvent, _window, cx| {
                    if this.selecting {
                        this.selecting = false;
                        let point = this.mouse_to_grid(event.position);
                        if let Some(ref mut sel) = this.selection {
                            sel.end = point;
                        }
                        // If start == end, clear selection (just a click)
                        if let Some(sel) = &this.selection {
                            if sel.start == sel.end {
                                this.selection = None;
                            }
                        }
                        // Auto-copy selection to clipboard
                        if let Some(text) = this.selected_text() {
                            cx.write_to_clipboard(ClipboardItem::new_string(text));
                        }
                        cx.notify();
                    }
                }),
            )
            .on_scroll_wheel(cx.listener(|this, event: &ScrollWheelEvent, _window, cx| {
                let delta_y = match event.delta {
                    ScrollDelta::Lines(delta) => -delta.y as isize,
                    ScrollDelta::Pixels(delta) => (-f32::from(delta.y) / 20.0) as isize,
                };
                if delta_y != 0 {
                    let mut grid = this.grid.lock();
                    if delta_y < 0 {
                        grid.scroll_viewport_up((-delta_y) as usize);
                    } else {
                        grid.scroll_viewport_down(delta_y as usize);
                    }
                    drop(grid);
                    cx.notify();
                }
            }))
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                if event.keystroke.modifiers.platform {
                    // Handle Cmd+V (paste) with bracketed paste support
                    if event.keystroke.key == "v" {
                        if let Some(text) = cx
                            .read_from_clipboard()
                            .and_then(|clip| clip.text())
                        {
                            if !text.is_empty() {
                                let bracketed = this.grid.lock().bracketed_paste;
                                if bracketed {
                                    this.write_to_pty(b"\x1b[200~");
                                }
                                this.write_to_pty(text.as_bytes());
                                if bracketed {
                                    this.write_to_pty(b"\x1b[201~");
                                }
                            }
                        }
                    }
                    // Cmd+K: clear scrollback and screen
                    if event.keystroke.key == "k" {
                        let mut grid = this.grid.lock();
                        grid.clear_scrollback();
                        drop(grid);
                        // Send clear command to shell
                        this.write_to_pty(b"\x0c"); // Form feed (Ctrl+L)
                        cx.notify();
                        return;
                    }
                    // Cmd+A: select all visible text
                    if event.keystroke.key == "a" {
                        let grid = this.grid.lock();
                        let rows = grid.rows;
                        let cols = grid.cols;
                        drop(grid);
                        this.selection = Some(Selection {
                            start: SelectionPoint { row: 0, col: 0 },
                            end: SelectionPoint {
                                row: rows.saturating_sub(1),
                                col: cols.saturating_sub(1),
                            },
                        });
                        if let Some(text) = this.selected_text() {
                            cx.write_to_clipboard(ClipboardItem::new_string(text));
                        }
                        cx.notify();
                        return;
                    }
                    // Cmd+C: copy selection if present, otherwise send SIGINT
                    if event.keystroke.key == "c" {
                        if let Some(text) = this.selected_text() {
                            cx.write_to_clipboard(ClipboardItem::new_string(text));
                            this.selection = None;
                            cx.notify();
                        } else {
                            this.write_to_pty(&[0x03]);
                        }
                    }
                    return;
                }
                let _ = window;
                // Reset cursor blink on keypress (stay visible while typing)
                this.cursor_blink_epoch = std::time::Instant::now();
                this.cursor_blink_visible = true;
                // Clear selection on any non-Cmd keypress
                if this.selection.is_some() {
                    this.selection = None;
                }
                // Snap back to live view when user types
                let (app_cursor_keys, kitty_flags);
                {
                    let mut grid = this.grid.lock();
                    app_cursor_keys = grid.application_cursor_keys;
                    kitty_flags = grid.kitty_keyboard_flags;
                    if grid.scrollback_offset > 0 {
                        grid.scrollback_offset = 0;
                        drop(grid);
                        cx.notify();
                    }
                }
                // Ctrl+D: close pane instead of sending EOT
                if event.keystroke.modifiers.control && event.keystroke.key == "d" {
                    cx.emit(ClosePaneRequest);
                    return;
                }

                // Tab with active suggestion: accept the ghost text
                if event.keystroke.key == "tab"
                    && !event.keystroke.modifiers.shift
                    && !event.keystroke.modifiers.control
                    && !event.keystroke.modifiers.alt
                {
                    if let Some(suggestion) = this.suggestion.take() {
                        this.last_suggestion_input = None;
                        this.write_to_pty(suggestion.as_bytes());
                        cx.notify();
                        return;
                    }
                }

                // Clear ghost text on any key that isn't Tab-accept
                if this.suggestion.is_some() {
                    this.suggestion = None;
                    this.last_suggestion_input = None;
                }

                let mods = con_terminal::input::Modifiers {
                    shift: event.keystroke.modifiers.shift,
                    ctrl: event.keystroke.modifiers.control,
                    alt: event.keystroke.modifiers.alt,
                    cmd: event.keystroke.modifiers.platform,
                };
                let key_name = &event.keystroke.key;
                // Use Kitty encoder when flags are active, fall back to standard
                let bytes = if kitty_flags > 0 {
                    con_terminal::InputEncoder::encode_key_kitty(key_name, mods)
                        .or_else(|| con_terminal::InputEncoder::encode_key(key_name, mods, app_cursor_keys))
                } else {
                    con_terminal::InputEncoder::encode_key(key_name, mods, app_cursor_keys)
                };
                if let Some(bytes) = bytes {
                    this.write_to_pty(&bytes);
                }
            }))
            // Background + cursor canvas (also handles resize detection)
            .child(
                canvas(
                    move |bounds, _window, cx| {
                        // Store the terminal origin for mouse coordinate conversion
                        *terminal_origin_for_canvas.lock() = (
                            f32::from(bounds.origin.x),
                            f32::from(bounds.origin.y),
                        );

                        // Detect if the available space requires a terminal resize.
                        let available_w: f32 = bounds.size.width.into();
                        let available_h: f32 = bounds.size.height.into();
                        let new_cols = (available_w / cell_w).max(1.0) as usize;
                        let new_rows = (available_h / cell_h).max(1.0) as usize;

                        // Skip degenerate sizes during layout settling
                        if new_cols < 4 || new_rows < 2 {
                            return;
                        }

                        let (last_cols, last_rows) = *last_resized.lock();

                        if new_cols != last_cols || new_rows != last_rows {
                            *last_resized.lock() = (new_cols, new_rows);

                            let mut grid = grid_for_resize.lock();
                            grid.resize(new_cols, new_rows);
                            drop(grid);

                            let pty = pty_for_resize.lock();
                            let _ = pty.resize(PtySize {
                                rows: new_rows as u16,
                                cols: new_cols as u16,
                            });
                            drop(pty);

                            // Trigger re-render on next frame with new dimensions
                            cx.notify(entity_id);
                        }
                    },
                    move |bounds, _, window, _cx| {
                        let origin = bounds.origin;

                        // Cell backgrounds
                        for &(row, col, bg) in &cells_for_canvas {
                            let x = origin.x + px(col as f32 * cell_w);
                            let y = origin.y + px(row as f32 * cell_h);
                            window.paint_quad(fill(
                                Bounds {
                                    origin: point(x, y),
                                    size: size(px(cell_w), px(cell_h)),
                                },
                                rgb(bg),
                            ));
                        }

                        // Selection highlight
                        if let Some(sel) = selection_for_canvas {
                            let (start, end) = sel.ordered();
                            for row in start.row..=end.row.min(rows.saturating_sub(1)) {
                                let col_start = if row == start.row { start.col } else { 0 };
                                let col_end = if row == end.row {
                                    end.col.min(cols.saturating_sub(1))
                                } else {
                                    cols.saturating_sub(1)
                                };
                                if col_start <= col_end {
                                    let x = origin.x + px(col_start as f32 * cell_w);
                                    let y = origin.y + px(row as f32 * cell_h);
                                    let w = (col_end - col_start + 1) as f32 * cell_w;
                                    window.paint_quad(fill(
                                        Bounds {
                                            origin: point(x, y),
                                            size: size(px(w), px(cell_h)),
                                        },
                                        selection_color,
                                    ));
                                }
                            }
                        }

                        // Cursor (hidden when scrolled into scrollback)
                        if cursor_visible && !in_scrollback && cursor_row < rows && cursor_col < cols {
                            let cx_pos = origin.x + px(cursor_col as f32 * cell_w);
                            let cy_pos = origin.y + px(cursor_row as f32 * cell_h);
                            let (w, h) = match cursor_shape {
                                CursorShape::Block => (px(cell_w), px(cell_h)),
                                CursorShape::Bar => (px(2.0), px(cell_h)),
                                CursorShape::Underline => (px(cell_w), px(2.0)),
                            };
                            let y_offset = match cursor_shape {
                                CursorShape::Underline => cy_pos + px(cell_h - 2.0),
                                _ => cy_pos,
                            };
                            window.paint_quad(fill(
                                Bounds {
                                    origin: point(cx_pos, y_offset),
                                    size: size(w, h),
                                },
                                cursor_color,
                            ));
                        }
                    },
                )
                .size_full(),
            )
            // Text overlays
            .children(text_divs);

        // Ghost text suggestion — dim text after cursor, Tab to accept
        if let Some(ref suggestion) = suggestion_text {
            if cursor_row < render_rows && cursor_col < render_cols {
                let ghost = div()
                    .absolute()
                    .top(px(cursor_row as f32 * cell_h))
                    .left(px(cursor_col as f32 * cell_w))
                    .font_family("Ioskeley Mono")
                    .text_size(px(font_sz))
                    .line_height(px(cell_h))
                    .text_color(cx.theme().muted_foreground.opacity(0.4))
                    .child(suggestion.clone());
                terminal = terminal.child(ghost);
            }
        }

        // Command block action bar — appears when hovering near a command block
        if let Some(hovered_row) = self.hovered_row {
            let theme = cx.theme();
            for block in &visible_blocks {
                // Show action bar if hovering within 1 row of the block's prompt row
                if hovered_row <= block.viewport_row + 1
                    && hovered_row + 2 >= block.viewport_row
                {
                    let exit_color = match block.exit_code {
                        Some(0) => theme.success,
                        Some(_) => theme.danger,
                        None => theme.muted_foreground,
                    };

                    let cmd_for_rerun = block.command.clone();
                    let cmd_for_explain = block.command.clone();
                    let output_start = block.output_start_row;
                    let output_end = block.output_end_row;
                    let grid_for_copy = self.grid.clone();
                    let grid_for_explain = self.grid.clone();

                    let action_bar = div()
                        .absolute()
                        .top(px(block.viewport_row as f32 * cell_h))
                        .right(px(8.0))
                        .flex()
                        .items_center()
                        .gap(px(4.0))
                        .px(px(6.0))
                        .py(px(2.0))
                        .rounded(px(6.0))
                        .bg(theme.title_bar.opacity(0.95))
                        .border_1()
                        .border_color(theme.border)
                        .shadow_sm()
                        // Exit code dot
                        .child(
                            div()
                                .size(px(6.0))
                                .rounded_full()
                                .bg(exit_color),
                        )
                        // Copy output
                        .child(
                            div()
                                .id(SharedString::from(format!(
                                    "cmd-copy-{}",
                                    block.viewport_row
                                )))
                                .cursor_pointer()
                                .p(px(3.0))
                                .rounded(px(4.0))
                                .hover(|s| s.bg(theme.secondary.opacity(0.5)))
                                .on_mouse_down(
                                    MouseButton::Left,
                                    cx.listener(move |this, _, _, cx| {
                                        let output = grid_for_copy
                                            .lock()
                                            .command_block_output(output_start, output_end);
                                        cx.write_to_clipboard(
                                            ClipboardItem::new_string(output),
                                        );
                                        let _ = this;
                                    }),
                                )
                                .child(
                                    svg()
                                        .path("phosphor/copy.svg")
                                        .size(px(12.0))
                                        .text_color(theme.muted_foreground),
                                ),
                        )
                        // Re-run
                        .child(
                            div()
                                .id(SharedString::from(format!(
                                    "cmd-rerun-{}",
                                    block.viewport_row
                                )))
                                .cursor_pointer()
                                .p(px(3.0))
                                .rounded(px(4.0))
                                .hover(|s| s.bg(theme.secondary.opacity(0.5)))
                                .on_mouse_down(
                                    MouseButton::Left,
                                    cx.listener(move |this, _, _, cx| {
                                        this.write_to_pty(
                                            format!("{}\n", cmd_for_rerun).as_bytes(),
                                        );
                                        cx.notify();
                                    }),
                                )
                                .child(
                                    svg()
                                        .path("phosphor/arrow-clockwise.svg")
                                        .size(px(12.0))
                                        .text_color(theme.muted_foreground),
                                ),
                        )
                        // Explain
                        .child(
                            div()
                                .id(SharedString::from(format!(
                                    "cmd-explain-{}",
                                    block.viewport_row
                                )))
                                .cursor_pointer()
                                .p(px(3.0))
                                .rounded(px(4.0))
                                .hover(|s| s.bg(theme.secondary.opacity(0.5)))
                                .on_mouse_down(
                                    MouseButton::Left,
                                    cx.listener(move |this, _, _, cx| {
                                        let output = grid_for_explain
                                            .lock()
                                            .command_block_output(output_start, output_end);
                                        cx.emit(ExplainCommand {
                                            command: cmd_for_explain.clone(),
                                            output,
                                        });
                                        let _ = this;
                                    }),
                                )
                                .child(
                                    svg()
                                        .path("phosphor/chat-circle.svg")
                                        .size(px(12.0))
                                        .text_color(theme.muted_foreground),
                                ),
                        );

                    terminal = terminal.child(action_bar);
                    break; // Only show one action bar at a time
                }
            }
        }

        // Scrollback indicator — floating pill when viewing history
        if in_scrollback {
            let theme = cx.theme();
            terminal = terminal.child(
                div()
                    .absolute()
                    .bottom(px(12.0))
                    .left_0()
                    .right_0()
                    .flex()
                    .justify_center()
                    .child(
                        div()
                            .px(px(12.0))
                            .py(px(5.0))
                            .rounded(px(14.0))
                            .bg(theme.title_bar.opacity(0.95))
                            .border_1()
                            .border_color(theme.border)
                            .shadow_sm()
                            .text_size(px(11.0))
                            .text_color(theme.muted_foreground)
                            .child(format!("{} lines up", scrollback_offset)),
                    ),
            );
        }

        terminal
    }
}

fn make_text_div(
    row: usize,
    col: usize,
    text: &str,
    style: &TextStyle,
    cell_w: f32,
    cell_h: f32,
    font_size: f32,
) -> Div {
    let color = if style.dim {
        rgba((style.fg << 8) | 0x80) // 50% opacity
    } else {
        rgb(style.fg)
    };
    let mut d = div()
        .absolute()
        .top(px(row as f32 * cell_h))
        .left(px(col as f32 * cell_w))
        .font_family("Ioskeley Mono")
        .text_size(px(font_size))
        .line_height(px(cell_h))
        .text_color(color);
    if style.bold {
        d = d.font_weight(FontWeight::BOLD);
    }
    if style.italic {
        d = d.italic();
    }
    if style.underline {
        d = d.underline();
    }
    if style.strikethrough {
        d = d.line_through();
    }
    d.child(text.to_string())
}
