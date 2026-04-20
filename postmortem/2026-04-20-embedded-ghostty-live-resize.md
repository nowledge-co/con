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
- The remaining per-surface 16ms GPUI polling loops were removed. Surface event draining and deferred resize/retry housekeeping now run from one workspace-level Ghostty wake pump instead of N independent pane loops.
- Con now also mirrors Ghostty's macOS cell-step window resize behavior by setting `contentResizeIncrements` from the active terminal surface's cell size. That reduces the number of meaningless sub-cell resize states the host and renderer ever see.

## What we learned

- Embedded Ghostty should be integrated on Ghostty's own terms. Polling it from the host app is not equivalent to honoring its wakeup contract.
- Live resize performance on macOS is as much an AppKit view-hosting problem as a renderer problem.
- "Feels slower than Ghostty" is often an integration smell, not a sign that Ghostty itself is slow.
- Even after the core renderer is wakeup-driven, duplicated host-side observer loops can still make the product feel slow. The render path and the host-side bookkeeping path both have to be reduced to one coherent ownership boundary.
- Matching Ghostty also means matching its resize semantics, not just its tick semantics. If Con allows far more intermediate window states than Ghostty does, it can still feel slower even when the renderer itself is healthy.
