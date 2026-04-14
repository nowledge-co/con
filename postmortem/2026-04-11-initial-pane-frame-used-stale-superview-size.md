## What happened

On startup, the first terminal pane could stay visually blank until a later layout change such as splitting the pane.

## Root cause

`GhosttyView::update_frame()` only repositioned the native Ghostty `NSView` when the pane bounds changed.

That assumption was wrong on macOS. The pane bounds could stay the same while the parent GPUI/NSView hierarchy was still settling, which meant the first frame calculation could use a stale superview height and place the embedded terminal incorrectly. Later actions that triggered a new layout with different bounds made the terminal appear, which is why splitting seemed to "fix" it.

## Fix applied

`update_frame()` now always reapplies the native `NSView` frame on every layout pass. Bounds changes still control terminal pixel-size updates, but native view positioning is no longer skipped just because the pane bounds are unchanged.

## What we learned

For embedded native views, frame placement depends on both local pane bounds and parent-view geometry. Caching only the child bounds was not enough.
