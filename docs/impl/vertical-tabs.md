# Vertical Tabs

Status: Implemented

Vertical tabs remain a first-class workspace navigation surface. They now
coexist with the code-editor sidebar tools instead of being replaced by them.

## Model

The left side of the workspace has three independent concepts:

- tab orientation: `appearance.tabs_orientation = "horizontal" | "vertical"`,
- sidebar visibility: hide/unhide the whole left sidebar for a clean terminal
  view,
- vertical tab mode: fold/unfold the tab surface while the sidebar is visible.

The current layout is:

```text
[tool/session rail] [active sidebar panel] [pane tree]
```

## Folded Mode

Folded mode renders the 44 px vertical tab rail. Files/Search live at the top of
the rail, followed by session creation and session icons. Session color,
attention, drag/drop, hover-card, close, and create-tab affordances remain
available in the compact state.

## Unfolded Mode

Unfolded mode keeps the rail visible and renders the session list in the
adjacent panel, with tab titles, subtitles, rename, close, drag/drop, and
create-tab controls. Opening Files/Search switches only that adjacent panel to
the selected tool; selecting a session switches the panel back to vertical
tabs.

## Compatibility

Old user files continue to load:

- `keybindings.toggle_vertical_tabs` is accepted as an alias for
  `keybindings.toggle_left_panel`,
- `vertical_tabs_width` in session JSON is accepted as an alias for
  `left_panel_width`,
- `vertical_tabs_pinned` in session JSON persists the folded/unfolded state.

The active product behavior is documented with the whole left-sidebar system in
`docs/impl/left-sidebar.md`.
