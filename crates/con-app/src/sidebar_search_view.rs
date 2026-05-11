//! Sidebar search panel — searches files below the active sidebar root.

use crate::file_tree_view::OpenFile;
use gpui::{
    Context, Div, Entity, EventEmitter, IntoElement, MouseButton, MouseDownEvent, ParentElement,
    Render, ScrollHandle, SharedString, StatefulInteractiveElement, Styled, StyledText, TextStyle,
    WhiteSpace, Window, div, prelude::*, px, svg,
};
use gpui_component::{
    ActiveTheme,
    input::{Input, InputState},
    scroll::{Scrollbar, ScrollbarShow},
};
use regex::{Regex, RegexBuilder};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};

const MAX_SEARCH_FILES: usize = 800;
const MAX_FILE_BYTES: u64 = 512 * 1024;
const MAX_RESULTS: usize = 200;
const MAX_MATCHES_PER_FILE: usize = 20;

#[derive(Clone, Debug, PartialEq, Eq)]
struct SearchMatch {
    path: PathBuf,
    line_number: usize,
    line: String,
    match_start: usize,
    match_len: usize,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct SearchOptions {
    case_sensitive: bool,
    regex: bool,
}

enum SearchMatcher {
    Literal {
        needle: String,
        needle_lower: String,
        case_sensitive: bool,
    },
    Regex(Regex),
}

impl SearchMatcher {
    fn new(query: &str, options: SearchOptions) -> Option<Self> {
        let query = query.trim();
        if query.is_empty() {
            return None;
        }
        if options.regex {
            let regex = RegexBuilder::new(query)
                .case_insensitive(!options.case_sensitive)
                .build()
                .ok()?;
            Some(Self::Regex(regex))
        } else {
            Some(Self::Literal {
                needle: query.to_string(),
                needle_lower: query.to_lowercase(),
                case_sensitive: options.case_sensitive,
            })
        }
    }

    fn find(&self, line: &str) -> Option<(usize, usize)> {
        match self {
            Self::Literal {
                needle,
                needle_lower,
                case_sensitive,
            } => {
                if *case_sensitive {
                    line.find(needle).map(|start| (start, needle.len()))
                } else {
                    line.to_lowercase()
                        .find(needle_lower)
                        .map(|start| (start, needle_lower.len()))
                }
            }
            Self::Regex(regex) => regex
                .find(line)
                .map(|matched| (matched.start(), matched.end() - matched.start())),
        }
    }
}

impl EventEmitter<OpenFile> for SidebarSearchView {}

pub struct SidebarSearchView {
    root: Option<PathBuf>,
    query: Entity<InputState>,
    results_scroll_handle: ScrollHandle,
    query_text: String,
    options: SearchOptions,
    results: Vec<SearchMatch>,
}

impl SidebarSearchView {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let query = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("Search")
                .auto_grow(1, 3)
        });

        Self {
            root: None,
            query,
            results_scroll_handle: ScrollHandle::default(),
            query_text: String::new(),
            options: SearchOptions::default(),
            results: Vec::new(),
        }
    }

    pub fn set_root(&mut self, root: PathBuf, cx: &mut Context<Self>) {
        if self.root.as_deref() == Some(root.as_path()) {
            return;
        }
        self.root = Some(root);
        self.refresh_results(cx);
    }

    fn refresh_results(&mut self, cx: &mut Context<Self>) {
        self.update_results();
        cx.notify();
    }

    fn update_results(&mut self) {
        self.results = match self.root.as_deref() {
            Some(root) => search_files(root, &self.query_text, self.options),
            None => Vec::new(),
        };
    }

    fn toggle_case_sensitive(&mut self, cx: &mut Context<Self>) {
        self.options.case_sensitive = !self.options.case_sensitive;
        self.refresh_results(cx);
    }

    fn toggle_regex(&mut self, cx: &mut Context<Self>) {
        self.options.regex = !self.options.regex;
        self.refresh_results(cx);
    }
}

fn search_files(root: &Path, query: &str, options: SearchOptions) -> Vec<SearchMatch> {
    let Some(matcher) = SearchMatcher::new(query, options) else {
        return Vec::new();
    };
    let mut files_seen = 0;
    let mut results = Vec::new();
    search_dir(root, &matcher, &mut files_seen, &mut results);
    results
}

