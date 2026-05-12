# Left Sidebar

Status: Implemented

The left sidebar is the workspace's navigation and project panel area. It is not
the workspace tab identity by itself; vertical tabs remain the primary tab
rail/panel inside the sidebar.

## Layout

```text
[vertical tabs] [Files/Search sections] [pane tree] [agent panel]
```

When the left sidebar is hidden, none of the three sidebar columns render. This
preserves the clean terminal view users expect from the sidebar toggle.

When the left sidebar is visible:

- vertical tabs can stay folded as a 44 px rail or unfold into the pinned tab
  panel,
- the file/search section header switches editor tools,
- the file/search section uses the same resizable content width.

## Slots

`ActivitySlot::Files` renders `FileTreeView`.

`ActivitySlot::Search` renders `SidebarSearchView`.

## Toggle Rules

- `Cmd+B` dispatches `ToggleLeftPanel`.
- The top bar sidebar button dispatches the same behavior.
- The toggle hides or unhides the whole sidebar, including vertical tabs,
  section icons, and file/search content.
- The vertical tab collapse/expand button only changes folded/unfolded tab
  mode while the sidebar is visible.
- Clicking an inactive section icon switches slot and opens the panel.
- Clicking the active section icon toggles the sidebar.

## Width Rules

The panel can resize across the available window width after accounting for the
vertical tabs and agent panel. When vertical tabs are unfolded, resize uses a
split budget so the tab panel and file/search section cannot crowd out the
terminal pane. The workspace owns this drag state because it has the full layout
budget. A capture overlay is rendered during drag so releasing the mouse outside
the handle exits resize mode reliably.

## Root Sync

Files and Search share the same root:

- terminal focus uses terminal cwd,
- editor focus uses active editor file parent,
- existing roots are preserved when they already contain the focused file,
- render fallback syncs the root if no folder has been set yet.

This keeps the sidebar useful when the user opens a tab that only contains an
editor pane or when focus moves between terminal and editor surfaces.
