//! Editor view — Phase 1: read-only file viewer.
//!
//! Uses GPUI's `uniform_list` for virtualized rendering — only visible rows
//! are laid out each frame, so large files are fast.

use gpui::{
    Context, EventEmitter, IntoElement, ParentElement, Render, SharedString, Styled,
    UniformListScrollHandle, Window, div, prelude::*, px, uniform_list,
};
use gpui_component::ActiveTheme;
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
    scroll_handle: UniformListScrollHandle,
}

impl EditorView {
    pub fn new() -> Self {
        Self {
            path: None,
            lines: Vec::new(),
            scroll_handle: UniformListScrollHandle::new(),
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
        // Reset scroll to top on new file.
        self.scroll_handle = UniformListScrollHandle::new();
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
        let fg = theme.foreground;
        let bg = theme.background;
        let lines = self.lines.clone();

        // Status bar
        let status_text: SharedString = format!("{} lines", line_count).into();
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
                                            .font_family(mono_font_list.clone())
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
            .child(list)
            .child(status_bar)
            .into_any_element()
    }
}
