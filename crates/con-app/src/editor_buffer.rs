//! Editable text buffer for the lightweight editor pane.
//!
//! This module deliberately has no GPUI dependencies so editing behavior stays
//! unit-testable and can evolve independently from rendering.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CursorPosition {
    pub row: usize,
    pub column: usize,
}

impl CursorPosition {
    pub const fn new(row: usize, column: usize) -> Self {
        Self { row, column }
    }
}

#[derive(Debug, Clone)]
pub struct EditorBuffer {
    lines: Vec<String>,
    cursor: CursorPosition,
    dirty: bool,
}

impl EditorBuffer {
    pub fn from_text(text: impl Into<String>) -> Self {
        let text = text.into();
        let mut lines = text.lines().map(ToString::to_string).collect::<Vec<_>>();
        if text.ends_with('\n') {
            lines.push(String::new());
        }
        if lines.is_empty() {
            lines.push(String::new());
        }

        Self {
            lines,
            cursor: CursorPosition::new(0, 0),
            dirty: false,
        }
    }

    pub fn lines(&self) -> &[String] {
        &self.lines
    }

    pub fn cursor(&self) -> CursorPosition {
        self.cursor
    }

    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    pub fn insert_text(&mut self, text: &str) {
        if text.is_empty() {
            return;
        }

        for ch in text.chars() {
            match ch {
                '\r' => {}
                '\n' => self.insert_newline(),
                ch => self.insert_char(ch),
            }
        }
        self.dirty = true;
    }

    pub fn insert_newline(&mut self) {
        let row = self.cursor.row;
        let column = self.cursor.column;
        let tail = self.lines[row].split_off(column);
        self.lines.insert(row + 1, tail);
        self.cursor = CursorPosition::new(row + 1, 0);
        self.dirty = true;
    }

    pub fn delete_backward(&mut self) {
        if self.cursor.column > 0 {
            let row = self.cursor.row;
            let column = self.cursor.column;
            self.lines[row].remove(column - 1);
            self.cursor.column -= 1;
            self.dirty = true;
            return;
        }

        if self.cursor.row == 0 {
            return;
        }

        let row = self.cursor.row;
        let current = self.lines.remove(row);
        let previous_len = self.lines[row - 1].len();
        self.lines[row - 1].push_str(&current);
        self.cursor = CursorPosition::new(row - 1, previous_len);
        self.dirty = true;
    }

    pub fn delete_forward(&mut self) {
        let row = self.cursor.row;
        let column = self.cursor.column;
        if column < self.lines[row].len() {
            self.lines[row].remove(column);
            self.dirty = true;
            return;
        }

        if row + 1 >= self.lines.len() {
            return;
        }

        let next = self.lines.remove(row + 1);
        self.lines[row].push_str(&next);
        self.dirty = true;
    }

    pub fn move_left(&mut self) {
        if self.cursor.column > 0 {
            self.cursor.column -= 1;
        } else if self.cursor.row > 0 {
            self.cursor.row -= 1;
            self.cursor.column = self.lines[self.cursor.row].len();
        }
    }

    pub fn move_right(&mut self) {
        if self.cursor.column < self.lines[self.cursor.row].len() {
            self.cursor.column += 1;
        } else if self.cursor.row + 1 < self.lines.len() {
            self.cursor.row += 1;
            self.cursor.column = 0;
        }
    }

    pub fn move_up(&mut self) {
        if self.cursor.row == 0 {
            return;
        }
        self.cursor.row -= 1;
        self.cursor.column = self.cursor.column.min(self.lines[self.cursor.row].len());
    }

    pub fn move_down(&mut self) {
        if self.cursor.row + 1 >= self.lines.len() {
            return;
        }
        self.cursor.row += 1;
        self.cursor.column = self.cursor.column.min(self.lines[self.cursor.row].len());
    }

    pub fn move_home(&mut self) {
        self.cursor.column = 0;
    }

    pub fn move_end(&mut self) {
        self.cursor.column = self.lines[self.cursor.row].len();
    }

    pub fn set_cursor(&mut self, row: usize, column: usize) {
        let row = row.min(self.lines.len().saturating_sub(1));
        let column = column.min(self.lines[row].len());
        self.cursor = CursorPosition::new(row, column);
    }

    pub fn text(&self) -> String {
        self.lines.join("\n")
    }

    pub fn mark_clean(&mut self) {
        self.dirty = false;
    }

    fn insert_char(&mut self, ch: char) {
        let row = self.cursor.row;
        let column = self.cursor.column;
        self.lines[row].insert(column, ch);
        self.cursor.column += ch.len_utf8();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn insert_text_updates_line_cursor_and_dirty_state() {
        let mut buffer = EditorBuffer::from_text("hello");

        buffer.set_cursor(0, 5);
        buffer.insert_text(" world");

        assert_eq!(buffer.text(), "hello world");
        assert_eq!(buffer.cursor(), CursorPosition::new(0, 11));
        assert!(buffer.is_dirty());
    }

    #[test]
    fn enter_splits_line_and_backspace_joins_lines() {
        let mut buffer = EditorBuffer::from_text("hello world");

        buffer.set_cursor(0, 5);
        buffer.insert_newline();

        assert_eq!(buffer.lines(), &["hello".to_string(), " world".to_string()]);
        assert_eq!(buffer.cursor(), CursorPosition::new(1, 0));

        buffer.delete_backward();

        assert_eq!(buffer.text(), "hello world");
        assert_eq!(buffer.cursor(), CursorPosition::new(0, 5));
    }

    #[test]
    fn delete_forward_merges_next_line_at_line_end() {
        let mut buffer = EditorBuffer::from_text("hello\nworld");

        buffer.set_cursor(0, 5);
        buffer.delete_forward();

        assert_eq!(buffer.text(), "helloworld");
        assert_eq!(buffer.cursor(), CursorPosition::new(0, 5));
    }

    #[test]
    fn vertical_movement_clamps_column_to_target_line_length() {
        let mut buffer = EditorBuffer::from_text("abcdef\nxy");

        buffer.set_cursor(0, 5);
        buffer.move_down();

        assert_eq!(buffer.cursor(), CursorPosition::new(1, 2));
    }
}
