## What happened

Closing Settings or other overlays could leave the embedded terminal visually blank until another mouse or terminal event occurred.

## Root cause

Con was hiding and showing the Ghostty surface `NSView` itself. On macOS, that is a poor lifecycle boundary for a Metal-backed embedded surface and can lead to display/z-order races when overlays come and go.

## Fix applied

Ghostty now lives inside a dedicated host container `NSView`:

- the host view is attached to the GPUI/AppKit window hierarchy
- the Ghostty surface view is attached inside that host
- Con hides and shows the host container, not the Ghostty surface directly

Frame updates now move the host in window coordinates and size the Ghostty surface inside the host at local `(0, 0)`.

## What we learned

For embedded native GPU surfaces, visibility control should happen at a stable container boundary. Hiding the render surface directly was the wrong abstraction.
