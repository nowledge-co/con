# Removed Vertical Tabs Compatibility

Status: Removed from active UI

Vertical tabs were replaced by the current left sidebar model:

- a fixed 40 px activity rail,
- Files/Search sidebar content,
- a normal horizontal tab strip for workspace tabs.

The old vertical tab strip, hover card, and pinned vertical tab panel are no
longer part of the active product surface. `Cmd+B` now toggles the left sidebar
content panel, not tab orientation.

Compatibility with old user files is intentionally one-way:

- `appearance.tabs_orientation` is ignored when loading config.
- `keybindings.toggle_vertical_tabs` is accepted as an alias for
  `keybindings.toggle_left_panel`.
- `vertical_tabs_width` in session JSON is accepted as an alias for
  `left_panel_width`.
- `vertical_tabs_pinned` in session JSON is ignored.

Current behavior is documented in `docs/impl/code-editor-design.md` and
`docs/impl/left-sidebar.md`.
