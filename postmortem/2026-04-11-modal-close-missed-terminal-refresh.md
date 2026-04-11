## What happened

After closing Settings or another overlay that temporarily hid Ghostty surfaces, the terminal could stay visually blank until the user moved the mouse or generated another terminal event.

## Root cause

Con hid the native Ghostty `NSView`s while overlays were open and showed them again afterwards, but the re-show path did not force Ghostty to repaint once the overlay was gone. A later mouse move or terminal event indirectly triggered that repaint, making the terminal appear.

## Fix applied

When overlay hiding ends and Con restores the active tab's Ghostty views, it now schedules a one-frame-later refresh for those terminal surfaces.

## What we learned

Revealing an embedded terminal surface is not enough by itself. Overlay dismissal needs an explicit repaint handoff back to Ghostty.
