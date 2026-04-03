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

/// Terminal color theme — the 16 ANSI colors plus default fg/bg.
/// The 216-color cube and 24-step grayscale ramp are computed algorithmically.
#[derive(Debug, Clone)]
pub struct TerminalTheme {
    pub name: String,
    pub foreground: Color,
    pub background: Color,
    pub ansi: [Color; 16],
}

impl TerminalTheme {
    /// Build a full 256-color palette from this theme.
    pub fn palette(&self) -> [Color; 256] {
        let mut palette = [Color::rgb(0, 0, 0); 256];
        palette[..16].copy_from_slice(&self.ansi);

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

    /// Flexoki dark — the default con theme.
    pub fn flexoki_dark() -> Self {
        Self {
            name: "flexoki-dark".into(),
            foreground: Color::rgb(0xCE, 0xCD, 0xC3),
            background: Color::rgb(0x10, 0x0F, 0x0F),
            ansi: [
                Color::rgb(0x10, 0x0F, 0x0F), // black
                Color::rgb(0xD1, 0x4D, 0x41), // red
                Color::rgb(0x87, 0x9A, 0x39), // green
                Color::rgb(0xD0, 0xA2, 0x15), // yellow
                Color::rgb(0x43, 0x85, 0xBE), // blue
                Color::rgb(0x8B, 0x7E, 0xC8), // magenta
                Color::rgb(0x3A, 0xA9, 0x9F), // cyan
                Color::rgb(0xCE, 0xCD, 0xC3), // white
                Color::rgb(0x57, 0x56, 0x53), // bright black
                Color::rgb(0xD1, 0x4D, 0x41), // bright red
                Color::rgb(0x87, 0x9A, 0x39), // bright green
                Color::rgb(0xD0, 0xA2, 0x15), // bright yellow
                Color::rgb(0x43, 0x85, 0xBE), // bright blue
                Color::rgb(0xCE, 0x5D, 0x97), // bright magenta
                Color::rgb(0x3A, 0xA9, 0x9F), // bright cyan
                Color::rgb(0xCE, 0xCD, 0xC3), // bright white
            ],
        }
    }

    /// Flexoki light.
    pub fn flexoki_light() -> Self {
        Self {
            name: "flexoki-light".into(),
            foreground: Color::rgb(0x10, 0x0F, 0x0F),
            background: Color::rgb(0xFF, 0xFC, 0xF0),
            ansi: [
                Color::rgb(0x10, 0x0F, 0x0F), // black
                Color::rgb(0xAF, 0x30, 0x29), // red
                Color::rgb(0x66, 0x80, 0x0B), // green
                Color::rgb(0xAD, 0x8A, 0x01), // yellow
                Color::rgb(0x20, 0x5E, 0xA6), // blue
                Color::rgb(0x5E, 0x40, 0x9D), // magenta
                Color::rgb(0x24, 0x83, 0x7B), // cyan
                Color::rgb(0xCE, 0xCD, 0xC3), // white
                Color::rgb(0x87, 0x85, 0x80), // bright black
                Color::rgb(0xD1, 0x4D, 0x41), // bright red
                Color::rgb(0x87, 0x9A, 0x39), // bright green
                Color::rgb(0xD0, 0xA2, 0x15), // bright yellow
                Color::rgb(0x43, 0x85, 0xBE), // bright blue
                Color::rgb(0xCE, 0x5D, 0x97), // bright magenta
                Color::rgb(0x3A, 0xA9, 0x9F), // bright cyan
                Color::rgb(0xFF, 0xFC, 0xF0), // bright white
            ],
        }
    }

    /// Catppuccin Mocha.
    pub fn catppuccin_mocha() -> Self {
        Self {
            name: "catppuccin-mocha".into(),
            foreground: Color::rgb(0xCD, 0xD6, 0xF4),
            background: Color::rgb(0x1E, 0x1E, 0x2E),
            ansi: [
                Color::rgb(0x45, 0x47, 0x5A), // black (surface1)
                Color::rgb(0xF3, 0x8B, 0xA8), // red
                Color::rgb(0xA6, 0xE3, 0xA1), // green
                Color::rgb(0xF9, 0xE2, 0xAF), // yellow
                Color::rgb(0x89, 0xB4, 0xFA), // blue
                Color::rgb(0xCB, 0xA6, 0xF7), // magenta (mauve)
                Color::rgb(0x94, 0xE2, 0xD5), // cyan (teal)
                Color::rgb(0xBA, 0xC2, 0xDE), // white (subtext1)
                Color::rgb(0x58, 0x5B, 0x70), // bright black (surface2)
                Color::rgb(0xF3, 0x8B, 0xA8), // bright red
                Color::rgb(0xA6, 0xE3, 0xA1), // bright green
                Color::rgb(0xF9, 0xE2, 0xAF), // bright yellow
                Color::rgb(0x89, 0xB4, 0xFA), // bright blue
                Color::rgb(0xCB, 0xA6, 0xF7), // bright magenta
                Color::rgb(0x94, 0xE2, 0xD5), // bright cyan
                Color::rgb(0xCD, 0xD6, 0xF4), // bright white (text)
            ],
        }
    }

    /// Tokyo Night.
    pub fn tokyonight() -> Self {
        Self {
            name: "tokyonight".into(),
            foreground: Color::rgb(0xC0, 0xCA, 0xF5),
            background: Color::rgb(0x1A, 0x1B, 0x26),
            ansi: [
                Color::rgb(0x15, 0x16, 0x1E), // black
                Color::rgb(0xF7, 0x76, 0x8E), // red
                Color::rgb(0x9E, 0xCE, 0x6A), // green
                Color::rgb(0xE0, 0xAF, 0x68), // yellow
                Color::rgb(0x7A, 0xA2, 0xF7), // blue
                Color::rgb(0xBB, 0x9A, 0xF7), // magenta
                Color::rgb(0x7D, 0xCF, 0xFF), // cyan
                Color::rgb(0xA9, 0xB1, 0xD6), // white
                Color::rgb(0x41, 0x48, 0x68), // bright black
                Color::rgb(0xF7, 0x76, 0x8E), // bright red
                Color::rgb(0x9E, 0xCE, 0x6A), // bright green
                Color::rgb(0xE0, 0xAF, 0x68), // bright yellow
                Color::rgb(0x7A, 0xA2, 0xF7), // bright blue
                Color::rgb(0xBB, 0x9A, 0xF7), // bright magenta
                Color::rgb(0x7D, 0xCF, 0xFF), // bright cyan
                Color::rgb(0xC0, 0xCA, 0xF5), // bright white
            ],
        }
    }

    /// Look up a built-in theme by name. Case-insensitive.
    pub fn by_name(name: &str) -> Option<Self> {
        match name.to_lowercase().as_str() {
            "flexoki-dark" | "flexoki" => Some(Self::flexoki_dark()),
            "flexoki-light" => Some(Self::flexoki_light()),
            "catppuccin-mocha" | "catppuccin" => Some(Self::catppuccin_mocha()),
            "tokyonight" | "tokyo-night" => Some(Self::tokyonight()),
            _ => None,
        }
    }

    /// All available built-in theme names.
    pub fn available() -> &'static [&'static str] {
        &["flexoki-dark", "flexoki-light", "catppuccin-mocha", "tokyonight"]
    }
}

