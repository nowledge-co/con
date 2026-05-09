# Code Editor Integration Design

**Date**: 2026-05-09  
**Status**: Draft  
**Scope**: Activity bar, file tree, editor area, `con-editor` crate

---

## Overview

con gains a lightweight code editor integrated into the main window. The editor
area sits above the terminal area in a vertically split layout. A new activity
bar on the far left replaces the current "vertical tabs is a separate setting"
model — tab management becomes one slot in the activity bar, alongside file
explorer and future panels (search, git, etc.).

The guiding constraint: **existing code changes as little as possible**. The
activity bar wraps the existing `SessionSidebar`; the terminal pane tree is
unchanged; the agent panel is unchanged. New surface area is additive.

---

## Layout

```
┌──────────────────────────────────────────────────────────────────┐
│  title bar / horizontal tab strip (existing, optional)           │
├────┬─────────────────────────────────────────────┬───────────────┤
│    │                                             │               │
│    │  left panel (240 px, collapsible)           │               │
│    │  ┌──────────────────────────────────────┐   │               │
│ A  │  │  FileTree  │  EditorView             │   │  AgentPanel   │
│ c  │  │  (240 px)  │  (flex-1)               │   │  (existing)   │
│ t  │  └──────────────────────────────────────┘   │               │
│ i  │  ← editor area (height: drag-resizable) →   │               │
│ v  ├─────────────────────────────────────────────┤               │
│ i  │                                             │               │
│ t  │  terminal area  (existing pane tree)        │               │
│ y  │                                             │               │
│    ├─────────────────────────────────────────────┤               │
│ B  │  input bar (existing)                       │               │
│ a  │                                             │               │
│ r  │                                             │               │
└────┴─────────────────────────────────────────────┴───────────────┘
 40px        flex-1                                   agent_panel_width
```

### Width budget

```
window_width = activity_bar_width(40)
             + left_panel_width(0 or 240..360, collapsible)
             + terminal_content_width(≥ 360)
             + agent_panel_width(0 or 200..600)
```

`terminal_content_left` in `render.rs` gains `ACTIVITY_BAR_WIDTH (40)` as a
fixed addend. The existing `vertical_tabs_width` calculation is unchanged — it
now describes the left panel width when the `Tabs` slot is active.

### Vertical split

The main content column (between activity bar and agent panel) is split
vertically into:

- **editor area**: height = `editor_area_height` (f32, 0.0 = collapsed).
  Minimum when open: 120 px. Default when first opened: 40% of available height.
- **horizontal resize handle**: 6 px drag target, same snap-guard pattern as
  the agent panel drag divider.
- **terminal area**: takes remaining height (minimum 120 px).

`editor_area_height` is persisted in `SessionState` alongside
`agent_panel_width`.

---

## New crate: `con-editor`

Location: `crates/con-editor/`  
Dependencies: `ropey`, `sum-tree` (already in workspace), `tree-sitter` (add),
`lsp-types` (already in workspace), `tokio`, `serde`. **Zero GPUI dependency.**

### `Buffer`

```rust
pub struct Buffer {
    rope: ropey::Rope,
    path: Option<PathBuf>,
    language: Option<Language>,
    history: EditHistory,       // undo/redo stack
    version: u64,               // incremented on every edit
    dirty: bool,
}
```

Operations: `insert`, `delete`, `replace_range`, `undo`, `redo`.  
All edits produce an `EditEvent` broadcast via a `tokio::sync::broadcast`
channel so the GPUI `EditorView` can subscribe and call `cx.notify()`.

### `Selection`

```rust
pub struct Selection {
    pub anchor: Point,   // (row, col) in UTF-16 code units — matches LSP
    pub head: Point,
}

pub struct MultiSelection {
    selections: SmallVec<[Selection; 1]>,
    primary: usize,
}
```

### `SyntaxLayer`

Wraps `tree-sitter`. Holds a `tree_sitter::Tree` and re-parses incrementally on
edit. Exposes `highlight_spans(line: u32) -> Vec<HighlightSpan>` where
`HighlightSpan` carries a `HighlightKind` enum (keyword, string, comment,
function, type, …) mapped to theme colors in `EditorView`.

