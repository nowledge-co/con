# Terminal input caused workspace rerenders

## What happened

Terminal typing could occasionally feel unresponsive even when the shell itself
was not blocked. The issue was most visible in terminal-focused workflows with
the surrounding workspace chrome enabled, and when returning to Con from another
application.

## Root cause

The macOS terminal key handler emitted `GhosttyFocusChanged` after every handled
key. The workspace treats that event as a pane focus change, so normal typing
could trigger a full workspace notification path: pane focus sync, terminal
focus-state sync, pane metadata rebuild, and input-bar/sidebar/agent-panel state
sync.

The input bar also notified on every pane metadata sync even when the pane list
and focused pane were unchanged.

A second issue was subtler: render-time skill discovery still touched the
filesystem. Even with caching, cwd changes could synchronously stat or rescan
skill roots from the workspace render path. That made terminal-adjacent chrome
capable of blocking the same UI thread that needs to accept keystrokes.

A separate focus-loss path could make the symptom look identical to input
latency: after switching away from Con and back, the OS window could be active
while no terminal/editor/input focus target had been reasserted. Typing then
appeared to do nothing until the user clicked the terminal.

## Fix applied

- Stopped terminal keypresses from emitting `GhosttyFocusChanged`; mouse/drop
  focus events still emit it.
- Made input-bar pane and cwd sync idempotent so unchanged render-time metadata
  does not notify the input bar again.
- Moved skill discovery to a background request/result path. Rendering now only
  schedules a deduped scan; the filesystem work runs off the UI thread.
- Kept skill-scan deduplication at the request level, but rebuilt the registry
  in the background whenever candidate roots change. This preserves the old
  behavior where skill edits are picked up after cwd/config changes without
  putting the rebuild back on the render path.
- Reassert the logical keyboard focus target when a Con window becomes active.
  Existing input/editor focus is preserved when GPUI still knows it; otherwise
  focus falls back to the active pane so an active terminal can accept input
  immediately after app switching.

## What we learned

Terminal input should invalidate only the terminal view unless the interaction
actually changes workspace focus or chrome state. Render-time state sync must be
idempotent, especially for terminal-adjacent UI, because a small notification in
the hot path can fan out into expensive parent/child rerenders.

The render path should also be treated as latency-sensitive infrastructure.
Filesystem discovery, parsing, and other potentially unbounded work must run
through an async or background handoff, even when a cache makes the common case
look cheap.

Responsiveness bugs are not always compute bugs. The app must also maintain a
clear keyboard-focus owner after OS-level activation changes; otherwise a
healthy terminal can look frozen because key events have nowhere useful to go.
