//! Editor view — Phase 1: read-only file viewer.
//!
//! Renders the contents of an open file using IoskeleyMono. No editing,
//! no syntax highlighting yet (Phase 2). Supports scrolling.
//!
//! The view holds the file content as a plain `Vec<String>` (one entry
//! per line). Phase 2 will replace this with a `con-editor::Buffer`.

use gpui::{
    Context, EventEmitter, IntoElement, ParentElement, Render, SharedString, Styled, Window, div,
    prelude::*, px,
};
use gpui_component::{ActiveTheme, scroll::ScrollableElement};
use std::path::PathBuf;

const LINE_HEIGHT: f32 = 20.0;
const EDITOR_FONT_SIZE: f32 = 13.0;
const GUTTER_WIDTH: f32 = 44.0;

/// Emitted when the user saves (Cmd+S). Phase 1: no-op (read-only).
#[allow(dead_code)]
pub struct FileSaved {
    pub path: PathBuf,
}

impl EventEmitter<FileSaved> for EditorView {}

pub struct EditorView {
    pub path: Option<PathBuf>,
    lines: Vec<SharedString>,
}

impl EditorView {
    pub fn new() -> Self {
        Self {
            path: None,
            lines: Vec::new(),
        }
    }

    /// Load a file from disk into the view.
    pub fn open_file(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        match std::fs::read_to_string(&path) {
            Ok(content) => {
                self.lines = content
                    .lines()
                    .map(|l| SharedString::from(l.to_string()))
                    .collect();
                // Ensure at least one line so the view isn't empty.
                if self.lines.is_empty() {
                    self.lines.push(SharedString::from(""));
                }
                self.path = Some(path);
            }
            Err(e) => {
                self.lines = vec![SharedString::from(format!("Error reading file: {e}"))];
                self.path = Some(path);
            }
        }
        cx.notify();
    }

    #[allow(dead_code)]
    pub fn is_empty(&self) -> bool {
        self.path.is_none()
    }
}

impl Render for EditorView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();

        if self.path.is_none() {
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

        let line_count = self.lines.len();
        let gutter_color = theme.muted_foreground.opacity(0.30);
        let gutter_bg = theme.muted.opacity(0.04);
        let mono_font = theme.mono_font_family.clone();

        // Header: file name
        let file_name: SharedString = self
            .path
            .as_ref()
            .and_then(|p| p.file_name())
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default()
            .into();

        let header = div()
            .h(px(28.0))
            .flex_shrink_0()
            .flex()
            .items_center()
            .px(px(12.0))
            .bg(theme.muted.opacity(0.05))
            .child(
                div()
                    .text_size(px(11.5))
                    .text_color(theme.foreground.opacity(0.70))
                    .font_family(mono_font.clone())
                    .truncate()
                    .child(file_name),
            );

        // Line rows
        let rows: Vec<_> = self
            .lines
            .iter()
            .enumerate()
            .map(|(i, line)| {
                let line_num: SharedString = format!("{}", i + 1).into();
                let line_text = line.clone();
                div()
                    .h(px(LINE_HEIGHT))
                    .w_full()
                    .flex()
                    .flex_row()
                    .items_start()
                    // Gutter
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
                                    .font_family(mono_font.clone())
                                    .child(line_num),
                            ),
                    )
                    // Line content
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
                                    .text_color(theme.foreground.opacity(0.90))
                                    .font_family(mono_font.clone())
                                    .whitespace_nowrap()
                                    .child(line_text),
                            ),
                    )
                    .into_any_element()
            })
            .collect();

        // Status bar
        let status_text: SharedString =
            format!("{} lines", line_count).into();
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

        div()
            .id("editor-view")
            .size_full()
            .flex()
            .flex_col()
            .bg(theme.background)
            .child(header)
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scrollbar()
                    .flex()
                    .flex_col()
                    .children(rows),
            )
            .child(status_bar)
            .into_any_element()
    }
}