impl Default for TerminalTheme {
    fn default() -> Self {
        Self::flexoki_dark()
    }
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
            fg: Color::rgb(0xCE, 0xCD, 0xC3), // Flexoki tx (foreground)
            bg: Color::rgb(0x10, 0x0F, 0x0F), // Flexoki bg (background)
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

/// A command block visible in the current viewport
#[derive(Debug, Clone)]
pub struct VisibleCommandBlock {
    pub command: String,
    pub viewport_row: usize,
    pub exit_code: Option<i32>,
    pub output_start_row: usize,
    pub output_end_row: usize,
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
    /// The default style used when resetting cells (fg/bg from the active theme)
    default_style: Style,
    saved_cursor: Option<Cursor>,
    palette: [Color; 256],
    alternate_screen: Option<Vec<Vec<Cell>>>,
    dirty: Vec<bool>,
    // OSC 133 semantic prompt tracking
    pub last_prompt_row: Option<usize>,
    pub last_command: Option<String>,
    pub last_exit_code: Option<i32>,
    pub current_dir: Option<String>,
    /// Hostname from OSC 7 URI — matches local hostname for local sessions,
    /// differs for SSH sessions (reveals remote host).
    pub hostname: Option<String>,
    /// Local machine hostname, cached once at Grid creation for SSH detection.
    local_hostname: String,
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
    /// Kitty keyboard protocol flags (0 = disabled)
    pub kitty_keyboard_flags: u32,
    /// Kitty keyboard flags stack (for push/pop)
    kitty_flags_stack: Vec<u32>,
    /// Callback fired when a command completes (OSC 133 D).
    /// Used by the visible terminal tool to capture command output.
    /// Takes the output text and optional exit code.
    on_command_complete: Option<Box<dyn FnOnce(String, Option<i32>) + Send>>,
}

impl Grid {
    pub fn new(cols: usize, rows: usize) -> Self {
        Self::with_scrollback(cols, rows, 10_000)
    }

