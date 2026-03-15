use vte::{Params, Perform};

/// RGBA color
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Color {
    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b, a: 255 }
    }

    pub fn to_u32(self) -> u32 {
        ((self.r as u32) << 16) | ((self.g as u32) << 8) | (self.b as u32)
    }
}

/// Default terminal palette (ANSI 256)
fn default_palette() -> [Color; 256] {
    let mut palette = [Color::rgb(0, 0, 0); 256];

    // Standard 16 colors
    palette[0] = Color::rgb(0x1e, 0x1e, 0x2e); // black (catppuccin base)
    palette[1] = Color::rgb(0xf3, 0x8b, 0xa8); // red
    palette[2] = Color::rgb(0xa6, 0xe3, 0xa1); // green
    palette[3] = Color::rgb(0xf9, 0xe2, 0xaf); // yellow
    palette[4] = Color::rgb(0x89, 0xb4, 0xfa); // blue
    palette[5] = Color::rgb(0xcb, 0xa6, 0xf7); // magenta
    palette[6] = Color::rgb(0x94, 0xe2, 0xd5); // cyan
    palette[7] = Color::rgb(0xcd, 0xd6, 0xf4); // white
    palette[8] = Color::rgb(0x58, 0x5b, 0x70); // bright black
    palette[9] = Color::rgb(0xf3, 0x8b, 0xa8); // bright red
    palette[10] = Color::rgb(0xa6, 0xe3, 0xa1); // bright green
    palette[11] = Color::rgb(0xf9, 0xe2, 0xaf); // bright yellow
    palette[12] = Color::rgb(0x89, 0xb4, 0xfa); // bright blue
    palette[13] = Color::rgb(0xcb, 0xa6, 0xf7); // bright magenta
    palette[14] = Color::rgb(0x94, 0xe2, 0xd5); // bright cyan
    palette[15] = Color::rgb(0xcd, 0xd6, 0xf4); // bright white

    // 216 color cube (indices 16-231)
    for i in 0..216 {
        let r = (i / 36) as u8;
        let g = ((i / 6) % 6) as u8;
        let b = (i % 6) as u8;
        palette[16 + i] = Color::rgb(
            if r > 0 { 55 + r * 40 } else { 0 },
            if g > 0 { 55 + g * 40 } else { 0 },
            if b > 0 { 55 + b * 40 } else { 0 },
        );
    }

    // Grayscale ramp (indices 232-255)
    for i in 0..24 {
        let v = (8 + i * 10) as u8;
        palette[232 + i] = Color::rgb(v, v, v);
    }

    palette
}

/// Text style attributes
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Style {
    pub fg: Color,
    pub bg: Color,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub strikethrough: bool,
    pub inverse: bool,
    pub dim: bool,
}

impl Default for Style {
    fn default() -> Self {
        Self {
            fg: Color::rgb(0xcd, 0xd6, 0xf4), // catppuccin text
            bg: Color::rgb(0x1e, 0x1e, 0x2e),  // catppuccin base
            bold: false,
            italic: false,
            underline: false,
            strikethrough: false,
            inverse: false,
            dim: false,
        }
    }
}

/// A single terminal cell
#[derive(Debug, Clone, PartialEq)]
pub struct Cell {
    pub c: char,
    pub style: Style,
    pub wide: bool,
}

impl Default for Cell {
    fn default() -> Self {
        Self {
            c: ' ',
            style: Style::default(),
            wide: false,
        }
    }
}

/// A completed command block detected via OSC 133 shell integration
#[derive(Debug, Clone)]
pub struct CommandBlock {
    pub command: String,
    pub output_start_row: usize,
    pub output_end_row: usize,
    pub exit_code: Option<i32>,
}

/// Cursor shape
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CursorShape {
    Block,
    Underline,
    Bar,
}

/// Cursor state
#[derive(Debug, Clone, Copy)]
pub struct Cursor {
    pub row: usize,
    pub col: usize,
    pub shape: CursorShape,
    pub visible: bool,
}

