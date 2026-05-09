//! Editor view — lightweight multi-file editor pane.
//!
//! Uses GPUI's `uniform_list` for virtualized rendering — only visible rows
//! are laid out each frame, so large files are fast.

use gpui::{
    Context, EventEmitter, InteractiveElement, IntoElement, MouseButton, ParentElement, Render,
    SharedString, Styled, UniformListScrollHandle, Window, div, px, svg, uniform_list,
};
use gpui_component::{ActiveTheme, scroll::ScrollableElement};
use std::path::{Path, PathBuf};

const LINE_HEIGHT: f32 = 20.0;
const EDITOR_FONT_SIZE: f32 = 13.0;
const GUTTER_WIDTH: f32 = 44.0;

/// Emitted when the user saves (Cmd+S). Phase 1: no-op (read-only).
#[allow(dead_code)]
pub struct FileSaved {
    pub path: PathBuf,
}

/// Emitted when the active file tab changes so the workspace can sync
/// file-tree root/highlight to the editor file's parent directory.
pub struct ActiveFileChanged {
    pub path: PathBuf,
}

impl EventEmitter<FileSaved> for EditorView {}
impl EventEmitter<ActiveFileChanged> for EditorView {}

#[derive(Clone)]
pub struct EditorTab {
    pub path: PathBuf,
    lines: Vec<SharedString>,
}

pub struct EditorView {
    tabs: Vec<EditorTab>,
    active_tab: usize,
    scroll_handle: UniformListScrollHandle,
}

impl EditorView {
    pub fn new() -> Self {
        Self {
            tabs: Vec::new(),
            active_tab: 0,
            scroll_handle: UniformListScrollHandle::new(),
        }
    }

    /// Load a file from disk into the editor pane. If the file is already open,
    /// it becomes the active editor tab; otherwise a new editor tab is appended.
    pub fn open_file(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        let content = match std::fs::read_to_string(&path) {
            Ok(content) => content,
            Err(e) => format!("Error reading file: {e}"),
        };
        self.open_file_from_content(path.clone(), content);
        cx.emit(ActiveFileChanged { path });
        cx.notify();
    }

