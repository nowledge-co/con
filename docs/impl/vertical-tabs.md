# Vertical Tabs

Status: Implemented

Vertical tabs remain a first-class workspace navigation surface. They now
coexist with the code-editor sidebar tools instead of being replaced by them.

## Model

The left side of the workspace has two independent concepts:

- sidebar visibility: hide/unhide the whole left sidebar for a clean terminal
  view,
- vertical tab mode: fold/unfold the tab surface while the sidebar is visible.

The current layout is:

```text
[vertical tab rail or panel] [Files/Search sections] [pane tree]
```

## Folded Mode

Folded mode renders the 44 px vertical tab rail. It keeps session color,
attention, drag/drop, hover-card, close, and create-tab affordances available in
the compact state.

## Unfolded Mode

Unfolded mode renders the pinned vertical tab panel with tab titles, subtitles,
rename, close, drag/drop, and create-tab controls. The file/search tools stay
beside it; unfolding tabs does not remove editor navigation.

## Compatibility

Old user files continue to load:

- `keybindings.toggle_vertical_tabs` is accepted as an alias for
  `keybindings.toggle_left_panel`,
- `vertical_tabs_width` in session JSON is accepted as an alias for
  `left_panel_width`.

The active product behavior is documented with the whole left-sidebar system in
`docs/impl/left-sidebar.md`.