    pub fn with_scrollback(cols: usize, rows: usize, max_scrollback: usize) -> Self {
        Self::with_theme(cols, rows, max_scrollback, &TerminalTheme::default())
    }

    pub fn with_theme(
        cols: usize,
        rows: usize,
        max_scrollback: usize,
        theme: &TerminalTheme,
    ) -> Self {
        let default_style = Style {
            fg: theme.foreground,
            bg: theme.background,
            ..Style::default()
        };
        let default_cell = Cell {
            c: ' ',
            style: default_style,
            wide: false,
        };
        let cells = vec![vec![default_cell; cols]; rows];
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
            style: default_style,
            default_style,
            saved_cursor: None,
            palette: theme.palette(),
            alternate_screen: None,
            dirty,
            last_prompt_row: None,
            last_command: None,
            last_exit_code: None,
            current_dir: None,
            hostname: None,
            local_hostname: gethostname::gethostname()
                .to_string_lossy()
                .to_string(),
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
            kitty_keyboard_flags: 0,
            kitty_flags_stack: Vec::new(),
            on_command_complete: None,
        }
    }

    /// Register a one-shot callback for the next command completion.
    /// Used by the visible terminal tool: the workspace sets this before
    /// writing a command to the PTY, and it fires when OSC 133 D arrives.
    pub fn set_command_complete_callback(
        &mut self,
        callback: Box<dyn FnOnce(String, Option<i32>) + Send>,
    ) {
        self.on_command_complete = Some(callback);
    }

    pub fn cell(&self, row: usize, col: usize) -> &Cell {
        &self.cells[row][col]
    }

    pub fn is_dirty(&self, row: usize) -> bool {
        self.dirty[row]
    }

    /// The default style (fg/bg) for this grid's theme.
    pub fn default_style(&self) -> &Style {
        &self.default_style
    }

    /// A blank cell using this grid's theme colors.
    fn blank_cell(&self) -> Cell {
        Cell {
            c: ' ',
            style: self.default_style,
            wide: false,
        }
    }

    /// A blank row of cells using this grid's theme colors.
    fn blank_row(&self) -> Vec<Cell> {
        vec![self.blank_cell(); self.cols]
    }

    /// Apply a new terminal theme, updating the palette and default colors.
    pub fn set_theme(&mut self, theme: &TerminalTheme) {
        self.default_style = Style {
            fg: theme.foreground,
            bg: theme.background,
            ..Style::default()
        };
        self.palette = theme.palette();
        // Reset the current style to match the new theme defaults
        self.style = self.default_style;
        // Re-render all cells with default bg/fg to match the new theme
        for row in &mut self.cells {
            for cell in row.iter_mut() {
                // Only update cells that were using the old default colors
                cell.style.fg = theme.foreground;
                cell.style.bg = theme.background;
            }
        }
        self.dirty = vec![true; self.rows];
    }