/// Terminal grid — holds the screen state
pub struct Grid {
    pub cols: usize,
    pub rows: usize,
    cells: Vec<Vec<Cell>>,
    scrollback: Vec<Vec<Cell>>,
    pub scrollback_offset: usize,
    pub cursor: Cursor,
    style: Style,
    saved_cursor: Option<Cursor>,
    palette: [Color; 256],
    alternate_screen: Option<Vec<Vec<Cell>>>,
    dirty: Vec<bool>,
    // OSC 133 semantic prompt tracking
    pub last_prompt_row: Option<usize>,
    pub last_command: Option<String>,
    pub last_exit_code: Option<i32>,
    pub current_dir: Option<String>,
    /// Completed command blocks from OSC 133 sequences
    pub command_blocks: Vec<CommandBlock>,
    /// Row where the current command started (OSC 133;C)
    command_start_row: Option<usize>,
    // Scroll region
    scroll_top: usize,
    scroll_bottom: usize,
    // Tab stops
    tab_stops: Vec<bool>,
    // DEC private modes
    /// DECCKM: application cursor keys mode (arrow keys send SS3 instead of CSI)
    pub application_cursor_keys: bool,
    /// Bracketed paste mode (2004)
    pub bracketed_paste: bool,
    /// Auto-wrap mode (DECAWM, mode 7)
    auto_wrap: bool,
    /// Window/tab title from OSC 0/1/2
    pub title: Option<String>,
    /// Pending responses to write back to the PTY (e.g., DA, DSR)
    pending_responses: Vec<Vec<u8>>,
    /// Maximum scrollback lines (configurable)
    max_scrollback: usize,
}

impl Grid {
    pub fn new(cols: usize, rows: usize) -> Self {
        Self::with_scrollback(cols, rows, 10_000)
    }

    pub fn with_scrollback(cols: usize, rows: usize, max_scrollback: usize) -> Self {
        let cells = vec![vec![Cell::default(); cols]; rows];
        let dirty = vec![true; rows];
        let mut tab_stops = vec![false; cols];
        for i in (0..cols).step_by(8) {
            tab_stops[i] = true;
        }
        Self {
            cols,
            rows,
            cells,
            scrollback: Vec::new(),
            scrollback_offset: 0,
            cursor: Cursor {
                row: 0,
                col: 0,
                shape: CursorShape::Block,
                visible: true,
            },
            style: Style::default(),
            saved_cursor: None,
            palette: default_palette(),
            alternate_screen: None,
            dirty,
            last_prompt_row: None,
            last_command: None,
            last_exit_code: None,
            current_dir: None,
            command_blocks: Vec::new(),
            command_start_row: None,
            scroll_top: 0,
            scroll_bottom: rows - 1,
            tab_stops,
            application_cursor_keys: false,
            bracketed_paste: false,
            auto_wrap: true,
            title: None,
            pending_responses: Vec::new(),
            max_scrollback,
        }
    }

    pub fn cell(&self, row: usize, col: usize) -> &Cell {
        &self.cells[row][col]
    }

    pub fn is_dirty(&self, row: usize) -> bool {
        self.dirty[row]
    }

    /// Extract the text content of a single row, trimming trailing whitespace.
    pub fn row_text(&self, row: usize) -> String {
        if row >= self.rows {
            return String::new();
        }
        let text: String = self.cells[row].iter().map(|c| c.c).collect();
        text.trim_end().to_string()
    }

    /// Get the cell at a given viewport position, accounting for scrollback offset.
    /// When scrollback_offset > 0, rows near the top show scrollback content.
    pub fn visible_cell(&self, row: usize, col: usize) -> &Cell {
        if self.scrollback_offset == 0 {
            return self.cell(row, col);
        }
        let scrollback_row = self.scrollback.len() as isize - self.scrollback_offset as isize + row as isize;
        if scrollback_row >= 0 && (scrollback_row as usize) < self.scrollback.len() {
            let sb_row = scrollback_row as usize;
            if col < self.scrollback[sb_row].len() {
                return &self.scrollback[sb_row][col];
            }
        }
        // Fall through to active screen for rows below scrollback
        let active_row = row as isize - self.scrollback_offset as isize;
        if active_row >= 0 && (active_row as usize) < self.rows {
            return self.cell(active_row as usize, col);
        }
        // Out of bounds — return default
        static DEFAULT_CELL: Cell = Cell {
            c: ' ',
            style: Style {
                fg: Color { r: 204, g: 204, b: 204, a: 255 },
                bg: Color { r: 28, g: 27, b: 25, a: 255 },
                bold: false,
                italic: false,
                underline: false,
                strikethrough: false,
                inverse: false,
                dim: false,
            },
            wide: false,
        };
        &DEFAULT_CELL
    }

