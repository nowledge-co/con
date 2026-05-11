# Left Sidebar

Status: Implemented

The left sidebar is the workspace's navigation and project panel area. It is not
the old vertical-tabs feature.

## Layout

```text
[ActivityBar 40 px] [optional content panel] [pane tree] [agent panel]
```

The activity rail is always visible. The content panel can collapse, resize, and
switch between sidebar features.

## Slots

`ActivitySlot::Files` renders `FileTreeView`.

`ActivitySlot::Search` renders `SidebarSearchView`.

`ActivitySlot::Tabs` is legacy-only and maps to file explorer content when old
session data restores it.

## Toggle Rules

- `Cmd+B` dispatches `ToggleLeftPanel`.
- The top bar sidebar button dispatches the same behavior.
- Clicking an inactive activity icon switches slot and opens the panel.
- Clicking the active activity icon toggles the content panel.
- Collapsing the content panel never hides the 40 px icon rail.

## Width Rules

The panel can resize across the available window width after accounting for the
activity rail and agent panel. The workspace owns this drag state because it has
the full layout budget. A capture overlay is rendered during drag so releasing
the mouse outside the handle exits resize mode reliably.

## Root Sync

Files and Search share the same root:

- terminal focus uses terminal cwd,
- editor focus uses active editor file parent,
- existing roots are preserved when they already contain the focused file,
- render fallback syncs the root if no folder has been set yet.

This keeps the sidebar useful when the user opens a tab that only contains an
editor pane or when focus moves between terminal and editor surfaces.