fn search_dir(
    dir: &Path,
    matcher: &SearchMatcher,
    files_seen: &mut usize,
    results: &mut Vec<SearchMatch>,
) {
    if results.len() >= MAX_RESULTS || *files_seen >= MAX_SEARCH_FILES {
        return;
    }

    let Ok(read_dir) = std::fs::read_dir(dir) else {
        return;
    };
    let mut entries = read_dir.flatten().collect::<Vec<_>>();
    entries.sort_by_key(|entry| entry.file_name().to_string_lossy().to_lowercase());

    for entry in entries {
        if results.len() >= MAX_RESULTS || *files_seen >= MAX_SEARCH_FILES {
            return;
        }
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with('.') || matches!(name.as_str(), "target" | "node_modules" | "dist") {
            continue;
        }
        if path.is_dir() {
            search_dir(&path, matcher, files_seen, results);
        } else {
            *files_seen += 1;
            search_file(&path, matcher, results);
        }
    }
}

fn search_file(path: &Path, matcher: &SearchMatcher, results: &mut Vec<SearchMatch>) {
    let Ok(metadata) = std::fs::metadata(path) else {
        return;
    };
    if metadata.len() > MAX_FILE_BYTES {
        return;
    }

    let Ok(content) = std::fs::read_to_string(path) else {
        return;
    };

    let mut matches_for_file = 0;
    for (line_idx, line) in content.lines().enumerate() {
        if matches_for_file >= MAX_MATCHES_PER_FILE || results.len() >= MAX_RESULTS {
            return;
        }
        let Some((match_start, match_len)) = matcher.find(line) else {
            continue;
        };
        results.push(SearchMatch {
            path: path.to_path_buf(),
            line_number: line_idx + 1,
            line: line.to_string(),
            match_start,
            match_len,
        });
        matches_for_file += 1;
    }
}