Language detection: file extension → grammar. Phase 1 ships Rust, TypeScript,
Python, TOML, JSON, Markdown grammars (the languages most relevant to con's
own development workflow).

### `LspClient`

Thin async wrapper around `lsp-types` + `tokio::process`. One `LspClient` per
language server process. Lifecycle: `start`, `shutdown`. Methods:
`did_open`, `did_change`, `did_save`, `hover`, `goto_definition`,
`diagnostics_stream`.

`LspClient` is optional — the editor works without it. Phase 1 ships without
LSP; Phase 3 adds it.

### `FileTree`

```rust
pub struct FileTree {
    root: PathBuf,
    entries: Vec<FileEntry>,   // flat list, depth-encoded
    expanded: HashSet<PathBuf>,
    watcher: notify::RecommendedWatcher,
}
```

Global singleton owned by `ConWorkspace`. Root updates when
`GhosttyCwdChanged` fires on the active tab (same event already used to update
the input bar CWD display). File system changes from `notify` trigger a
`FileTreeChanged` event that `ConWorkspace` forwards to the `FileTree` entity.

---

## New UI components (in `con-app/src/`)

### `activity_bar.rs`

```rust
pub enum ActivitySlot {
    Tabs,
    Files,
    // Search, Git — future
}

pub struct ActivityBar {
    active_slot: ActivitySlot,
    left_panel_open: bool,
}
```

Events emitted: `ActivitySlotChanged { slot: ActivitySlot }`,
`ToggleLeftPanel`.

Renders as a 40 px wide column of icon buttons (Phosphor icons). Active slot
gets accent-colored icon; inactive slots get `muted_foreground`. No text
labels — icons only, consistent with the design language.

Icons:
- `Tabs` → `phosphor/terminal-window.svg`
- `Files` → `phosphor/folder-open.svg`

Clicking the active slot's icon toggles the left panel open/closed (same
behavior as clicking the active item in VS Code's activity bar).

### `file_tree_view.rs`

GPUI `Render` for `FileTree`. Virtualised list (only renders visible rows).
Row height: 24 px. Indent: 12 px per depth level. Icons: Phosphor
`folder.svg` / `folder-open.svg` / `file.svg` (language-specific file icons
are a future enhancement).

Events emitted: `OpenFile { path: PathBuf }`.

### `editor_view.rs`

GPUI `Render` for `Buffer` + `MultiSelection` + `SyntaxLayer`.

Rendering approach: per-line `StyledText` (same as the Linux terminal renderer
already in the codebase). Each line is a `StyledText` built from
`SyntaxLayer::highlight_spans`. Cursor is an absolute-positioned 2 px wide
`div` overlay. Scrolling via GPUI's built-in scroll handle.

Font: IoskeleyMono (same as terminal chrome — consistent with the design
language rule that code contexts use mono font).

Key bindings handled in `editor_view.rs` (not global actions — editor captures
keys only when focused):
- Movement: arrows, Home/End, Ctrl+Home/End, Page Up/Down
- Edit: printable chars, Backspace, Delete, Enter, Tab (inserts spaces)
- Selection: Shift+movement
- Clipboard: Cmd+C, Cmd+X, Cmd+V, Cmd+A
- Undo/Redo: Cmd+Z, Cmd+Shift+Z
- Save: Cmd+S → `fs::write` + emit `FileSaved`

---

## Workspace changes

### `workspace/mod.rs` — new state fields

```rust
// Activity bar
activity_slot: ActivitySlot,
left_panel_open: bool,

// Editor area
editor_area_height: f32,          // 0.0 = collapsed
editor_area_drag: Option<f32>,    // active drag start y
open_buffers: HashMap<PathBuf, Entity<Buffer>>,
active_buffer: Option<PathBuf>,

// Global file tree
file_tree: Entity<FileTree>,

// New UI entities
activity_bar: Entity<ActivityBar>,
editor_view: Option<Entity<EditorView>>,
file_tree_view: Entity<FileTreeView>,
```

### `workspace/render.rs` — layout changes

The existing two-column layout:

```
[sidebar?] [terminal + agent]
```

becomes three-column:

```
[activity_bar(40px)] [left_panel(0..360px)?] [main_column] [agent_panel]
```

`main_column` is a vertical flex container:

