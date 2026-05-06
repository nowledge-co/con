# macOS Display Scale Sync

## What Happened

Moving an existing Con window from a built-in Retina display to a 1920x1080
external display could leave existing terminal panes with far fewer rows and
columns than a newly-created pane in the same window.

The failure only affected already-created Ghostty surfaces. Creating a new tab
or pane after the move used the correct grid size.

An initial fix synchronized Ghostty's content scale and display id from GPUI's
layout path. That corrected PTY rows and columns after the move, but could leave
the rendered font tiny because the native layer presentation scale and Ghostty's
font/cell DPI scale were no longer updated as one AppKit backing transaction.

## Root Cause

The macOS host resized the embedded Ghostty surface from the current AppKit
backing size, but the already-created surface kept the content scale that was
captured when it was first created. A surface created on a 2x Retina display
could therefore be resized to a 1x external display backing size while Ghostty
still used 2x content-scale semantics for cell metrics.

Con also exposed Ghostty's `ghostty_surface_set_display_id` binding in FFI but
did not drive it from the AppKit screen that currently owns the window.

The missing piece was AppKit's backing-property lifecycle. Ghostty's own macOS
view updates the surface layer's `contentsScale`, Ghostty content scale, and
framebuffer size together from `convertToBacking(...)`. Updating only
`ghostty_surface_set_content_scale` changed Ghostty's font DPI without also
keeping Core Animation's layer scale and the surface pixel size in the same
coordinate system.

## Fix Applied

- Added a native AppKit trampoline for embedded Ghostty surfaces.
- The trampoline synchronizes `layer.contentsScale`,
  `ghostty_surface_set_display_id`, `ghostty_surface_set_content_scale`, and
  `ghostty_surface_set_size` in the same order as Ghostty's upstream AppKit
  view.
- Registered native window screen/backing-property observers for each embedded
  Ghostty `NSView`, so cross-display moves trigger a backing sync even when the
  GPUI pane bounds do not change.
- Changed Con's macOS layout path to use the same backing sync helper instead of
  directly setting content scale from GPUI layout.

## What We Learned

For embedded native terminal surfaces, display identity and content scale are
part of geometry state. Bounds alone are not enough: a pane can have the same
logical size after a cross-display move while its backing pixel scale and
display id have changed underneath it.

For GPU-backed AppKit content, content scale is not just metadata. In Ghostty it
feeds font DPI and cell metrics, while Core Animation also needs the hosted
layer's `contentsScale` to avoid compositor scaling. These values must be kept
in sync with the framebuffer size as one geometry update.