    /// Scroll up into scrollback by `lines` lines.
    pub fn scroll_viewport_up(&mut self, lines: usize) {
        let max_offset = self.scrollback.len();
        self.scrollback_offset = (self.scrollback_offset + lines).min(max_offset);
    }

    /// Scroll down toward live content by `lines` lines.
    pub fn scroll_viewport_down(&mut self, lines: usize) {
        self.scrollback_offset = self.scrollback_offset.saturating_sub(lines);
    }

    /// Number of scrollback lines available.
    pub fn scrollback_len(&self) -> usize {
        self.scrollback.len()
    }

    /// Drain any pending PTY responses (DA, DSR, etc.).
    /// The caller should write these bytes back to the PTY.
    pub fn drain_responses(&mut self) -> Vec<Vec<u8>> {
        std::mem::take(&mut self.pending_responses)
    }

    pub fn clear_dirty(&mut self) {
        for d in &mut self.dirty {
            *d = false;
        }
    }

    pub fn mark_all_dirty(&mut self) {
        for d in &mut self.dirty {
            *d = true;
        }
    }

    pub fn resize(&mut self, new_cols: usize, new_rows: usize) {
        // Reflow cells into new dimensions
        let mut new_cells = vec![vec![Cell::default(); new_cols]; new_rows];
        let copy_rows = new_rows.min(self.rows);
        let copy_cols = new_cols.min(self.cols);
        for row in 0..copy_rows {
            for col in 0..copy_cols {
                new_cells[row][col] = self.cells[row][col].clone();
            }
        }
        self.cells = new_cells;
        self.cols = new_cols;
        self.rows = new_rows;
        self.dirty = vec![true; new_rows];
        self.scroll_bottom = new_rows - 1;
        self.cursor.row = self.cursor.row.min(new_rows - 1);
        self.cursor.col = self.cursor.col.min(new_cols - 1);
        self.tab_stops = vec![false; new_cols];
        for i in (0..new_cols).step_by(8) {
            self.tab_stops[i] = true;
        }
    }

    /// Get lines of text content from the terminal (for agent context)
    pub fn content_lines(&self, max_lines: usize) -> Vec<String> {
        let mut lines = Vec::new();
        let start = if self.rows > max_lines {
            self.rows - max_lines
        } else {
            0
        };
        for row in start..self.rows {
            let line: String = self.cells[row].iter().map(|c| c.c).collect();
            lines.push(line.trim_end().to_string());
        }
        // Trim trailing empty lines
        while lines.last().is_some_and(|l| l.is_empty()) {
            lines.pop();
        }
        lines
    }

    fn scroll_up(&mut self) {
        // Move top line to scrollback
        let row = self.cells[self.scroll_top].clone();
        self.scrollback.push(row);
        // Limit scrollback
        if self.scrollback.len() > self.max_scrollback {
            self.scrollback.remove(0);
        }
        // Shift lines up within scroll region
        for i in self.scroll_top..self.scroll_bottom {
            self.cells[i] = self.cells[i + 1].clone();
            self.dirty[i] = true;
        }
        self.cells[self.scroll_bottom] = vec![Cell::default(); self.cols];
        self.dirty[self.scroll_bottom] = true;
    }

    fn scroll_down(&mut self) {
        for i in (self.scroll_top + 1..=self.scroll_bottom).rev() {
            self.cells[i] = self.cells[i - 1].clone();
            self.dirty[i] = true;
        }
        self.cells[self.scroll_top] = vec![Cell::default(); self.cols];
        self.dirty[self.scroll_top] = true;
    }

    fn newline(&mut self) {
        if self.cursor.row == self.scroll_bottom {
            self.scroll_up();
        } else if self.cursor.row < self.rows - 1 {
            self.cursor.row += 1;
        }
    }

    fn set_cell(&mut self, c: char) {
        if self.cursor.col >= self.cols {
            self.cursor.col = 0;
            self.newline();
        }
        let row = self.cursor.row;
        let col = self.cursor.col;
        self.cells[row][col] = Cell {
            c,
            style: self.style,
            wide: false,
        };
        self.dirty[row] = true;
        self.cursor.col += 1;
    }

