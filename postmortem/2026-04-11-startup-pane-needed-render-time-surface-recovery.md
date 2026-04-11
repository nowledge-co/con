## What happened

Even after tightening the bootstrap path, restored startup panes could still open blank. The workspace chrome appeared, but the active terminal surface had not been materialized yet.

## Root cause

Startup still depended too heavily on the asynchronous bootstrap reassert loop to create a Ghostty surface after layout. In practice, the active pane could already have real bounds during render while still lacking a native surface, leaving the window visibly blank until some later interaction retriggered recovery.

## Fix applied

During normal workspace render, the active tab now checks each visible pane:

- if it already has real layout
- and it still does not have a Ghostty surface

then Con eagerly calls `ensure_surface(...)` before replaying visibility.

This keeps ordinary startup and restore on the normal visible-pane path instead of relying on a background bootstrap timer to rescue them later.

## What we learned

For visible panes, surface recovery belongs on the render path, not only on an async bootstrap path. If the pane is on screen and has layout, Con should converge it to a live terminal immediately.
