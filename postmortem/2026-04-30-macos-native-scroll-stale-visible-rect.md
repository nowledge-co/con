# macOS Native Scroll Stale Visible Rect

## What Happened

After split, nested split, zoom, or unzoom operations on macOS, a terminal pane could show a blank, gray, or transparent rectangle until the user manually resized pane dividers. The issue reproduced in `v0.1.0-beta.45`, before the pane-zoom PR, which ruled out pane zoom as the root cause.

## Root Cause

The April 20 macOS scroll work wrapped the embedded Ghostty `NSView` in an `NSScrollView` and positioned the surface from the scroll view's `documentVisibleRect`. That mirrors Ghostty's own structure at a high level, but Con does not own an AppKit `layout()` method like Ghostty's native `SurfaceScrollView`.

Con mutates the `NSScrollView`, document view, and surface view imperatively from GPUI layout callbacks. During split and zoom topology changes, AppKit can still return a stale `documentVisibleRect` immediately after those frame mutations. Con then positioned the Metal surface from the stale rect, leaving part of the pane covered only by matte or desktop background.

Manual divider resizing appeared to fix the issue because it forced another GPUI/AppKit frame pass with fresh geometry.

## Fix Applied

`GhosttyView::sync_native_scroll_view` no longer reads `documentVisibleRect` after mutating the scroll hierarchy. It computes the surface frame directly from Con-owned pane bounds and Ghostty scrollbar state: `(x = 0, y = scroll_y, width = visible_width, height = visible_height)`.

The PR also keeps native pane views behind a short layout reveal barrier during topology changes so stale or bootstrap frames are not exposed while GPUI settles the split tree.

## What We Learned

Mirroring an upstream AppKit view hierarchy is not enough if the host owns a different layout lifecycle. Ghostty can safely use `documentVisibleRect` inside its own `NSView.layout()` flow; Con cannot safely query that computed AppKit geometry in the same pass where GPUI just drove imperative frame changes.

For embedded native views under GPUI, deterministic geometry should come from Con's layout model and terminal state. AppKit geometry queries are acceptable as observations, not as source-of-truth immediately after host-driven mutations.
