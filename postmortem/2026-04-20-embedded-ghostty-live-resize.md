## What happened

Resizing a Con window that contained a heavy TUI such as Claude Code still felt dramatically slower than native Ghostty, even after earlier fixes removed obvious workspace-level stalls.

The lag reproduced in the simplest case:

- one pane
- one active terminal surface
- no split management involved

That ruled out pane-count scaling as the primary cause.

## Root cause

Two integration mistakes were left in the embedded Ghostty path:

1. Con was still driving `ghostty_app_tick()` from its own workspace timer instead of honoring Ghostty's embedded runtime wakeup model.
2. The embedded host `NSView` stack did not use the same live-resize-friendly AppKit policies that GPUI applies to its own native view host:
   - autoresizing masks for width and height
   - `NSViewLayerContentsRedrawDuringViewResize`

In practice this meant Con was doing extra scheduling work while AppKit was also resizing a child Metal-backed view without the native redraw hints Ghostty/GPUI expect.

## Fix applied

- `con-ghostty` now stores a small wake handle in app userdata and uses Ghostty's `wakeup_cb` to dispatch `ghostty_app_tick()` onto the macOS main queue, with atomic deduping so repeated wakeups coalesce cleanly.
- Con's old workspace-level 8ms Ghostty tick loop was removed.
- `GhosttyView` now gives the embedded host and child surface views native width/height autoresizing masks and `NSViewLayerContentsRedrawDuringViewResize`.

## What we learned

- Embedded Ghostty should be integrated on Ghostty's own terms. Polling it from the host app is not equivalent to honoring its wakeup contract.
- Live resize performance on macOS is as much an AppKit view-hosting problem as a renderer problem.
- "Feels slower than Ghostty" is often an integration smell, not a sign that Ghostty itself is slow.
