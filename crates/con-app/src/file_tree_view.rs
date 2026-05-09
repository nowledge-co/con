//! File tree panel — shows the directory tree rooted at the active tab's cwd.
//!
//! Phase 1: read-only directory listing. Clicking a file emits `OpenFile`.
//! The tree root updates when `set_root` is called (driven by GhosttyCwdChanged
//! on the active tab).
//!
//! Visual rules
//! ---
//! - Row height: 24 px.
//! - Indent: 12 px per depth level.
//! - Icons: phosphor/folder.svg, phosphor/folder-open.svg, phosphor/file-text.svg.
//! - Active (open) file row gets a subtle accent bg.
//! - No borders — surface separation via bg opacity.

use gpui::{
    Context, EventEmitter, IntoElement, MouseButton, MouseDownEvent, ParentElement, Render,
    SharedString, Styled, Window, div, prelude::*, px, svg,
};
use gpui_component::{ActiveTheme, scroll::ScrollableElement};
use std::path::{Path, PathBuf};

const ROW_HEIGHT: f32 = 24.0;
const INDENT_PER_LEVEL: f32 = 12.0;
const ICON_SIZE: f32 = 13.0;

/// A single entry in the flat file tree list.
#[derive(Clone)]
pub struct FileEntry {
    pub path: PathBuf,
    pub name: String,
    pub depth: usize,
    pub is_dir: bool,
    pub is_expanded: bool,
}

/// Emitted when the user clicks a file row.
pub struct OpenFile {
    pub path: PathBuf,
}

impl EventEmitter<OpenFile> for FileTreeView {}

pub struct FileTreeView {
    root: Option<PathBuf>,
    entries: Vec<FileEntry>,
    /// Path of the currently open file (highlighted row).
    active_path: Option<PathBuf>,
}

impl FileTreeView {
    pub fn new() -> Self {
        Self {
            root: None,
            entries: Vec::new(),
            active_path: None,
        }
    }

    /// Set the root directory and rebuild the entry list.
    pub fn set_root(&mut self, root: PathBuf, cx: &mut Context<Self>) {
        if self.root.as_deref() == Some(root.as_path()) {
            return;
        }
        self.root = Some(root.clone());
        self.entries = build_root_entries(&root);
        cx.notify();
    }

    pub fn set_active_path(&mut self, path: Option<PathBuf>, cx: &mut Context<Self>) {
        if self.active_path != path {
            self.active_path = path;
            cx.notify();
        }
    }

    /// Toggle expand/collapse for a directory entry.
    fn toggle_dir(&mut self, path: &Path, cx: &mut Context<Self>) {
        let Some(idx) = self.entries.iter().position(|e| e.path == path) else {
            return;
        };
        let entry = &mut self.entries[idx];
        if !entry.is_dir {
            return;
        }
        entry.is_expanded = !entry.is_expanded;
        let expanded = entry.is_expanded;
        let depth = entry.depth;

        if expanded {
            // Insert children after this entry.
            let children = build_entries(path, depth + 1, false);
            let insert_at = idx + 1;
            for (i, child) in children.into_iter().enumerate() {
                self.entries.insert(insert_at + i, child);
            }
        } else {
            // Remove all descendants (depth > current depth).
            let remove_start = idx + 1;
            let remove_end = self.entries[remove_start..]
                .iter()
                .position(|e| e.depth <= depth)
                .map(|rel| remove_start + rel)
                .unwrap_or(self.entries.len());
            self.entries.drain(remove_start..remove_end);
        }
        cx.notify();
    }
}

/// Build the visible tree starting at `root` itself. The root row is shown as
/// an expanded directory so the sidebar has a clear parent label and can be
/// collapsed/expanded like any other folder.
fn build_root_entries(root: &Path) -> Vec<FileEntry> {
    let name = root
        .file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_else(|| root.display().to_string());
    let mut entries = vec![FileEntry {
        path: root.to_path_buf(),
        name,
        depth: 0,
        is_dir: true,
        is_expanded: true,
    }];
    entries.extend(build_entries(root, 1, false));
    entries
}

/// Build a flat entry list for `dir` at `depth`. Only one level deep
/// (children of expanded dirs are inserted lazily by `toggle_dir`).
fn build_entries(dir: &Path, depth: usize, _expand_root: bool) -> Vec<FileEntry> {
    let Ok(read_dir) = std::fs::read_dir(dir) else {
        return Vec::new();
    };

    let mut dirs: Vec<FileEntry> = Vec::new();
    let mut files: Vec<FileEntry> = Vec::new();

    for entry in read_dir.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        // Skip hidden files/dirs (dot-prefixed).
        if name.starts_with('.') {
            continue;
        }
        let is_dir = path.is_dir();
        let fe = FileEntry {
            path,
            name,
            depth,
            is_dir,
            is_expanded: false,
        };
        if is_dir {
            dirs.push(fe);
        } else {
            files.push(fe);
        }
    }

    dirs.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    files.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));

    let mut result = Vec::new();
    result.extend(dirs);
    result.extend(files);

    result
}

