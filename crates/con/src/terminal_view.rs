use con_terminal::{CursorShape, Grid, Pty, PtyEvent, PtySize};
use gpui::*;
use parking_lot::Mutex;
use std::sync::Arc;
use vte::Parser;

use gpui_component::ActiveTheme;

/// Terminal view — renders the grid and handles input
pub struct TerminalView {
    grid: Arc<Mutex<Grid>>,
    pty: Arc<Mutex<Pty>>,
    _parser: Arc<Mutex<Parser>>,
    focus_handle: FocusHandle,
    cell_width: f32,
    cell_height: f32,
}

impl TerminalView {
    pub fn new(cols: usize, rows: usize, cx: &mut Context<Self>) -> Self {
        let grid = Arc::new(Mutex::new(Grid::new(cols, rows)));
        let pty = Pty::spawn(PtySize {
            rows: rows as u16,
            cols: cols as u16,
        })
        .expect("Failed to spawn PTY");
        let pty_events = pty.events().clone();
        let pty = Arc::new(Mutex::new(pty));
        let parser = Arc::new(Mutex::new(Parser::new()));

        let cell_width = 14.0 * 0.6;
        let cell_height = 14.0 * 1.4;

        // Spawn IO processing loop
        let grid_for_io = grid.clone();
        let parser_for_io = parser.clone();
        cx.spawn(async move |this, cx| {
            loop {
                match pty_events.try_recv() {
                    Ok(PtyEvent::Output(data)) => {
                        let mut grid = grid_for_io.lock();
                        let mut parser = parser_for_io.lock();
                        parser.advance(&mut *grid, &data);
                        drop(grid);
                        drop(parser);
                        this.update(cx, |_, cx| cx.notify()).ok();
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

        Self {
            grid,
            pty,
            _parser: parser,
            focus_handle: cx.focus_handle(),
            cell_width,
            cell_height,
        }
    }

    pub fn grid(&self) -> &Arc<Mutex<Grid>> {
        &self.grid
    }

    pub fn write_to_pty(&self, data: &[u8]) {
        let mut pty = self.pty.lock();
        let _ = pty.write(data);
    }
}

impl Focusable for TerminalView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for TerminalView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let grid = self.grid.lock();
        let rows = grid.rows;
        let cols = grid.cols;
        let cell_w = self.cell_width;
        let cell_h = self.cell_height;

        // Snapshot grid for rendering
        struct CellInfo {
            row: usize,
            col: usize,
            ch: char,
            fg: u32,
            bg: u32,
            bold: bool,
        }

        let default_bg = con_terminal::Style::default().bg.to_u32();
        let mut cells: Vec<CellInfo> = Vec::new();
        let cursor_row = grid.cursor.row;
        let cursor_col = grid.cursor.col;
        let cursor_visible = grid.cursor.visible;
        let cursor_shape = grid.cursor.shape;

        for row in 0..rows {
            for col in 0..cols {
                let cell = grid.cell(row, col);
                let bg = cell.style.bg.to_u32();
                if cell.c != ' ' || bg != default_bg {
                    cells.push(CellInfo {
                        row,
                        col,
                        ch: cell.c,
                        fg: cell.style.fg.to_u32(),
                        bg,
                        bold: cell.style.bold,
                    });
                }
            }
        }
        drop(grid);

        // Build text overlays using GPUI text engine
        let mut text_divs: Vec<Div> = Vec::new();
        let mut run_row = usize::MAX;
        let mut run_col = 0usize;
        let mut run_text = String::new();
        let mut run_fg: u32 = 0;
        let mut run_bold = false;

        for cell in &cells {
            if cell.ch == ' ' {
                if !run_text.is_empty() {
                    text_divs.push(make_text_div(
                        run_row, run_col, &run_text, run_fg, run_bold, cell_w, cell_h,
                    ));
                    run_text.clear();
                }
                continue;
            }
            if cell.row != run_row
                || cell.col != run_col + run_text.len()
                || cell.fg != run_fg
                || cell.bold != run_bold
            {
                if !run_text.is_empty() {
                    text_divs.push(make_text_div(
                        run_row, run_col, &run_text, run_fg, run_bold, cell_w, cell_h,
                    ));
                    run_text.clear();
                }
                run_row = cell.row;
                run_col = cell.col;
                run_fg = cell.fg;
                run_bold = cell.bold;
            }
            run_text.push(cell.ch);
        }
        if !run_text.is_empty() {
            text_divs.push(make_text_div(
                run_row, run_col, &run_text, run_fg, run_bold, cell_w, cell_h,
            ));
        }

        // Canvas for backgrounds and cursor
        let cells_for_canvas: Vec<(usize, usize, u32)> = cells
            .iter()
            .filter(|c| c.bg != default_bg)
            .map(|c| (c.row, c.col, c.bg))
            .collect();

        let focus = self.focus_handle.clone();

        div()
            .relative()
            .size_full()
            .bg(cx.theme().background)
            .track_focus(&self.focus_handle(cx))
            .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                window.focus(&focus, cx);
            })
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, _window, _cx| {
                if event.keystroke.modifiers.platform {
                    return; // Don't send Cmd+key to PTY
                }
                let mods = con_terminal::input::Modifiers {
                    shift: event.keystroke.modifiers.shift,
                    ctrl: event.keystroke.modifiers.control,
                    alt: event.keystroke.modifiers.alt,
                    cmd: event.keystroke.modifiers.platform,
                };
                let key_name = &event.keystroke.key;
                if let Some(bytes) = con_terminal::InputEncoder::encode_key(key_name, mods) {
                    this.write_to_pty(&bytes);
                }
            }))
            // Background + cursor canvas
            .child(
                canvas(
                    move |_bounds, _window, _cx| {},
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

                        // Cursor
                        if cursor_visible && cursor_row < rows && cursor_col < cols {
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
                                rgba(0xb4befe80),
                            ));
                        }
                    },
                )
                .size_full(),
            )
            // Text overlays
            .children(text_divs)
    }
}

fn make_text_div(
    row: usize,
    col: usize,
    text: &str,
    fg: u32,
    bold: bool,
    cell_w: f32,
    cell_h: f32,
) -> Div {
    let mut d = div()
        .absolute()
        .top(px(row as f32 * cell_h))
        .left(px(col as f32 * cell_w))
        .text_sm()
        .text_color(rgb(fg));
    if bold {
        d = d.font_weight(FontWeight::BOLD);
    }
    d.child(text.to_string())
}