    fn erase_in_display(&mut self, mode: u16) {
        match mode {
            0 => {
                // Erase from cursor to end
                for col in self.cursor.col..self.cols {
                    self.cells[self.cursor.row][col] = Cell::default();
                }
                self.dirty[self.cursor.row] = true;
                for row in (self.cursor.row + 1)..self.rows {
                    self.cells[row] = vec![Cell::default(); self.cols];
                    self.dirty[row] = true;
                }
            }
            1 => {
                // Erase from start to cursor
                for row in 0..self.cursor.row {
                    self.cells[row] = vec![Cell::default(); self.cols];
                    self.dirty[row] = true;
                }
                for col in 0..=self.cursor.col.min(self.cols - 1) {
                    self.cells[self.cursor.row][col] = Cell::default();
                }
                self.dirty[self.cursor.row] = true;
            }
            2 | 3 => {
                // Erase entire display
                for row in 0..self.rows {
                    self.cells[row] = vec![Cell::default(); self.cols];
                    self.dirty[row] = true;
                }
            }
            _ => {}
        }
    }

    fn erase_in_line(&mut self, mode: u16) {
        let row = self.cursor.row;
        match mode {
            0 => {
                for col in self.cursor.col..self.cols {
                    self.cells[row][col] = Cell::default();
                }
            }
            1 => {
                for col in 0..=self.cursor.col.min(self.cols - 1) {
                    self.cells[row][col] = Cell::default();
                }
            }
            2 => {
                self.cells[row] = vec![Cell::default(); self.cols];
            }
            _ => {}
        }
        self.dirty[row] = true;
    }

    fn parse_sgr(&mut self, params: &Params) {
        let mut iter = params.iter();
        loop {
            let param = match iter.next() {
                Some(p) => p,
                None => break,
            };
            let code = param[0] as u16;
            match code {
                0 => self.style = Style::default(),
                1 => self.style.bold = true,
                2 => self.style.dim = true,
                3 => self.style.italic = true,
                4 => self.style.underline = true,
                7 => self.style.inverse = true,
                9 => self.style.strikethrough = true,
                22 => {
                    self.style.bold = false;
                    self.style.dim = false;
                }
                23 => self.style.italic = false,
                24 => self.style.underline = false,
                27 => self.style.inverse = false,
                29 => self.style.strikethrough = false,
                30..=37 => self.style.fg = self.palette[(code - 30) as usize],
                38 => {
                    // Extended foreground
                    if let Some(next) = iter.next() {
                        match next[0] as u16 {
                            5 => {
                                if let Some(idx) = iter.next() {
                                    let i = (idx[0] as usize).min(255);
                                    self.style.fg = self.palette[i];
                                }
                            }
                            2 => {
                                let r = iter.next().map(|p| p[0] as u8).unwrap_or(0);
                                let g = iter.next().map(|p| p[0] as u8).unwrap_or(0);
                                let b = iter.next().map(|p| p[0] as u8).unwrap_or(0);
                                self.style.fg = Color::rgb(r, g, b);
                            }
                            _ => {}
                        }
                    }
                }
                39 => self.style.fg = Style::default().fg,
                40..=47 => self.style.bg = self.palette[(code - 40) as usize],
                48 => {
                    // Extended background
                    if let Some(next) = iter.next() {
                        match next[0] as u16 {
                            5 => {
                                if let Some(idx) = iter.next() {
                                    let i = (idx[0] as usize).min(255);
                                    self.style.bg = self.palette[i];
                                }
                            }
                            2 => {
                                let r = iter.next().map(|p| p[0] as u8).unwrap_or(0);
                                let g = iter.next().map(|p| p[0] as u8).unwrap_or(0);
                                let b = iter.next().map(|p| p[0] as u8).unwrap_or(0);
                                self.style.bg = Color::rgb(r, g, b);
                            }
                            _ => {}
                        }
                    }
                }
                49 => self.style.bg = Style::default().bg,
                90..=97 => self.style.fg = self.palette[(code - 90 + 8) as usize],
                100..=107 => self.style.bg = self.palette[(code - 100 + 8) as usize],
                _ => {}
            }
        }
    }
}

impl Perform for Grid {
    fn print(&mut self, c: char) {
        self.set_cell(c);
    }