impl Render for FileTreeView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();

        if self.root.is_none() {
            return div()
                .size_full()
                .flex()
                .items_center()
                .justify_center()
                .child(
                    div()
                        .text_size(px(11.0))
                        .text_color(theme.muted_foreground.opacity(0.5))
                        .font_family(theme.font_family.clone())
                        .child("No folder open"),
                )
                .into_any_element();
        }

        let active_path = self.active_path.clone();
        let accent_bg = theme.primary.opacity(0.10);
        let hover_bg = theme.muted.opacity(0.08);

        let rows: Vec<_> = self
            .entries
            .iter()
            .enumerate()
            .map(|(idx, entry)| {
                let path = entry.path.clone();
                let name: SharedString = entry.name.clone().into();
                let depth = entry.depth;
                let is_dir = entry.is_dir;
                let is_expanded = entry.is_expanded;
                let is_active = active_path.as_deref() == Some(entry.path.as_path());

                let indent = INDENT_PER_LEVEL * depth as f32 + 8.0;

                let disclosure_icon = if is_dir {
                    Some(if is_expanded {
                        "phosphor/caret-down.svg"
                    } else {
                        "phosphor/caret-right.svg"
                    })
                } else {
                    None
                };

                let icon = if is_dir {
                    if is_expanded {
                        "phosphor/folder-open.svg"
                    } else {
                        "phosphor/folder.svg"
                    }
                } else {
                    "phosphor/file-text.svg"
                };

                let icon_color = if is_dir {
                    theme.primary.opacity(0.75)
                } else {
                    theme.muted_foreground.opacity(0.80)
                };

                let text_color = if is_active {
                    theme.foreground
                } else {
                    theme.foreground.opacity(0.85)
                };

                let row_bg = if is_active { accent_bg } else { theme.transparent };

                div()
                    .id(("file-row", idx))
                    .h(px(ROW_HEIGHT))
                    .w_full()
                    .flex()
                    .items_center()
                    .pl(px(indent))
                    .gap(px(5.0))
                    .bg(row_bg)
                    .cursor_pointer()
                    .hover(move |s| {
                        if is_active {
                            s.bg(accent_bg)
                        } else {
                            s.bg(hover_bg)
                        }
                    })
                    .child(if let Some(disclosure_icon) = disclosure_icon {
                        svg()
                            .path(disclosure_icon)
                            .size(px(10.0))
                            .flex_shrink_0()
                            .text_color(theme.muted_foreground.opacity(0.62))
                            .into_any_element()
                    } else {
                        div().w(px(10.0)).flex_shrink_0().into_any_element()
                    })
                    .child(
                        svg()
                            .path(icon)
                            .size(px(ICON_SIZE))
                            .flex_shrink_0()
                            .text_color(icon_color),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_size(px(12.0))
                            .text_color(text_color)
                            .font_family(theme.font_family.clone())
                            .child(name),
                    )
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, _: &MouseDownEvent, _window, cx| {
                            if is_dir {
                                this.toggle_dir(&path, cx);
                            } else {
                                cx.emit(OpenFile { path: path.clone() });
                            }
                        }),
                    )
                    .into_any_element()
            })
            .collect();

        div()
            .id("file-tree")
            .size_full()
            .overflow_y_scrollbar()
            .flex()
            .flex_col()
            .children(rows)
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_tree() -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "con-file-tree-test-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(root.join("README.md"), "readme").unwrap();
        fs::write(root.join("src/main.rs"), "fn main() {}").unwrap();
        root
    }

    #[test]
    fn root_directory_is_rendered_as_first_expanded_entry() {
        let root = temp_tree();
        let entries = build_root_entries(&root);

        assert_eq!(entries.first().unwrap().path, root);
        assert_eq!(entries.first().unwrap().depth, 0);
        assert!(entries.first().unwrap().is_dir);
        assert!(entries.first().unwrap().is_expanded);
    }

    #[test]
    fn root_children_start_at_depth_one() {
        let root = temp_tree();
        let entries = build_root_entries(&root);

        assert!(entries.iter().skip(1).all(|entry| entry.depth == 1));
        assert!(entries.iter().any(|entry| entry.name == "src"));
        assert!(entries.iter().any(|entry| entry.name == "README.md"));
    }
}
