//! Editable text buffer for the lightweight editor pane.
//!
//! This module deliberately has no GPUI dependencies so editing behavior stays
//! unit-testable and can evolve independently from rendering.

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
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
    selection_anchor: Option<CursorPosition>,
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
            selection_anchor: None,
            dirty: false,
        }
    }

    pub fn lines(&self) -> &[String] {
        &self.lines
    }

    pub fn cursor(&self) -> CursorPosition {
        self.cursor
    }

    pub fn has_selection(&self) -> bool {
        self.normalized_selection().is_some()
    }

    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    pub fn insert_text(&mut self, text: &str) {
        if text.is_empty() {
            return;
        }

        let _ = self.delete_selection_if_any();
        for ch in text.chars() {
            match ch {
                '\r' => {}
                '\n' => self.insert_newline_raw(),
                ch => self.insert_char(ch),
            }
        }
        self.dirty = true;
    }

    pub fn insert_newline(&mut self) {
        let _ = self.delete_selection_if_any();
        self.insert_newline_raw();
        self.dirty = true;
    }

    pub fn delete_backward(&mut self) {
        if self.delete_selection_if_any() {
            self.dirty = true;
            return;
        }

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
        if self.delete_selection_if_any() {
            self.dirty = true;
            return;
        }

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
        self.clear_selection();
        if self.cursor.column > 0 {
            self.cursor.column -= 1;
        } else if self.cursor.row > 0 {
            self.cursor.row -= 1;
            self.cursor.column = self.lines[self.cursor.row].len();
        }
    }

    pub fn move_right(&mut self) {
        self.clear_selection();
        if self.cursor.column < self.lines[self.cursor.row].len() {
            self.cursor.column += 1;
        } else if self.cursor.row + 1 < self.lines.len() {
            self.cursor.row += 1;
            self.cursor.column = 0;
        }
    }

    pub fn move_up(&mut self) {
        self.clear_selection();
        if self.cursor.row == 0 {
            return;
        }
        self.cursor.row -= 1;
        self.cursor.column = self.cursor.column.min(self.lines[self.cursor.row].len());
    }

    pub fn move_down(&mut self) {
        self.clear_selection();
        if self.cursor.row + 1 >= self.lines.len() {
            return;
        }
        self.cursor.row += 1;
        self.cursor.column = self.cursor.column.min(self.lines[self.cursor.row].len());
    }

    pub fn move_home(&mut self) {
        self.clear_selection();
        self.cursor.column = 0;
    }

    pub fn move_end(&mut self) {
        self.clear_selection();
        self.cursor.column = self.lines[self.cursor.row].len();
    }

    pub fn set_cursor(&mut self, row: usize, column: usize) {
        let row = row.min(self.lines.len().saturating_sub(1));
        let column = column.min(self.lines[row].len());
        self.cursor = CursorPosition::new(row, column);
        self.clear_selection();
    }

    pub fn set_selection(&mut self, anchor: CursorPosition, cursor: CursorPosition) {
        self.selection_anchor = Some(self.clamp_position(anchor));
        self.cursor = self.clamp_position(cursor);
    }

    pub fn select_all(&mut self) {
        self.selection_anchor = Some(CursorPosition::new(0, 0));
        let last_row = self.lines.len().saturating_sub(1);
        self.cursor = CursorPosition::new(last_row, self.lines[last_row].len());
    }

    pub fn selected_text(&self) -> Option<String> {
        let (start, end) = self.normalized_selection()?;
        Some(self.text_in_range(start, end))
    }

    pub fn cut_selection(&mut self) -> Option<String> {
        let text = self.selected_text()?;
        self.delete_selection_if_any();
        self.dirty = true;
        Some(text)
    }

    pub fn text(&self) -> String {
        self.lines.join("\n")
    }

    pub fn mark_clean(&mut self) {
        self.dirty = false;
    }

    pub fn save_to(&mut self, path: &std::path::Path) -> std::io::Result<()> {
        std::fs::write(path, self.text())?;
        self.mark_clean();
        Ok(())
    }

    fn clear_selection(&mut self) {
        self.selection_anchor = None;
    }

    fn normalized_selection(&self) -> Option<(CursorPosition, CursorPosition)> {
        let anchor = self.selection_anchor?;
        if anchor == self.cursor {
            return None;
        }
        Some(if anchor <= self.cursor {
            (anchor, self.cursor)
        } else {
            (self.cursor, anchor)
        })
    }

    fn clamp_position(&self, position: CursorPosition) -> CursorPosition {
        let row = position.row.min(self.lines.len().saturating_sub(1));
        CursorPosition::new(row, position.column.min(self.lines[row].len()))
    }

    fn text_in_range(&self, start: CursorPosition, end: CursorPosition) -> String {
        if start.row == end.row {
            return self.lines[start.row][start.column..end.column].to_string();
        }

        let mut text = String::new();
        text.push_str(&self.lines[start.row][start.column..]);
        for row in (start.row + 1)..end.row {
            text.push('\n');
            text.push_str(&self.lines[row]);
        }
        text.push('\n');
        text.push_str(&self.lines[end.row][..end.column]);
        text
    }

    fn delete_selection_if_any(&mut self) -> bool {
        let Some((start, end)) = self.normalized_selection() else {
            return false;
        };

        if start.row == end.row {
            self.lines[start.row].replace_range(start.column..end.column, "");
        } else {
            let tail = self.lines[end.row][end.column..].to_string();
            self.lines[start.row].replace_range(start.column.., "");
            self.lines[start.row].push_str(&tail);
            self.lines.drain((start.row + 1)..=end.row);
        }
        self.cursor = start;
        self.clear_selection();
        true
    }

    fn insert_newline_raw(&mut self) {
        let row = self.cursor.row;
        let column = self.cursor.column;
        let tail = self.lines[row].split_off(column);
        self.lines.insert(row + 1, tail);
        self.cursor = CursorPosition::new(row + 1, 0);
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

    #[test]
    fn select_all_and_replace_with_text() {
        let mut buffer = EditorBuffer::from_text("hello\nworld");

        buffer.select_all();
        assert_eq!(buffer.selected_text().as_deref(), Some("hello\nworld"));
        buffer.insert_text("replacement");

        assert_eq!(buffer.text(), "replacement");
        assert_eq!(buffer.cursor(), CursorPosition::new(0, 11));
        assert!(!buffer.has_selection());
    }

    #[test]
    fn copy_and_cut_selected_text() {
        let mut buffer = EditorBuffer::from_text("hello world");
        buffer.set_selection(CursorPosition::new(0, 6), CursorPosition::new(0, 11));

        assert_eq!(buffer.selected_text().as_deref(), Some("world"));
        assert_eq!(buffer.cut_selection().as_deref(), Some("world"));
        assert_eq!(buffer.text(), "hello ");
        assert!(buffer.is_dirty());
    }

    #[test]
    fn save_writes_text_to_disk_and_marks_buffer_clean() {
        let path = std::env::temp_dir().join(format!(
            "con-editor-buffer-save-{}-{}.txt",
            std::process::id(),
            unique_suffix()
        ));
        let mut buffer = EditorBuffer::from_text("hello");
        buffer.set_cursor(0, 5);
        buffer.insert_text(" world");

        buffer.save_to(&path).expect("save buffer");

        assert_eq!(std::fs::read_to_string(&path).unwrap(), "hello world");
        assert!(!buffer.is_dirty());
        let _ = std::fs::remove_file(path);
    }

    fn unique_suffix() -> u128 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    }
}
