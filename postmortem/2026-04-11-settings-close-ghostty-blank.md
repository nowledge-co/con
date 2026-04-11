## What happened

After opening Settings and closing it again, the embedded Ghostty terminal sometimes came back blank until the user typed or clicked inside the pane.

## Root cause

Con hides native Ghostty `NSView`s while modal overlays are visible so GPUI overlays can render above them. When the modal closed, the code only unhid the native view. It did not also clear Ghostty's occlusion state or request an immediate redraw.

That left Ghostty visible again at the AppKit layer, but still effectively waiting for the next input-driven render.

## Fix applied

Updated `GhosttyView::set_visible(...)` to:

- propagate occlusion state to Ghostty whenever native visibility changes
- trigger `refresh()` immediately when a surface becomes visible again

This makes modal close restore both the native view and Ghostty's render state in one place.

## What we learned

- Native visibility and terminal render state are separate concerns.
- For embedded Ghostty surfaces, "show the NSView" is not sufficient; unocclude and refresh need to happen at the same time.
- The visibility boundary belongs in `GhosttyView`, not in each modal caller.