impl Render for SidebarSearchView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let next_query = self.query.read(cx).value().to_string();
        if next_query != self.query_text {
            self.query_text = next_query;
            self.update_results();
        }

        let root = self.root.clone();
        let query_empty = self.query_text.trim().is_empty();
        let results = self.results.clone();
        let match_counts = file_match_counts(&results);
        let case_sensitive = self.options.case_sensitive;
        let regex_enabled = self.options.regex;
        let mut rows = Vec::new();

        if root.is_none() {
            rows.push(
                empty_state("No folder open", theme)
                    .id("search-empty-root")
                    .into_any_element(),
            );
        } else if query_empty {
            rows.push(
                empty_state("Search files", theme)
                    .id("search-empty-query")
                    .into_any_element(),
            );
        } else if results.is_empty() {
            rows.push(
                empty_state("No results", theme)
                    .id("search-empty-results")
                    .into_any_element(),
            );
        } else {
            let mut current_path: Option<PathBuf> = None;
            for (idx, result) in results.into_iter().enumerate() {
                if current_path.as_deref() != Some(result.path.as_path()) {
                    current_path = Some(result.path.clone());
                    let match_count = match_counts.get(&result.path).copied().unwrap_or(0);
                    let display = root
                        .as_deref()
                        .and_then(|root| result.path.strip_prefix(root).ok())
                        .unwrap_or(result.path.as_path())
                        .display()
                        .to_string();
                    rows.push(
                        div()
                            .id(("search-file", idx))
                            .h(px(24.0))
                            .flex()
                            .items_center()
                            .gap(px(6.0))
                            .px(px(10.0))
                            .mt(px(8.0))
                            .child(
                                svg()
                                    .path("phosphor/file-text.svg")
                                    .size(px(13.0))
                                    .text_color(theme.muted_foreground.opacity(0.72)),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .truncate()
                                    .text_size(px(12.0))
                                    .font_family(theme.font_family.clone())
                                    .text_color(theme.foreground.opacity(0.82))
                                    .child(SharedString::from(display)),
                            )
                            .child(result_count_badge(match_count, theme))
                            .into_any_element(),
                    );
                }

                let path = result.path.clone();
                let line_number = result.line_number;
                let (preview, match_start, match_len) =
                    result_preview_match(&result.line, result.match_start, result.match_len);
                rows.push(
                    div()
                        .id(("search-result", idx))
                        .h(px(24.0))
                        .flex()
                        .items_center()
                        .gap(px(8.0))
                        .pl(px(30.0))
                        .pr(px(10.0))
                        .cursor_pointer()
                        .hover(|s| s.bg(theme.muted.opacity(0.08)))
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(move |_this, _: &MouseDownEvent, _window, cx| {
                                cx.emit(OpenFile { path: path.clone() });
                            }),
                        )
                        .child(
                            div()
                                .w(px(28.0))
                                .text_size(px(10.0))
                                .font_family(theme.mono_font_family.clone())
                                .text_color(theme.muted_foreground.opacity(0.55))
                                .child(SharedString::from(line_number.to_string())),
                        )
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .truncate()
                                .text_size(px(12.0))
                                .font_family(theme.mono_font_family.clone())
                                .text_color(theme.foreground.opacity(0.78))
                                .child(highlighted_result_text(
                                    &preview,
                                    match_start,
                                    match_len,
                                    theme,
                                )),
                        )
                        .into_any_element(),
                );
            }
        }

        div()
            .id("sidebar-search")
            .size_full()
            .flex()
            .flex_col()
            .child(
                div()
                    .px(px(10.0))
                    .pt(px(10.0))
                    .pb(px(8.0))
                    .flex()
                    .flex_col()
                    .gap(px(8.0))
                    .child(
                        div()
                            .text_size(px(13.0))
                            .font_family(theme.font_family.clone())
                            .text_color(theme.foreground.opacity(0.72))
                            .child("Search"),
                    )
                    .child(
                        div()
                            .min_h(px(42.0))
                            .max_h(px(92.0))
                            .flex()
                            .flex_col()
                            .gap(px(6.0))
                            .px(px(10.0))
                            .py(px(7.0))
                            .rounded(px(6.0))
                            .bg(theme.foreground.opacity(0.055))
                            .child(
                                div()
                                    .flex()
                                    .items_start()
                                    .gap(px(8.0))
                                    .child(
                                        svg()
                                            .path("phosphor/magnifying-glass.svg")
                                            .size(px(14.0))
                                            .mt(px(3.0))
                                            .text_color(theme.muted_foreground.opacity(0.72)),
                                    )
                                    .child(
                                        div().flex_1().min_w_0().child(
                                            Input::new(&self.query)
                                                .appearance(false)
                                                .text_size(px(13.0))
                                                .line_height(px(18.0)),
                                        ),
                                    ),
                            )
                            .child(
                                div()
                                    .flex()
                                    .justify_end()
                                    .gap(px(4.0))
                                    .child(search_option_button(
                                        "search-case-sensitive",
                                        "Aa",
                                        case_sensitive,
                                        theme,
                                        cx.listener(|this, _: &MouseDownEvent, _window, cx| {
                                            this.toggle_case_sensitive(cx);
                                        }),
                                    ))
                                    .child(search_option_button(
                                        "search-regex",
                                        ".*",
                                        regex_enabled,
                                        theme,
                                        cx.listener(|this, _: &MouseDownEvent, _window, cx| {
                                            this.toggle_regex(cx);
                                        }),
                                    )),
                            ),
                    ),
            )
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .relative()
                    .child(
                        div()
                            .id("search-results-scroll")
                            .size_full()
                            .overflow_y_scroll()
                            .track_scroll(&self.results_scroll_handle)
                            .child(div().flex().flex_col().children(rows)),
                    )
                    .child(
                        Scrollbar::vertical(&self.results_scroll_handle)
                            .scrollbar_show(ScrollbarShow::Always),
                    ),
            )
            .into_any_element()
    }
}

fn result_preview_match(
    line: &str,
    match_start: usize,
    match_len: usize,
) -> (String, usize, usize) {
    let preview = line.trim_start();
    let trim_offset = line.len() - preview.len();
    let preview_len = preview.len();
    let match_end = match_start.saturating_add(match_len);
    let visible_start = match_start.saturating_sub(trim_offset).min(preview_len);
    let visible_end = match_end.saturating_sub(trim_offset).min(preview_len);

    (
        preview.to_string(),
        visible_start,
        visible_end.saturating_sub(visible_start),
    )
}

fn file_match_counts(results: &[SearchMatch]) -> HashMap<PathBuf, usize> {
    let mut counts = HashMap::new();
    for result in results {
        *counts.entry(result.path.clone()).or_insert(0) += 1;
    }
    counts
}

