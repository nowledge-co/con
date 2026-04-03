use crate::grid::{CommandBlock, TerminalTheme, VisibleCommandBlock};

/// Abstraction over terminal backends (legacy Grid+vte or ghostty GPU renderer).
///
/// All methods that read terminal state are safe to call from any thread
/// — implementations must handle their own locking internally.
pub trait TerminalBackend: Send + Sync {
    // ── State Queries ──────────────────────────────────────

    /// Terminal title (from OSC 0/1/2).
    fn title(&self) -> Option<String>;

    /// Working directory (from OSC 7).
    fn current_dir(&self) -> Option<String>;

    /// Remote hostname (from OSC 7 URI or SSH detection).
    fn detected_remote_host(&self) -> Option<String>;

    /// Whether a command is currently executing (between OSC 133 C and D).
    fn is_busy(&self) -> bool;

    /// Whether the cursor is at a shell prompt (OSC 133 detected, not in alt screen).
    fn at_shell_prompt(&self) -> bool;

    /// The text currently being typed at the prompt.
    fn current_input(&self) -> Option<String>;

    /// Last N lines of terminal content (for agent context).
    fn content_lines(&self, n: usize) -> Vec<String>;

    /// Recent N lines (may include scrollback).
    fn recent_lines(&self, n: usize) -> Vec<String>;

    /// Completed command blocks (OSC 133).
    fn command_blocks(&self) -> Vec<CommandBlock>;

    /// Command blocks visible in the current viewport.
    fn visible_command_blocks(&self) -> Vec<VisibleCommandBlock>;

    /// Output text from a command block.
    fn command_block_output(&self, start_row: usize, end_row: usize) -> String;

    /// Last executed command string.
    fn last_command(&self) -> Option<String>;

    /// Exit code of last command.
    fn last_exit_code(&self) -> Option<i32>;

    /// Search visible text for a pattern.
    fn search_text(&self, pattern: &str, limit: usize) -> Vec<(usize, String)>;

    /// Grid dimensions.
    fn grid_size(&self) -> (usize, usize); // (cols, rows)

    /// Whether the child process is still alive.
    fn is_alive(&self) -> bool;

    /// Whether the terminal is in alternate screen mode.
    fn is_alt_screen(&self) -> bool;

    // ── Mutations ──────────────────────────────────────────

    /// Write bytes to the terminal PTY.
    fn write(&self, data: &[u8]);

    /// Resize the terminal.
    fn resize(&self, cols: usize, rows: usize);

    /// Set the terminal theme.
    fn set_theme(&self, theme: &TerminalTheme);

    /// Clear scrollback buffer.
    fn clear_scrollback(&self);
}