```
[editor_area(editor_area_height px)]
[resize_handle(6px)]
[terminal_area(flex-1)]
[input_bar]
```

When `editor_area_height == 0.0`, the resize handle is hidden and the
terminal area takes full height (no visible change from today).

The existing `terminal_content_left` calculation gains `ACTIVITY_BAR_WIDTH`:

```rust
let terminal_content_left = ACTIVITY_BAR_WIDTH
    + if self.left_panel_open { left_panel_width } else { 0.0 };
```

All existing snap-guard, release-cover, and seam-cover logic for the sidebar
continues to work — it now applies to the left panel (which is still backed by
`SessionSidebar` when `activity_slot == Tabs`).

### `workspace/session_state.rs` — persistence

Add to `SessionState`:

```rust
pub editor_area_height: f32,
pub left_panel_open: bool,
pub activity_slot: String,   // "tabs" | "files"
pub file_tree_root: Option<String>,
```

### New actions

```rust
actions!(
    ToggleEditorArea,       // Cmd+Shift+E
    ToggleLeftPanel,        // Cmd+B  (matches VS Code / Zed muscle memory)
    OpenFileInEditor,       // triggered by FileTreeView click
    SaveActiveBuffer,       // Cmd+S when editor focused
    CloseActiveBuffer,
);
```

---

## Agent integration

The existing `edit_file` tool in `con-agent/src/tools.rs` writes directly to
disk. In Phase 3, it gains an optional path: if the file is open in a
`Buffer`, apply the edit to the buffer (triggering live re-render) instead of
writing to disk directly. The buffer's `dirty` flag and `Cmd+S` save flow
handle the actual write.

This means the agent's file edits become visible in the editor in real time —
consistent with the "agent transparency" principle already in DESIGN.md.

---

## Implementation phases

### Phase 1 — Layout + file tree + read-only preview

Deliverables:
- `ActivityBar` component, two slots (Tabs, Files)
- `FileTree` + `FileTreeView` — directory listing, expand/collapse, cwd-tracking
- `EditorView` read-only mode — open file, render text with IoskeleyMono, scroll
- Workspace layout: editor area above terminal, drag-resizable, collapsible
- `ToggleLeftPanel` (Cmd+B), `ToggleEditorArea` (Cmd+Shift+E) actions
- Session persistence for `editor_area_height`, `left_panel_open`, `activity_slot`

No `con-editor` crate yet — Phase 1 uses `std::fs::read_to_string` directly
into a `String` held by `EditorView`.

### Phase 2 — Editable buffer + syntax highlighting

Deliverables:
- `con-editor` crate: `Buffer`, `Selection`, `MultiSelection`, `EditHistory`
- `SyntaxLayer` with tree-sitter (Rust, TS, Python, TOML, JSON, Markdown)
- `EditorView` edit mode: full key binding set, multi-cursor, Cmd+S save
- Dirty indicator (dot) in file tree row and editor tab header

### Phase 3 — LSP + agent integration

Deliverables:
- `LspClient` in `con-editor`: diagnostics, hover tooltip, go-to-definition
- Inline diagnostics rendered as underlines in `EditorView`
- `edit_file` agent tool upgraded to write through open `Buffer` when available
- Agent can open files in the editor via new `open_file` tool

---

## What does NOT change

- `SessionSidebar` — zero modifications. It becomes the content of the left
  panel when `activity_slot == Tabs`.
- `PaneTree` and all terminal pane logic — unchanged.
- `AgentPanel` — unchanged.
- `InputBar` — unchanged.
- Horizontal tab strip — unchanged.
- All existing keybindings — unchanged.
- `con-core`, `con-agent`, `con-ghostty`, `con-terminal` crates — unchanged
  until Phase 3 agent integration.

---

## Open questions (deferred)

- **Editor tabs**: when multiple files are open, show a tab strip inside the
  editor area. Deferred to Phase 2.
- **Split editor panes**: open two files side by side inside the editor area.
  Deferred post-Phase 3.
- **Language server management**: which LSP binary to use, how to install,
  per-project config. Deferred to Phase 3.
- **Windows / Linux**: activity bar and file tree are platform-agnostic GPUI
  components and will work on all platforms. `LspClient` uses `tokio::process`
  which works everywhere. No platform-specific blockers identified.
