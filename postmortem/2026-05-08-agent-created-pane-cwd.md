# Agent-created panes lost working directory

## What happened

Panes created through the control plane or the built-in agent opened from the
default shell directory instead of inheriting the focused pane's working
directory.

## Root cause

The immediate split actions already passed the focused terminal cwd into
`create_terminal`, but `panes.create` deferred creation until a `Window` was
available. That deferred request only kept the command, tab, and split
direction, so the eventual window-aware flush always called
`create_terminal(None, ...)`.

## Fix applied

The create-pane request now captures the focused pane cwd before deferring and
passes it through the window-aware flush. The flush also fails stale tab
requests cleanly and saves the session after successful agent-created panes.

## What we learned

Window-aware deferred work must carry the same semantic inputs as the direct UI
path. If a future pane creation path needs a `Window`, capture cwd, target, and
user intent before queueing instead of recomputing from defaults later.
