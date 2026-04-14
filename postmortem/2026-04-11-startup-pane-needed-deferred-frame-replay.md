## What happened

The first terminal pane still opened blank at startup even after immediate frame updates and immediate refreshes were added. Any later UI mutation that caused another frame pass made it appear.

## Root cause

Startup needed not just a deferred repaint, but a deferred replay of the native `NSView` frame itself. The first layout pass could still compute against an AppKit parent view that had not fully settled yet.

## Fix applied

`GhosttyView` now:

- factors native frame placement into a reusable helper
- reapplies that native frame on the next GPUI frame after layout
- then refreshes Ghostty on that same deferred pass

## What we learned

For an embedded AppKit surface, the right startup sequence can be: layout now, then frame replay + refresh on the next frame after the host view hierarchy stabilizes.