    fn execute(&mut self, byte: u8) {
        match byte {
            0x07 => {} // BEL
            0x08 => {
                // BS
                if self.cursor.col > 0 {
                    self.cursor.col -= 1;
                }
            }
            0x09 => {
                // HT (tab)
                let next_tab = (self.cursor.col + 1..self.cols)
                    .find(|&i| self.tab_stops.get(i).copied().unwrap_or(false))
                    .unwrap_or(self.cols - 1);
                self.cursor.col = next_tab;
            }
            0x0A | 0x0B | 0x0C => {
                // LF, VT, FF
                self.newline();
            }
            0x0D => {
                // CR
                self.cursor.col = 0;
            }
            _ => {}
        }
    }

    fn hook(&mut self, _params: &Params, _intermediates: &[u8], _ignore: bool, _action: char) {}

    fn put(&mut self, _byte: u8) {}

    fn unhook(&mut self) {}

    fn osc_dispatch(&mut self, params: &[&[u8]], _bell_terminated: bool) {
        if params.is_empty() {
            return;
        }

        match params[0] {
            // OSC 0, 1, 2: Window title
            b"0" | b"1" | b"2" => {
                if params.len() > 1 {
                    self.title = Some(String::from_utf8_lossy(params[1]).to_string());
                }
            }
            // OSC 7: Current directory
            b"7" => {
                if params.len() > 1 {
                    let uri = String::from_utf8_lossy(params[1]);
                    // file://hostname/path format
                    if let Some(path) = uri.strip_prefix("file://") {
                        if let Some(slash_pos) = path.find('/') {
                            self.current_dir = Some(path[slash_pos..].to_string());
                        }
                    }
                }
            }
            // OSC 133: Semantic prompts (shell integration)
            // A = prompt start, B = prompt end, C = command start, D = command end
            b"133" => {
                if params.len() > 1 {
                    match params[1] {
                        b"A" => {
                            self.last_prompt_row = Some(self.cursor.row);
                        }
                        b"C" => {
                            // Command start — extract command text from prompt row
                            if let Some(prompt_row) = self.last_prompt_row {
                                let cmd = self.row_text(prompt_row).trim().to_string();
                                if !cmd.is_empty() {
                                    self.last_command = Some(cmd);
                                }
                            }
                            self.command_start_row = Some(self.cursor.row);
                        }
                        b"D" => {
                            // Command end — extract exit code and finalize block
                            let exit_code = if params.len() > 2 {
                                std::str::from_utf8(params[2])
                                    .ok()
                                    .and_then(|s| s.parse::<i32>().ok())
                            } else {
                                None
                            };
                            self.last_exit_code = exit_code;

                            if let (Some(cmd), Some(start_row)) =
                                (self.last_command.clone(), self.command_start_row)
                            {
                                self.command_blocks.push(CommandBlock {
                                    command: cmd,
                                    output_start_row: start_row,
                                    output_end_row: self.cursor.row,
                                    exit_code,
                                });
                                // Keep last 50 command blocks
                                if self.command_blocks.len() > 50 {
                                    self.command_blocks.remove(0);
                                }
                            }
                            self.command_start_row = None;
                        }
                        _ => {}
                    }
                }
            }
            _ => {}
        }
    }

