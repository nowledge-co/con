# Postmortem: Control-Created Panes Returned Before Ghostty Existed

## What happened

`panes.create` could return a stable `pane_id`, but the new pane was still only a GPUI leaf placeholder. In practice that meant:

- `surface_ready` was effectively false, although the API could not say so yet
- `is_alive` looked false because there was no `GhosttyTerminal` attached at all
- startup commands like `printf ...` or `cd ~/dev/temp && codex` were written into a queue before the surface ever became a real control target
- benchmark and `con-cli` flows saw blank or misleading panes

## Root cause

Ghostty surface creation was coupled to the first real layout pass inside `GhosttyView::on_layout()`. That is acceptable for mouse-driven UI, but it is not acceptable for a control plane.

The control path was doing this:

1. create a `GhosttyView`
2. insert it into the pane tree
3. immediately return `PaneCreated`

That returned layout identity, not runtime readiness.

## Fix applied

- Added eager Ghostty surface initialization for control-created panes.
- Added persistent native-view visibility state so visibility changes still apply before the NSView exists.
- Added a hidden provisional bootstrap geometry for surfaces created before a real layout pass.
- Added `surface_ready`, `is_alive`, and `has_shell_integration` to `PaneCreated`.
- Added `surface_ready` to pane metadata and benchmark checks.
- Updated `create_pane` tool waiting logic to poll the stable `pane_id` and distinguish:
  - live pane with immediate output
  - live pane without immediate output
  - pane that still is not ready

## What we learned

- A pane tree identity is not the same thing as a terminal runtime.
- UI-driven initialization and control-plane initialization must not share the same readiness assumptions.
- Benchmarking the real socket surface caught this quickly; compile-only confidence would not have.