fn highlighted_result_text(
    preview: &str,
    match_start: usize,
    match_len: usize,
    theme: &gpui_component::Theme,
) -> StyledText {
    let text = SharedString::from(preview.to_string());
    let base_style = TextStyle {
        color: theme.foreground.opacity(0.78),
        font_family: theme.mono_font_family.clone(),
        font_size: px(12.0).into(),
        line_height: px(18.0).into(),
        white_space: WhiteSpace::Nowrap,
        ..Default::default()
    };
    let mut runs = Vec::new();
    let match_start = match_start.min(preview.len());
    let match_len = match_len.min(preview.len().saturating_sub(match_start));
    let match_end = match_start + match_len;

    if match_start > 0 {
        runs.push(base_style.to_run(match_start));
    }
    if match_len > 0 {
        let mut match_style = base_style.clone();
        match_style.color = theme.foreground.opacity(0.96);
        match_style.background_color = Some(theme.warning.opacity(0.34));
        runs.push(match_style.to_run(match_len));
    }
    if match_end < preview.len() {
        runs.push(base_style.to_run(preview.len() - match_end));
    }
    if runs.is_empty() {
        runs.push(base_style.to_run(preview.len()));
    }

    StyledText::new(text).with_runs(runs)
}

fn result_count_badge(count: usize, theme: &gpui_component::Theme) -> Div {
    div()
        .min_w(px(20.0))
        .h(px(20.0))
        .px(px(6.0))
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(6.0))
        .bg(theme.primary.opacity(0.24))
        .text_size(px(11.0))
        .font_family(theme.mono_font_family.clone())
        .text_color(theme.foreground.opacity(0.82))
        .child(SharedString::from(count.to_string()))
}

fn search_option_button<F>(
    id: &'static str,
    label: &'static str,
    active: bool,
    theme: &gpui_component::Theme,
    handler: F,
) -> impl IntoElement
where
    F: Fn(&MouseDownEvent, &mut Window, &mut gpui::App) + 'static,
{
    div()
        .id(id)
        .h(px(24.0))
        .min_w(px(28.0))
        .px(px(6.0))
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(5.0))
        .cursor_pointer()
        .bg(if active {
            theme.primary.opacity(0.18)
        } else {
            theme.transparent
        })
        .hover(|s| s.bg(theme.muted.opacity(0.12)))
        .text_size(px(11.0))
        .font_family(theme.font_family.clone())
        .text_color(if active {
            theme.primary
        } else {
            theme.muted_foreground.opacity(0.86)
        })
        .child(label)
        .on_mouse_down(MouseButton::Left, handler)
}

fn empty_state(text: &'static str, theme: &gpui_component::Theme) -> Div {
    div()
        .size_full()
        .flex()
        .items_center()
        .justify_center()
        .text_size(px(11.0))
        .font_family(theme.font_family.clone())
        .text_color(theme.muted_foreground.opacity(0.5))
        .child(text)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_search_tree() -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "con-sidebar-search-test-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(root.join("README.md"), "hello world\n").unwrap();
        fs::write(root.join("src/main.rs"), "fn main() {}\nlet needle = 1;\n").unwrap();
        fs::write(root.join("src/lib.rs"), "Needle again\n").unwrap();
        root
    }

    #[test]
    fn search_files_finds_case_insensitive_matches_recursively() {
        let root = temp_search_tree();
        let results = search_files(&root, "needle", SearchOptions::default());

        assert_eq!(results.len(), 2);
        assert!(
            results
                .iter()
                .any(|result| result.path.ends_with("main.rs") && result.line_number == 2)
        );
        assert!(
            results
                .iter()
                .any(|result| result.path.ends_with("lib.rs") && result.line_number == 1)
        );

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn search_files_ignores_empty_query() {
        let root = temp_search_tree();
        assert!(search_files(&root, " ", SearchOptions::default()).is_empty());

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn search_files_respects_case_sensitive_option() {
        let root = temp_search_tree();
        let results = search_files(
            &root,
            "Needle",
            SearchOptions {
                case_sensitive: true,
                regex: false,
            },
        );

        assert_eq!(results.len(), 1);
        assert!(results[0].path.ends_with("lib.rs"));

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn search_files_supports_regex_option() {
        let root = temp_search_tree();
        let results = search_files(
            &root,
            r"need(le)?\s*=\s*1",
            SearchOptions {
                case_sensitive: false,
                regex: true,
            },
        );

        assert_eq!(results.len(), 1);
        assert!(results[0].path.ends_with("main.rs"));

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn result_preview_match_keeps_highlight_aligned_after_trimming_indent() {
        let (preview, match_start, match_len) = result_preview_match("    let needle = 1;", 8, 6);

        assert_eq!(preview, "let needle = 1;");
        assert_eq!(match_start, 4);
        assert_eq!(match_len, 6);
    }
}
