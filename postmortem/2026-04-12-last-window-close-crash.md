## What happened

Closing the last tab in a window with `cmd-w` could crash Con during window teardown.

The failure was most visible on one-tab auxiliary windows created with `cmd-n`, but the same risk applied to any path that destroyed the last terminal-bearing window.

## Root cause

There were two lifecycle problems stacked together:

- Con had a custom "close last tab" path that did not match the normal window-close lifecycle.
- Embedded Ghostty surfaces were only being hidden or detached from AppKit, while the actual `ghostty_surface_free` still happened later during GPUI entity/window destruction.

That meant a Metal-backed embedded Ghostty surface could still be torn down while the GPUI window and native view hierarchy were already collapsing. This was the wrong ownership boundary.

An intermediate attempt also showed that manually calling native `NSWindow.close` from the tab action path was unsafe, because it re-entered GPUI's macOS close callback from inside active UI update work.

## Fix applied

The final fix was to shut down Ghostty surfaces explicitly before window removal:

- added `GhosttyView::shutdown_surface()`
- on pane close, tab close, and last-window close, Con now:
  - clears surface focus
  - marks the surface occluded
  - detaches the host `NSView`
  - drops the Ghostty surface immediately
  - clears local surface state
- `cmd-w` on the last tab now uses GPUI's `window.remove_window()` path after workspace cleanup, instead of manually sending AppKit `close`

This moved Ghostty surface destruction into Con's controlled close path, while the workspace and native host views were still valid.

## What we learned

- For embedded GPU surfaces, "hide the native view" is not the same as "tear down the surface".
- Surface destruction should happen at a stable application-owned boundary, not be left to generic window/entity drop timing.
- Last-tab close and native window close should share cleanup semantics, but they do not need to share the same low-level dispatch mechanism.
