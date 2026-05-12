# Sidebar Vertical Tabs Regression

## What happened

PR #179 added the editor file/search sidebar, but the sidebar model replaced the
active vertical-tab surface with an always-visible activity rail. Users who rely
on vertical tabs lost the folded/unfolded tab workflow, and users who rely on a
clean terminal view could no longer hide all left-side chrome.

## Root cause

The editor sidebar was designed as a new top-level left rail instead of as a
section inside the existing sidebar system. The implementation reused the
sidebar width/session state for file/search panels and rendered the activity
rail independently, so vertical tabs were no longer part of the active layout.

## Fix applied

The follow-up restores the sidebar as one system:

- `ToggleLeftPanel` hides or unhides the whole sidebar.
- `SessionSidebar` renders the folded vertical-tab rail again and keeps the
  unfolded vertical-tab panel available.
- File/search tools render as compact sections beside vertical tabs, not as a
  replacement for them.
- Sidebar resize budgeting accounts for the vertical-tab surface so the terminal
  pane keeps usable width.

## What we learned

New navigation surfaces should compose with established user workflows before
they replace them. For terminal chrome, "collapse" and "fold" are separate
concepts: collapse/hide controls workspace cleanliness, while fold/unfold
controls tab density.
