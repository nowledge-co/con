# New panes and tabs lost working directory

## What happened

Panes created through the control plane or the built-in agent opened from the
default shell directory instead of inheriting the focused pane's working
directory. New tabs had the same user-visible problem after restoring a
session: the restored active pane knew its cwd, but the new-tab path ignored it
and launched from the shell default.

## Root cause

The immediate split actions already passed the focused terminal cwd into
`create_terminal`, but `panes.create` deferred creation until a `Window` was
available. That deferred request only kept the command, tab, and split
direction, so the eventual window-aware flush always called
`create_terminal(None, ...)`. Separately, `new_tab` always passed `None`
instead of using the active terminal cwd, so restored workspaces could still
produce home-directory tabs.

## Fix applied

The create-pane request now captures the focused pane cwd before deferring and
passes it through the window-aware flush. New tabs inherit the active terminal
cwd as well. The flush also fails stale tab requests cleanly and saves the
session after successful agent-created panes.

## What we learned

Window-aware deferred work and top-level creation actions must carry the same
semantic inputs as the direct split path. If a future pane or tab creation path
needs a `Window`, capture cwd, target, and user intent before queueing instead
of recomputing from defaults later.