    /// Whether a command is currently executing (between OSC 133;C and D).
    /// Returns false if shell integration is not active.
    pub fn is_busy(&self) -> bool {
        self.command_start_row.is_some()
    }

    /// Detect a remote hostname if this pane is an SSH session.
    /// Uses three signals (checked in priority order):
    /// 1. OSC 7 hostname differs from local hostname
    /// 2. Terminal title matches `user@host` pattern (set by many SSH configs)
    /// 3. Last command starts with `ssh`
    pub fn detected_remote_host(&self) -> Option<String> {
        // 1. OSC 7 hostname differs from local
        if let Some(h) = &self.hostname {
            if !h.is_empty() && !h.eq_ignore_ascii_case(&self.local_hostname) {
                return Some(h.clone());
            }
        }

        // 2. Title matches user@host
        if let Some(title) = &self.title {
            if let Some(at_pos) = title.find('@') {
                let host = title[at_pos + 1..].trim();
                // Skip if host looks like local
                if !host.is_empty() && !host.eq_ignore_ascii_case(&self.local_hostname) {
                    // Take first word (title may have trailing info like ": ~/dir")
                    let host = host.split(&[':', ' ', '\t'][..]).next().unwrap_or(host);
                    if !host.is_empty() {
                        return Some(host.to_string());
                    }
                }
            }
        }

        // 3. Command text contains "ssh " (may have prompt prefix like "❯ ssh host")
        if let Some(cmd) = &self.last_command {
            if let Some(host) = extract_ssh_target(cmd) {
                return Some(host);
            }
        }

        None
    }

    /// Whether the cursor is at a shell prompt (not in alternate screen / TUI,
    /// and OSC 133 has marked a prompt row). Useful for ghost text suggestions.
    pub fn at_shell_prompt(&self) -> bool {
        self.alternate_screen.is_none()
            && self.last_prompt_row.is_some()
            && self.scrollback_offset == 0
    }

