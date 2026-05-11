# Code Editor and Left Sidebar

Status: Implemented
Scope: editor pane surface, left sidebar activity rail, file explorer, search,
keybinding/focus integration.

## Current Model

The code editor is a normal pane surface inside the workspace `PaneTree`. It is
not a separate top-level editor area and there is no standalone `con-editor`
crate. Terminal panes, editor panes, the input bar, and the agent panel all
share the existing workspace layout and close/focus machinery.

The left side of the window is split into two layers:

- `ActivityBar`: a fixed 40 px icon rail that is always visible.
- Left panel content: user-resizable content shown only when the panel is open.

Horizontal tabs are the only tab UI. The removed vertical tabs config/session
fields are accepted only for old-file compatibility; see
`docs/impl/vertical-tabs.md`.

## Left Sidebar

`crates/con-app/src/activity_bar.rs` owns the rail. `ActivitySlot::Files` shows
the file explorer and `ActivitySlot::Search` shows workspace search. Clicking a
different icon switches content and opens the panel. Clicking the already active
icon toggles the panel open or closed.

`Cmd+B` is bound to `ToggleLeftPanel` and the user-facing label is "Toggle Left
Sidebar". The top bar sidebar button remains a first-class toggle for the same
left panel. The rail stays visible even when the panel content is collapsed so
Files/Search can reopen it directly.

The panel width is stored as `left_panel_width` in session state; old
`vertical_tabs_width` session files load through a serde alias. The active
resize gesture is owned by the workspace because it needs the full window
width, agent panel width, and pane layout constraints. While resizing,
`render.rs` installs a capture overlay so mouse movement and mouse-up events
end the drag even if the cursor leaves the handle.

## File Explorer

`FileTreeView` has an optional root. The workspace keeps it in sync with the
active focus:

- Terminal focus uses the active terminal cwd.
- Editor focus uses the active editor file's parent directory.
- If an editor file is inside the existing root, the root is preserved.
- If the root is missing at render time, the workspace performs a fallback sync
  from the currently focused pane.

Opening a file from the explorer routes through
`ConWorkspace::open_path_in_active_editor`, which reuses the active tab's shared
editor pane when possible.

## Search Panel

`SidebarSearchView` searches below the same root used by the file explorer. The
query input auto-grows from one to three lines and supports case-sensitive and
regular-expression modes.

Search intentionally has bounded work:

- `MAX_SEARCH_FILES = 800`
- `MAX_FILE_BYTES = 512 KiB`
- `MAX_RESULTS = 200`
- `MAX_MATCHES_PER_FILE = 20`

Results are grouped by file, show a per-file match count, and highlight the
matched text. The result list uses a real vertical scrollbar; it only becomes
visually relevant when the result content overflows.

## Editor Pane

`EditorView` is a lightweight multi-file editor pane:

- `EditorTab` pairs a `PathBuf`, `EditorBuffer`, and render cache.
- `EditorBuffer` owns text, cursor, selection, undo/redo, and revision state.
- Rendering uses GPUI `uniform_list` so only visible rows are laid out.
- Syntax highlighting is provided by `editor_syntax`.
- Basic language-server diagnostics are provided by `editor_lsp` when a server
  is available.
- Font family and size follow the terminal/code font settings instead of using
  a separate editor default.

The editor supports long single lines with horizontal scrolling. Cursor movement
and line-boundary actions scroll the cursor into view, including `Ctrl+A` and
`Ctrl+E`. The current cursor line renders with a subtle background, and double
click selects the word under the cursor.

Closing follows the pane model: `Cmd+W` closes editor files one by one. When the
last editor file in an editor pane closes, the pane is closed instead of
rendering a "No file open" placeholder.

## Focus and Keybindings

Editor text-editing bindings are scoped to `EditorView` so terminal keys such as
Enter and Backspace are not intercepted globally. App-level shortcuts remain
global by default so `Cmd+T`, `Cmd+W`, tab navigation, command palette, and
left-sidebar toggles still work when an editor pane is focused or when a tab
contains only an editor pane.

See `docs/impl/keybindings.md` for the binding-spec table and scope rules.

## Code Map

```text
crates/con-app/src/activity_bar.rs
  Fixed icon rail and Files/Search slot events.

crates/con-app/src/file_tree_view.rs
  File explorer rows and OpenFile events.

crates/con-app/src/sidebar_search_view.rs
  Sidebar search query/options/results rendering and bounded filesystem scan.

crates/con-app/src/editor_buffer.rs
  Text, cursor, selection, undo/redo, and line movement primitives.

crates/con-app/src/editor_view.rs
  Multi-file editor pane, tabs, hit-testing, scrolling, rendering, LSP events.

crates/con-app/src/editor_syntax.rs
  File type detection and syntax highlight runs.

crates/con-app/src/editor_lsp.rs
  Best-effort language-server process integration and diagnostics parsing.

crates/con-app/src/workspace/editor_actions.rs
  Editor action dispatch and text-key fallback handling.

crates/con-app/src/workspace/render.rs
  Activity rail, left panel layout, resize overlay, editor pane composition.
```

## Validation

Relevant checks:

- `cargo check -p con`
- `cargo test -p con workspace -- --nocapture`
- `cargo test -p con sidebar_search -- --nocapture`
- `cargo test -p con editor_view -- --nocapture`
