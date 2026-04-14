## What happened

Overlay dismissal kept producing display races because Con's architecture assumed Ghostty had to be hidden whenever GPUI overlays appeared.

## Root cause

The Ghostty host view was attached into the same native view layer in a way that made it compete with GPUI overlays for z-order. That forced Con to hide/show the terminal around settings, palette, and skill popups.

## Fix applied

- attach the Ghostty host below GPUI's native view in the AppKit hierarchy
- stop using modal/popup visibility as a reason to hide Ghostty views

Tab switching still controls terminal visibility; overlays no longer do.

## What we learned

The correct long-term model is layering, not toggling. If overlays live above GPUI and Ghostty lives below GPUI, Con does not need a fragile hide/show lifecycle for modals.
