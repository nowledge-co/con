# Left Sidebar

Status: Implemented

The left sidebar is the workspace's navigation and project panel area. It is not
the workspace tab identity by itself; vertical tabs remain the primary tab
rail/panel inside the sidebar.

## Layout

```text
[optional tool/session rail] [active sidebar panel] [pane tree] [agent panel]
```

When the left sidebar is hidden, none of the three sidebar columns render. This
preserves the clean terminal view users expect from the sidebar toggle.

When the left sidebar is visible:

- vertical tabs can stay folded as a 44 px rail or unfold into the pinned tab
  panel when `appearance.tabs_orientation = "vertical"`,
- the folded rail puts Files/Search first, then session controls and sessions,
- Files/Search and sessions are one sidebar system: opening Files/Search keeps a
  thin session/tool rail and swaps the adjacent panel to Files or Search.

## Slots

`ActivitySlot::Files` renders `FileTreeView`.

`ActivitySlot::Search` renders `SidebarSearchView`.

## Toggle Rules

- `Cmd+B` dispatches `ToggleLeftPanel`.
- The top bar sidebar button dispatches the same behavior.
- The toggle hides or unhides the whole sidebar, including vertical tabs and
  file/search drawer controls.
- The vertical tab collapse/expand button only changes folded/unfolded tab
  mode while the sidebar is visible.
- Clicking Files or Search switches the active sidebar panel.
- Clicking a session switches the active sidebar panel back to tabs/sessions.

## Width Rules

The file/search panel can resize across the available window width. In both
horizontal-tab and vertical-tab modes it participates in layout, so the
terminal/editor starts after the sidebar instead of being covered by it. The
workspace owns this drag state because it has the full layout budget. A capture
overlay is rendered during drag so releasing the mouse outside the handle exits
resize mode reliably.

## Root Sync

Files and Search share the same root:

- terminal focus uses terminal cwd,
- editor focus uses active editor file parent,
- existing roots are preserved when they already contain the focused file,
- render fallback syncs the root if no folder has been set yet.

This keeps the sidebar useful when the user opens a tab that only contains an
editor pane or when focus moves between terminal and editor surfaces.