    /// Testable core of `open_file` that avoids filesystem/GPUI coupling.
    pub fn open_file_from_content(&mut self, path: PathBuf, content: String) {
        if let Some(index) = self.tabs.iter().position(|tab| tab.path == path) {
            self.active_tab = index;
            self.scroll_handle = UniformListScrollHandle::new();
            return;
        }

        let mut lines = content
            .lines()
            .map(|l| SharedString::from(l.to_string()))
            .collect::<Vec<_>>();
        if lines.is_empty() {
            lines.push(SharedString::from(""));
        }

        self.tabs.push(EditorTab { path, lines });
        self.active_tab = self.tabs.len().saturating_sub(1);
        self.scroll_handle = UniformListScrollHandle::new();
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub fn tab_count(&self) -> usize {
        self.tabs.len()
    }

    pub fn active_path(&self) -> Option<&Path> {
        self.active_tab_ref().map(|tab| tab.path.as_path())
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub fn active_parent_dir(&self) -> Option<&Path> {
        self.active_path().and_then(Path::parent)
    }

    #[allow(dead_code)]
    pub fn contains_path(&self, path: &Path) -> bool {
        self.tabs.iter().any(|tab| tab.path == path)
    }

    pub fn activate_tab(&mut self, index: usize) {
        if index < self.tabs.len() && self.active_tab != index {
            self.active_tab = index;
            self.scroll_handle = UniformListScrollHandle::new();
        }
    }

    fn activate_tab_and_emit(&mut self, index: usize, cx: &mut Context<Self>) {
        self.activate_tab(index);
        if let Some(path) = self.active_path().map(Path::to_path_buf) {
            cx.emit(ActiveFileChanged { path });
        }
        cx.notify();
    }

    /// Close the active file tab. Returns true when no file tabs remain and
    /// the containing editor pane should be closed.
    pub fn close_active_tab(&mut self) -> bool {
        if self.tabs.is_empty() {
            return true;
        }
        let index = self.active_tab;
        self.tabs.remove(index);
        if self.tabs.is_empty() {
            self.active_tab = 0;
        } else if self.active_tab >= self.tabs.len() {
            self.active_tab = self.tabs.len() - 1;
        } else if index < self.active_tab {
            self.active_tab -= 1;
        }
        self.scroll_handle = UniformListScrollHandle::new();
        self.tabs.is_empty()
    }

    fn close_tab(&mut self, index: usize, cx: &mut Context<Self>) {
        if index >= self.tabs.len() {
            return;
        }
        self.active_tab = index;
        self.close_active_tab();
        if let Some(path) = self.active_path().map(Path::to_path_buf) {
            cx.emit(ActiveFileChanged { path });
        }
        cx.notify();
    }

    fn active_tab_ref(&self) -> Option<&EditorTab> {
        self.tabs.get(self.active_tab)
    }

    #[allow(dead_code)]
    pub fn is_empty(&self) -> bool {
        self.tabs.is_empty()
    }
}

impl Render for EditorView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();

        if self.tabs.is_empty() {
            return div()
                .size_full()
                .flex()
                .items_center()
                .justify_center()
                .bg(theme.background)
                .child(
                    div()
                        .text_size(px(12.0))
                        .text_color(theme.muted_foreground.opacity(0.5))
                        .font_family(theme.font_family.clone())
                        .child("Select a file to open"),
                )
                .into_any_element();
        }

        let active_index = self.active_tab;
        let tabs = self.tabs.clone();
        let active = tabs[active_index].clone();
        let line_count = active.lines.len();
        let gutter_color = theme.muted_foreground.opacity(0.30);
        let gutter_bg = theme.muted.opacity(0.04);
        let mono_font = theme.mono_font_family.clone();
        let ui_font = theme.font_family.clone();
        let fg = theme.foreground;
        let bg = theme.background;
        let lines = active.lines.clone();

        let mut tab_bar = div()
            .h(px(28.0))
            .flex_shrink_0()
            .flex()
            .items_center()
            .gap(px(2.0))
            .px(px(6.0))
            .bg(theme.tab_bar_segmented.opacity(0.72))
            .overflow_x_scrollbar();

        for (index, tab) in tabs.iter().enumerate() {
            let is_active = index == active_index;
            let title = tab
                .path
                .file_name()
                .map(|name| name.to_string_lossy().to_string())
                .unwrap_or_else(|| tab.path.display().to_string());
            let activate_index = index;
            let close_index = index;
            let mut tab_el = div()
                .id(("editor-file-tab", index))
                .h(px(22.0))
                .max_w(px(180.0))
                .flex_shrink_0()
                .flex()
                .items_center()
                .gap(px(5.0))
                .pl(px(8.0))
                .pr(px(3.0))
                .rounded(px(6.0))
                .cursor_pointer()
                .bg(if is_active { theme.tab_active } else { theme.transparent })
                .hover(move |s| s.bg(if is_active { theme.tab_active } else { fg.opacity(0.08) }))
                .on_mouse_down(MouseButton::Left, cx.listener(move |this, _event, _window, cx| {
                    this.activate_tab_and_emit(activate_index, cx);
                }))
                .child(
                    div()
                        .truncate()
                        .text_size(px(12.0))
                        .line_height(px(14.0))
                        .font_family(ui_font.clone())
                        .text_color(if is_active { fg.opacity(0.92) } else { fg.opacity(0.58) })
                        .child(SharedString::from(title)),
                );

            tab_el = tab_el.child(
                div()
                    .size(px(14.0))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(px(4.0))
                    .text_size(px(11.0))
                    .text_color(fg.opacity(if is_active { 0.55 } else { 0.42 }))
                    .hover(|s| s.bg(gpui::black().opacity(0.08)))
                    .on_mouse_down(MouseButton::Left, cx.listener(move |this, _event, window, cx| {
                        cx.stop_propagation();
                        window.prevent_default();
                        this.close_tab(close_index, cx);
                    }))
                    .child(
                        svg()
                            .path("phosphor/x.svg")
                            .size(px(8.0))
                            .text_color(fg.opacity(if is_active { 0.55 } else { 0.42 })),
                    ),
            );

            tab_bar = tab_bar.child(tab_el);
        }

        let status_text: SharedString = format!(
            "{} lines — {}",
            line_count,
            active.path.display()
        )
        .into();
        let status_bar = div()
            .h(px(22.0))
            .flex_shrink_0()
            .flex()
            .items_center()
            .px(px(12.0))
            .bg(theme.muted.opacity(0.05))
            .child(
                div()
                    .text_size(px(10.5))
                    .text_color(theme.muted_foreground.opacity(0.55))
                    .font_family(mono_font.clone())
                    .child(status_text),
            );

        let mono_font_list = mono_font.clone();
        let list = uniform_list(
            "editor-lines",
            line_count,
            move |range, _window, _cx| {
                range
                    .map(|i| {
                        let line_num: SharedString = format!("{}", i + 1).into();
                        let line_text = lines[i].clone();
                        div()
                            .h(px(LINE_HEIGHT))
                            .w_full()
                            .flex()
                            .flex_row()
                            .items_start()
                            .bg(bg)
                            .child(
                                div()
                                    .w(px(GUTTER_WIDTH))
                                    .flex_shrink_0()
                                    .h(px(LINE_HEIGHT))
                                    .flex()
                                    .items_center()
                                    .justify_end()
                                    .pr(px(10.0))
                                    .bg(gutter_bg)
                                    .child(
                                        div()
                                            .text_size(px(EDITOR_FONT_SIZE - 1.0))
                                            .text_color(gutter_color)
                                            .font_family(mono_font_list.clone())
                                            .child(line_num),
                                    ),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .h(px(LINE_HEIGHT))
                                    .flex()
                                    .items_center()
                                    .pl(px(12.0))
                                    .child(
                                        div()
                                            .text_size(px(EDITOR_FONT_SIZE))
                                            .text_color(fg.opacity(0.90))
                                            .font_family(mono_font_list.clone())
                                            .whitespace_nowrap()
                                            .child(line_text),
                                    ),
                            )
                    })
                    .collect()
            },
        )
        .flex_1()
        .min_h_0()
        .track_scroll(&self.scroll_handle);

        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(theme.background)
            .child(tab_bar)
            .child(list)
            .child(status_bar)
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn open_file_adds_tabs_and_focuses_existing_path() {
        let mut editor = EditorView::new();
        let first = PathBuf::from("/tmp/project-a/src/main.rs");
        let second = PathBuf::from("/tmp/project-b/README.md");

        editor.open_file_from_content(first.clone(), "fn main() {}".into());
        editor.open_file_from_content(second.clone(), "# Project B".into());
        editor.open_file_from_content(first.clone(), "fn main() {}".into());

        assert_eq!(editor.tab_count(), 2);
        assert_eq!(editor.active_path(), Some(first.as_path()));
    }

    #[test]
    fn close_active_tab_removes_only_current_file_until_empty() {
        let mut editor = EditorView::new();
        let first = PathBuf::from("/tmp/project-a/src/main.rs");
        let second = PathBuf::from("/tmp/project-b/README.md");

        editor.open_file_from_content(first.clone(), "fn main() {}".into());
        editor.open_file_from_content(second.clone(), "# Project B".into());

        assert!(!editor.close_active_tab());
        assert_eq!(editor.tab_count(), 1);
        assert_eq!(editor.active_path(), Some(first.as_path()));

        assert!(editor.close_active_tab());
        assert_eq!(editor.tab_count(), 0);
        assert_eq!(editor.active_path(), None);
    }

    #[test]
    fn active_parent_dir_tracks_active_editor_tab() {
        let mut editor = EditorView::new();
        let first = PathBuf::from("/tmp/project-a/src/main.rs");
        let second = PathBuf::from("/tmp/project-b/README.md");

        editor.open_file_from_content(first, "fn main() {}".into());
        editor.open_file_from_content(second.clone(), "# Project B".into());

        assert_eq!(editor.active_parent_dir(), Some(Path::new("/tmp/project-b")));

        editor.activate_tab(0);
        assert_eq!(editor.active_parent_dir(), Some(Path::new("/tmp/project-a/src")));
    }
}
