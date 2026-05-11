# Vertical Tabs

Status: Removed from active UI

Vertical tabs were replaced by the current left sidebar model:

- a fixed 40 px activity rail,
- Files/Search sidebar content,
- a normal horizontal tab strip for workspace tabs.

The old vertical tab strip, hover card, and pinned vertical tab panel are no
longer part of the active product surface. `Cmd+B` now toggles the left sidebar
content panel, not tab orientation.

Some code and session field names still mention `vertical_tabs` for backward
compatibility and migration:

- `Session.vertical_tabs_width` persists the user-resized left panel width.
- `Session.vertical_tabs_pinned` is legacy session data.
- `ToggleVerticalTabs` routes to the current left-sidebar action path where
  older keybinding/config names still exist.
- `SessionSidebar` remains as legacy tab/sidebar infrastructure and width
  storage, but the active left rail is `ActivityBar`.

Current behavior is documented in `docs/impl/code-editor-design.md` and
`docs/impl/left-sidebar.md`.