    fn csi_dispatch(&mut self, params: &Params, intermediates: &[u8], _ignore: bool, action: char) {
        let mut params_vec: Vec<u16> = Vec::new();
        for param in params.iter() {
            params_vec.push(param[0] as u16);
        }

        let p = |i: usize, default: u16| -> u16 {
            params_vec.get(i).copied().filter(|&v| v != 0).unwrap_or(default)
        };

        match (action, intermediates) {
            // CUU — Cursor Up
            ('A', []) => {
                let n = p(0, 1) as usize;
                self.cursor.row = self.cursor.row.saturating_sub(n);
                self.dirty[self.cursor.row] = true;
            }
            // CUD — Cursor Down
            ('B', []) => {
                let n = p(0, 1) as usize;
                self.cursor.row = (self.cursor.row + n).min(self.rows - 1);
                self.dirty[self.cursor.row] = true;
            }
            // CUF — Cursor Forward
            ('C', []) => {
                let n = p(0, 1) as usize;
                self.cursor.col = (self.cursor.col + n).min(self.cols - 1);
            }
            // CUB — Cursor Backward
            ('D', []) => {
                let n = p(0, 1) as usize;
                self.cursor.col = self.cursor.col.saturating_sub(n);
            }
            // CNL — Cursor Next Line
            ('E', []) => {
                let n = p(0, 1) as usize;
                self.cursor.row = (self.cursor.row + n).min(self.rows - 1);
                self.cursor.col = 0;
            }
            // CPL — Cursor Previous Line
            ('F', []) => {
                let n = p(0, 1) as usize;
                self.cursor.row = self.cursor.row.saturating_sub(n);
                self.cursor.col = 0;
            }
            // CHA — Cursor Horizontal Absolute
            ('G', []) => {
                self.cursor.col = (p(0, 1) as usize).saturating_sub(1).min(self.cols - 1);
            }
            // CUP — Cursor Position
            ('H', []) | ('f', []) => {
                let row = (p(0, 1) as usize).saturating_sub(1).min(self.rows - 1);
                let col = (p(1, 1) as usize).saturating_sub(1).min(self.cols - 1);
                self.cursor.row = row;
                self.cursor.col = col;
            }
            // ED — Erase in Display
            ('J', []) => {
                self.erase_in_display(p(0, 0));
            }
            // EL — Erase in Line
            ('K', []) => {
                self.erase_in_line(p(0, 0));
            }
            // IL — Insert Lines
            ('L', []) => {
                let n = p(0, 1) as usize;
                for _ in 0..n {
                    self.scroll_down();
                }
            }
            // DL — Delete Lines
            ('M', []) => {
                let n = p(0, 1) as usize;
                for _ in 0..n {
                    self.scroll_up();
                }
            }
            // DCH — Delete Characters
            ('P', []) => {
                let n = p(0, 1) as usize;
                let row = self.cursor.row;
                let col = self.cursor.col;
                for i in col..self.cols {
                    if i + n < self.cols {
                        self.cells[row][i] = self.cells[row][i + n].clone();
                    } else {
                        self.cells[row][i] = Cell::default();
                    }
                }
                self.dirty[row] = true;
            }
            // SGR — Select Graphic Rendition
            ('m', []) => {
                self.parse_sgr(params);
            }
            // DECSTBM — Set Scrolling Region
            ('r', []) => {
                let top = (p(0, 1) as usize).saturating_sub(1);
                let bottom = (p(1, self.rows as u16) as usize).saturating_sub(1).min(self.rows - 1);
                self.scroll_top = top;
                self.scroll_bottom = bottom;
                self.cursor.row = 0;
                self.cursor.col = 0;
            }
            // ICH — Insert Characters
            ('@', []) => {
                let n = p(0, 1) as usize;
                let row = self.cursor.row;
                let col = self.cursor.col;
                for i in (col..self.cols).rev() {
                    if i >= col + n {
                        self.cells[row][i] = self.cells[row][i - n].clone();
                    }
                }
                for i in col..(col + n).min(self.cols) {
                    self.cells[row][i] = Cell::default();
                }
                self.dirty[row] = true;
            }
            // ECH — Erase Characters
            ('X', []) => {
                let n = p(0, 1) as usize;
                let row = self.cursor.row;
                for i in self.cursor.col..(self.cursor.col + n).min(self.cols) {
                    self.cells[row][i] = Cell::default();
                }
                self.dirty[row] = true;
            }
            // SU — Scroll Up (pan down)
            ('S', []) => {
                let n = p(0, 1) as usize;
                for _ in 0..n {
                    self.scroll_up();
                }
            }
            // SD — Scroll Down (pan up)
            ('T', []) => {
                let n = p(0, 1) as usize;
                for _ in 0..n {
                    self.scroll_down();
                }
            }
            // REP — Repeat last character
            ('b', []) => {
                let n = p(0, 1) as usize;
                if self.cursor.col > 0 {
                    let last_char = self.cells[self.cursor.row][self.cursor.col - 1].c;
                    for _ in 0..n {
                        self.set_cell(last_char);
                    }
                }
            }
            // DA — Device Attributes (respond with VT100 identity)
            ('c', []) | ('c', [b'?']) => {
                // Report as VT100 with advanced video option
                self.pending_responses.push(b"\x1b[?1;2c".to_vec());
            }
            // VPA — Vertical Position Absolute
            ('d', []) => {
                self.cursor.row = (p(0, 1) as usize).saturating_sub(1).min(self.rows - 1);
            }
            // TBC — Tab Clear
            ('g', []) => {
                match p(0, 0) {
                    0 => {
                        if self.cursor.col < self.tab_stops.len() {
                            self.tab_stops[self.cursor.col] = false;
                        }
                    }
                    3 => {
                        self.tab_stops.fill(false);
                    }
                    _ => {}
                }
            }
            // DSR — Device Status Report
            ('n', []) => {
                match p(0, 0) {
                    5 => {
                        // Report "OK"
                        self.pending_responses.push(b"\x1b[0n".to_vec());
                    }
                    6 => {
                        // CPR — Cursor Position Report
                        let response = format!("\x1b[{};{}R", self.cursor.row + 1, self.cursor.col + 1);
                        self.pending_responses.push(response.into_bytes());
                    }
                    _ => {}
                }
            }
            // DECSC — Save Cursor (via CSI s)
            ('s', []) => {
                self.saved_cursor = Some(self.cursor);
            }
            // DECRC — Restore Cursor (via CSI u)
            ('u', []) => {
                if let Some(saved) = self.saved_cursor {
                    self.cursor = saved;
                }
            }
            // SGR with ? prefix (DEC private modes)
            ('h', [b'?']) => {
                for &code in &params_vec {
                    match code {
                        1 => self.application_cursor_keys = true,
                        7 => self.auto_wrap = true,
                        25 => self.cursor.visible = true,
                        47 | 1047 => {
                            // Alternate screen (no save/restore cursor)
                            self.alternate_screen = Some(self.cells.clone());
                            self.cells = vec![vec![Cell::default(); self.cols]; self.rows];
                            self.mark_all_dirty();
                        }
                        1049 => {
                            // Alternate screen buffer (with save/restore cursor)
                            self.saved_cursor = Some(self.cursor);
                            self.alternate_screen = Some(self.cells.clone());
                            self.cells = vec![vec![Cell::default(); self.cols]; self.rows];
                            self.mark_all_dirty();
                        }
                        2004 => self.bracketed_paste = true,
                        _ => {}
                    }
                }
            }
            ('l', [b'?']) => {
                for &code in &params_vec {
                    match code {
                        1 => self.application_cursor_keys = false,
                        7 => self.auto_wrap = false,
                        25 => self.cursor.visible = false,
                        47 | 1047 => {
                            if let Some(saved) = self.alternate_screen.take() {
                                self.cells = saved;
                                self.mark_all_dirty();
                            }
                        }
                        1049 => {
                            // Restore from alternate screen buffer
                            if let Some(saved) = self.alternate_screen.take() {
                                self.cells = saved;
                                self.mark_all_dirty();
                            }
                            if let Some(saved) = self.saved_cursor {
                                self.cursor = saved;
                            }
                        }
                        2004 => self.bracketed_paste = false,
                        _ => {}
                    }
                }
            }
            // Cursor style (DECSCUSR)
            ('q', [b' ']) => {
                match p(0, 1) {
                    0 | 1 => self.cursor.shape = CursorShape::Block,
                    2 => self.cursor.shape = CursorShape::Block,
                    3 | 4 => self.cursor.shape = CursorShape::Underline,
                    5 | 6 => self.cursor.shape = CursorShape::Bar,
                    _ => {}
                }
            }
            _ => {
                log::trace!("unhandled CSI: {:?} {:?} {:?}", params_vec, intermediates, action);
            }
        }
    }

    fn esc_dispatch(&mut self, intermediates: &[u8], _ignore: bool, byte: u8) {
        match (byte, intermediates) {
            // RI — Reverse Index
            (b'M', []) => {
                if self.cursor.row == self.scroll_top {
                    self.scroll_down();
                } else if self.cursor.row > 0 {
                    self.cursor.row -= 1;
                }
            }
            // DECSC — Save Cursor
            (b'7', []) => {
                self.saved_cursor = Some(self.cursor);
            }
            // DECRC — Restore Cursor
            (b'8', []) => {
                if let Some(saved) = self.saved_cursor {
                    self.cursor = saved;
                }
            }
            // HTS — Horizontal Tab Set
            (b'H', []) => {
                if self.cursor.col < self.tab_stops.len() {
                    self.tab_stops[self.cursor.col] = true;
                }
            }
            // DECKPAM — Application Keypad Mode (numpad)
            (b'=', []) => {}
            // DECKPNM — Normal Keypad Mode
            (b'>', []) => {}
            // RIS — Full Reset
            (b'c', []) => {
                *self = Grid::new(self.cols, self.rows);
            }
            _ => {}
        }
    }
}
