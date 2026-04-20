## What happened

Resizing a Con window that contained a heavy TUI such as Claude Code still felt dramatically slower than native Ghostty, even after earlier fixes removed obvious workspace-level stalls.

The lag reproduced in the simplest case:

- one pane
- one active terminal surface
- no split management involved

That ruled out pane-count scaling as the primary cause.

## Root cause

Three integration mistakes were left in the embedded Ghostty path:

1. Con was still driving `ghostty_app_tick()` from its own workspace timer instead of honoring Ghostty's embedded runtime wakeup model.
2. The embedded host `NSView` stack did not use the same live-resize-friendly AppKit policies that GPUI applies to its own native view host:
   - autoresizing masks for width and height
   - `NSViewLayerContentsRedrawDuringViewResize`
3. Con hosted each Ghostty surface in a bare `NSView` and ignored `GHOSTTY_ACTION_SCROLLBAR`. Standalone Ghostty does not do that on macOS; it wraps the surface in a native scroll container and keeps the visible viewport synchronized with Ghostty's scrollbar state.

That third mistake was the one that matched the user-visible symptom most closely: during heavy TUI resize, Con could briefly show older Y-axis scrollback content and only later settle back to the bottom, while standalone Ghostty stayed bottom-anchored throughout the drag.

## Fix applied

- `con-ghostty` now stores a small wake handle in app userdata and uses Ghostty's `wakeup_cb` to dispatch `ghostty_app_tick()` onto the macOS main queue, with atomic deduping so repeated wakeups coalesce cleanly.
- Con's old workspace-level 8ms Ghostty tick loop was removed.
- `GhosttyView` now gives the embedded host and child surface views native width/height autoresizing masks and `NSViewLayerContentsRedrawDuringViewResize`.
- The remaining per-surface 16ms GPUI polling loops were removed. Surface event draining and init-retry housekeeping now run from one workspace-level Ghostty wake pump instead of N independent pane loops.
- Con now also mirrors Ghostty's macOS cell-step window resize behavior by setting `contentResizeIncrements` from the active terminal surface's cell size. That reduces the number of meaningless sub-cell resize states the host and renderer ever see.
- The workspace render path stopped performing fresh terminal observations for pane-metadata UI. It now reuses cached runtime state so normal UI renders do not pull terminal text while a heavy TUI is active.
- `con-ghostty` now exposes Ghostty scrollbar actions through `TerminalState`, and `GhosttyView` hosts the embedded surface inside a native scroll container with a document view sized from Ghostty's scrollbar model. Con now keeps the viewport position synchronized with Ghostty during resize instead of treating the surface as a plain fixed-origin child view.
- Con's embedded scroll host no longer fabricates a fallback `offset = 0` scrollbar model when Ghostty has not emitted real viewport state yet. The native scroll container now stays inert until Ghostty provides real scrollbar data, matching Ghostty's own macOS host behavior and avoiding forced top-of-history paints during startup and reflow.
- Con's embedded macOS host no longer defers Ghostty surface size commits behind a custom coalescing policy. It now updates the core surface immediately on layout using AppKit backing-size conversion, which matches Ghostty's own macOS size propagation more closely.
- Con now also derives embedded surface size from the native scroll container's actual content area and visible rect instead of from raw GPUI pane bounds. That keeps viewport geometry and PTY resize math aligned while the window is actively resizing.

## What we learned

- Embedded Ghostty should be integrated on Ghostty's own terms. Polling it from the host app is not equivalent to honoring its wakeup contract.
- Live resize performance on macOS is as much an AppKit view-hosting problem as a renderer problem.
- "Feels slower than Ghostty" is often an integration smell, not a sign that Ghostty itself is slow.
- Even after the core renderer is wakeup-driven, duplicated host-side observer loops can still make the product feel slow. The render path and the host-side bookkeeping path both have to be reduced to one coherent ownership boundary.
- Matching Ghostty also means matching its resize semantics, not just its tick semantics. If Con allows far more intermediate window states than Ghostty does, it can still feel slower even when the renderer itself is healthy.
- Performance regressions also hide in “small” UI metadata paths. If the render path reads terminal text for sidebar/input-bar decoration, a dense TUI can make the whole app feel heavier even when the terminal surface itself is correct.
- On macOS, Ghostty's scrollbar updates are not just UI chrome for a visible scrollbar. They are part of the viewport-hosting contract. Ignoring them leaves the embedder with the wrong mental model of what the surface view represents.
- The host must also avoid guessing viewport state before Ghostty publishes it. A "helpful" fallback scrollbar model can be worse than no model at all because it actively drives the embed into the wrong part of scrollback.
- When matching a native host, "same rectangle on paper" is not enough. During live resize, the source of truth for terminal size has to be the scroll container that actually owns the visible viewport, not an outer layout rect that may be ahead of or behind AppKit's inner content geometry.
