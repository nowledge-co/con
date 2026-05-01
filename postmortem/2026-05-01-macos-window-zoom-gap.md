# macOS Window Zoom Gap

## What Happened

Maximizing or fullscreening Con on macOS could leave a visible strip at the
bottom of the screen. Double-clicking the in-app title/tab bar also did not
trigger the normal macOS titlebar zoom action.

## Root Cause

Con set `NSWindow.contentResizeIncrements` from the active terminal cell size.
That made manual window resizing snap to terminal-cell increments, but AppKit
also applies the same increments to window zoom/fullscreen sizing. If the screen
height is not an exact multiple of the terminal cell height plus chrome, AppKit
chooses the nearest smaller content size and leaves part of the visible frame
unused.

The double-click issue was separate: the in-app top bar was wired as a drag /
double-click titlebar area only on non-macOS. On macOS, we relied on the
transparent native titlebar to infer the behavior, but our GPUI-rendered tab bar
was the actual hit target.

## Fix Applied

- Removed the macOS window-level content resize increment sync. The window now
  fills the platform-provided zoom/fullscreen frame exactly, while Ghostty and
  the PTY resize path map that pixel size to terminal rows/columns.
- Marked the macOS top bar as a drag area and forwarded double-clicks to
  `window.titlebar_double_click()`, preserving the user's system preference for
  double-click titlebar behavior.

## What We Learned

Terminal cell snapping is not a safe window-level constraint. It affects more
than manual resize, including system zoom and fullscreen decisions. In an
embedded terminal app, the window should obey platform geometry exactly; terminal
grid quantization belongs inside the terminal surface and PTY resize pipeline.
