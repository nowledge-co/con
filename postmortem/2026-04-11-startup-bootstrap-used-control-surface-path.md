## What happened

On app launch, the shell chrome rendered but the initial embedded terminal could stay blank until some later interaction. The startup path was accidentally treating ordinary panes like control-created panes.

## Root cause

`schedule_terminal_bootstrap_reassert()` always called `terminal.ensure_surface(...)`. That method uses Ghostty's control-surface initialization path, which is intentionally allowed to create a terminal before GPUI has delivered real pane layout.

That fallback path is correct for externally created panes, but wrong for normal startup panes. It can create a hidden surface from fallback bounds before the first real layout pass, which made startup visibility dependent on later layout/visibility replay.

## Fix applied

- Added a `has_layout()` check on `GhosttyView` / `TerminalPane`
- Changed bootstrap reassert to only call `ensure_surface(...)` after the pane has a real layout and still lacks a surface

This keeps the startup pane on the normal on-layout initialization path while preserving the control-plane bootstrap behavior for panes that were explicitly created through control code.

## What we learned

The control-plane surface path should stay isolated. Reusing it for generic startup "safety" made the terminal lifecycle harder to reason about and introduced a second initialization mode for ordinary panes.