    /// The text the user has typed on the current cursor line, from the start
    /// of the prompt command area to the cursor position.
    /// Returns None if not at a shell prompt.
    pub fn current_input(&self) -> Option<String> {
        if !self.at_shell_prompt() {
            return None;
        }
        let row = self.cursor.row;
        if row >= self.rows {
            return None;
        }
        // Gather text from start of line to cursor column
        let text: String = self.cells[row]
            .iter()
            .take(self.cursor.col)
            .map(|c| c.c)
            .collect();
        let trimmed = text.trim_end().to_string();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed)
        }
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
        if new_rows < self.rows {
            // Shrinking vertically: push excess rows above cursor into scrollback
            // so content near the cursor (recent output, prompt) stays visible.
            let excess = self.rows - new_rows;
            let save_count = excess.min(self.cursor.row);

            // Save top rows to scrollback
            for i in 0..save_count {
                // Rewidth the row to new_cols for consistency
                let mut row = self.cells[i].clone();
                row.resize(new_cols, self.blank_cell());
                self.scrollback.push(row);
            }
            while self.scrollback.len() > self.max_scrollback {
                self.scrollback.remove(0);
            }

            // Build new cells from remaining rows (starting after saved rows)
            let mut new_cells = vec![vec![self.blank_cell(); new_cols]; new_rows];
            for row in 0..new_rows {
                let src_row = save_count + row;
                if src_row < self.rows {
                    let copy_cols = new_cols.min(self.cells[src_row].len());
                    for col in 0..copy_cols {
                        new_cells[row][col] = self.cells[src_row][col].clone();
                    }
                }
            }
            self.cursor.row = self.cursor.row.saturating_sub(save_count).min(new_rows - 1);
            self.cells = new_cells;
        } else if new_rows > self.rows {
            // Growing vertically: pull lines from scrollback to fill extra space
            let extra = new_rows - self.rows;
            let pull = extra.min(self.scrollback.len());

            let mut new_cells = vec![vec![self.blank_cell(); new_cols]; new_rows];

            // Pull scrollback lines into top of new grid
            for i in 0..pull {
                let sb_idx = self.scrollback.len() - pull + i;
                let copy_cols = new_cols.min(self.scrollback[sb_idx].len());
                for col in 0..copy_cols {
                    new_cells[i][col] = self.scrollback[sb_idx][col].clone();
                }
            }
            let new_sb_len = self.scrollback.len() - pull;
            self.scrollback.truncate(new_sb_len);

            // Copy existing screen content below pulled lines
            for row in 0..self.rows {
                let dst_row = pull + row;
                let copy_cols = new_cols.min(self.cells[row].len());
                for col in 0..copy_cols {
                    new_cells[dst_row][col] = self.cells[row][col].clone();
                }
            }
            self.cursor.row = (self.cursor.row + pull).min(new_rows - 1);
            self.cells = new_cells;
        } else {
            // Same row count — just adjust columns
            let mut new_cells = vec![vec![self.blank_cell(); new_cols]; new_rows];
            for row in 0..new_rows {
                let copy_cols = new_cols.min(self.cells[row].len());
                for col in 0..copy_cols {
                    new_cells[row][col] = self.cells[row][col].clone();
                }
            }
            self.cells = new_cells;
        }

        self.cols = new_cols;
        self.rows = new_rows;
        self.dirty = vec![true; new_rows];
        self.scroll_top = 0;
        self.scroll_bottom = new_rows - 1;
        self.cursor.row = self.cursor.row.min(new_rows - 1);
        self.cursor.col = self.cursor.col.min(new_cols - 1);
        self.tab_stops = vec![false; new_cols];
        for i in (0..new_cols).step_by(8) {
            self.tab_stops[i] = true;
        }
    }

    /// Clear scrollback buffer and reset scroll offset
    pub fn clear_scrollback(&mut self) {
        self.scrollback.clear();
        self.scrollback_offset = 0;
    }

    /// Get lines of text content from the terminal (for agent context)
    /// Return command blocks whose output rows overlap the current viewport.
    /// Row indices are adjusted to viewport-relative coordinates.
    pub fn visible_command_blocks(&self) -> Vec<VisibleCommandBlock> {
        let scrollback_len = self.scrollback.len();
        let viewport_start = scrollback_len.saturating_sub(self.scrollback_offset);
        let viewport_end = viewport_start + self.rows;

        self.command_blocks
            .iter()
            .filter_map(|block| {
                // Command block rows are absolute (scrollback + screen)
                let block_start = block.output_start_row;
                let block_end = block.output_end_row;

                // Check overlap with viewport
                if block_end < viewport_start || block_start >= viewport_end {
                    return None;
                }

                // Convert to viewport-relative row
                let rel_row = block_start.saturating_sub(viewport_start);

                Some(VisibleCommandBlock {
                    command: block.command.clone(),
                    viewport_row: rel_row,
                    exit_code: block.exit_code,
                    output_start_row: block.output_start_row,
                    output_end_row: block.output_end_row,
                })
            })
            .collect()
    }

    /// Extract the text output of a command block
    pub fn command_block_output(&self, output_start: usize, output_end: usize) -> String {
        let scrollback_len = self.scrollback.len();
        let mut lines = Vec::new();

        for abs_row in output_start..=output_end {
            let line = if abs_row < scrollback_len {
                self.scrollback[abs_row]
                    .iter()
                    .map(|c| c.c)
                    .collect::<String>()
            } else {
                let screen_row = abs_row - scrollback_len;
                if screen_row < self.rows {
                    self.cells[screen_row]
                        .iter()
                        .map(|c| c.c)
                        .collect::<String>()
                } else {
                    String::new()
                }
            };
            lines.push(line.trim_end().to_string());
        }

        // Trim trailing empty lines
        while lines.last().is_some_and(|l| l.is_empty()) {
            lines.pop();
        }
        lines.join("\n")
    }

    /// Get recent lines from the active screen only (no scrollback).
    /// Used for ghost text suggestions and terminal_exec fallback capture.
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
        while lines.last().is_some_and(|l| l.is_empty()) {
            lines.pop();
        }
        lines
    }

    /// Get the last `max_lines` lines from scrollback + active screen combined.
    /// Unlike `content_lines`, this reaches into scrollback history — useful for
    /// read_pane where the agent needs to see output that scrolled off screen.
    pub fn recent_lines(&self, max_lines: usize) -> Vec<String> {
        let mut lines = Vec::new();
        let sb_len = self.scrollback.len();
        let total = sb_len + self.rows;
        let start = total.saturating_sub(max_lines);

        // Scrollback portion
        if start < sb_len {
            let sb_start = start;
            for i in sb_start..sb_len {
                let line: String = self.scrollback[i].iter().map(|c| c.c).collect();
                lines.push(line.trim_end().to_string());
            }
        }

        // Screen portion
        let screen_start = if start > sb_len { start - sb_len } else { 0 };
        for row in screen_start..self.rows {
            let line: String = self.cells[row].iter().map(|c| c.c).collect();
            lines.push(line.trim_end().to_string());
        }

        while lines.last().is_some_and(|l| l.is_empty()) {
            lines.pop();
        }
        lines
    }

    /// Search scrollback + active screen for lines matching `pattern` (case-insensitive substring).
    /// Returns matching lines with 1-based line numbers, **newest first** so the most
    /// recent matches survive the `max_matches` cap.
    pub fn search_text(&self, pattern: &str, max_matches: usize) -> Vec<(usize, String)> {
        if pattern.is_empty() || max_matches == 0 {
            return Vec::new();
        }
        let pattern_lower = pattern.to_lowercase();
        let mut results = Vec::new();
        let sb_len = self.scrollback.len();

        // Search active screen bottom-to-top (most recent first)
        for i in (0..self.rows).rev() {
            let line: String = self.cells[i].iter().map(|c| c.c).collect();
            let trimmed = line.trim_end().to_string();
            if !trimmed.is_empty() && trimmed.to_lowercase().contains(&pattern_lower) {
                results.push((sb_len + i + 1, trimmed));
                if results.len() >= max_matches {
                    return results;
                }
            }
        }

        // Search scrollback bottom-to-top (most recent first)
        for i in (0..sb_len).rev() {
            let line: String = self.scrollback[i].iter().map(|c| c.c).collect();
            let trimmed = line.trim_end().to_string();
            if !trimmed.is_empty() && trimmed.to_lowercase().contains(&pattern_lower) {
                results.push((i + 1, trimmed));
                if results.len() >= max_matches {
                    return results;
                }
            }
        }

        results
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
        self.cells[self.scroll_bottom] = self.blank_row();
        self.dirty[self.scroll_bottom] = true;
    }

    fn scroll_down(&mut self) {
        for i in (self.scroll_top + 1..=self.scroll_bottom).rev() {
            self.cells[i] = self.cells[i - 1].clone();
            self.dirty[i] = true;
        }
        self.cells[self.scroll_top] = self.blank_row();
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
                    self.cells[self.cursor.row][col] = self.blank_cell();
                }
                self.dirty[self.cursor.row] = true;
                for row in (self.cursor.row + 1)..self.rows {
                    self.cells[row] = self.blank_row();
                    self.dirty[row] = true;
                }
            }
            1 => {
                // Erase from start to cursor
                for row in 0..self.cursor.row {
                    self.cells[row] = self.blank_row();
                    self.dirty[row] = true;
                }
                for col in 0..=self.cursor.col.min(self.cols - 1) {
                    self.cells[self.cursor.row][col] = self.blank_cell();
                }
                self.dirty[self.cursor.row] = true;
            }
            2 | 3 => {
                // Erase entire display
                for row in 0..self.rows {
                    self.cells[row] = self.blank_row();
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
                    self.cells[row][col] = self.blank_cell();
                }
            }
            1 => {
                for col in 0..=self.cursor.col.min(self.cols - 1) {
                    self.cells[row][col] = self.blank_cell();
                }
            }
            2 => {
                self.cells[row] = self.blank_row();
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
                0 => self.style = self.default_style,
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
                39 => self.style.fg = self.default_style.fg,
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
                49 => self.style.bg = self.default_style.bg,
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
            // OSC 7: Current directory (file://hostname/path)
            b"7" => {
                if params.len() > 1 {
                    let uri = String::from_utf8_lossy(params[1]);
                    if let Some(rest) = uri.strip_prefix("file://") {
                        if let Some(slash_pos) = rest.find('/') {
                            let host = &rest[..slash_pos];
                            if !host.is_empty() {
                                self.hostname = Some(host.to_string());
                            }
                            self.current_dir = Some(rest[slash_pos..].to_string());
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
                                let output_end = self.cursor.row;
                                self.command_blocks.push(CommandBlock {
                                    command: cmd,
                                    output_start_row: start_row,
                                    output_end_row: output_end,
                                    exit_code,
                                });
                                // Keep last 50 command blocks
                                if self.command_blocks.len() > 50 {
                                    self.command_blocks.remove(0);
                                }

                                // Fire the visible terminal tool callback if registered
                                if let Some(callback) = self.on_command_complete.take() {
                                    let output =
                                        self.command_block_output(start_row, output_end);
                                    callback(output, exit_code);
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
                        self.cells[row][i] = self.blank_cell();
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
                    self.cells[row][i] = self.blank_cell();
                }
                self.dirty[row] = true;
            }
            // ECH — Erase Characters
            ('X', []) => {
                let n = p(0, 1) as usize;
                let row = self.cursor.row;
                for i in self.cursor.col..(self.cursor.col + n).min(self.cols) {
                    self.cells[row][i] = self.blank_cell();
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
                            self.cells = vec![self.blank_row(); self.rows];
                            self.mark_all_dirty();
                        }
                        1049 => {
                            // Alternate screen buffer (with save/restore cursor)
                            self.saved_cursor = Some(self.cursor);
                            self.alternate_screen = Some(self.cells.clone());
                            self.cells = vec![self.blank_row(); self.rows];
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
            // Kitty keyboard protocol: CSI > u — push flags
            ('u', [b'>']) => {
                let flags = p(0, 0) as u32;
                self.kitty_flags_stack.push(self.kitty_keyboard_flags);
                self.kitty_keyboard_flags = flags;
            }
            // Kitty keyboard protocol: CSI < u — pop flags
            ('u', [b'<']) => {
                let count = p(0, 1) as usize;
                for _ in 0..count {
                    if let Some(flags) = self.kitty_flags_stack.pop() {
                        self.kitty_keyboard_flags = flags;
                    } else {
                        self.kitty_keyboard_flags = 0;
                        break;
                    }
                }
            }
            // Kitty keyboard protocol: CSI ? u — query flags
            ('u', [b'?']) => {
                let response = format!("\x1b[?{}u", self.kitty_keyboard_flags);
                self.pending_responses.push(response.into_bytes());
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

/// Extract the SSH target host from a command string that may include prompt prefix.
/// Handles: "ssh host", "❯ ssh host", "ssh user@host", "ssh -p 22 host", etc.
fn extract_ssh_target(text: &str) -> Option<String> {
    // Find "ssh " in the text, ensuring it's at a word boundary
    let idx = text.find("ssh ")?;
    if idx > 0 {
        let prev = text.as_bytes()[idx - 1];
        if prev.is_ascii_alphanumeric() || prev == b'/' {
            return None; // Part of another word like "openssh" or path
        }
    }

    let ssh_part = &text[idx..];
    let parts: Vec<&str> = ssh_part.split_whitespace().collect();

    // Skip flags and their arguments to find the host
    let mut skip_next = false;
    for part in parts.iter().skip(1) {
        if skip_next {
            skip_next = false;
            continue;
        }
        if part.starts_with('-') {
            // Flags that take a value argument
            if matches!(*part, "-p" | "-i" | "-l" | "-o" | "-F" | "-J" | "-W" | "-L" | "-R" | "-D" | "-b" | "-c" | "-e" | "-m" | "-S" | "-w") {
                skip_next = true;
            }
            continue;
        }
        // This is the host (possibly user@host)
        let host = match part.find('@') {
            Some(at) => &part[at + 1..],
            None => part,
        };
        // Strip port suffix if present (host:port)
        let host = host.split(':').next().unwrap_or(host);
        if !host.is_empty() {
            return Some(host.to_string());
        }
    }
    None
}
