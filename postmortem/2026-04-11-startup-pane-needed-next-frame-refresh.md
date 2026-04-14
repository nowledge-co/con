## What happened

The initial terminal pane could still stay blank at startup even after immediate layout-time refreshes were added. Any later UI interaction that triggered another frame made it appear.

## Root cause

The embedded Ghostty surface needed one refresh after GPUI completed the current frame. Refreshing during the same layout pass was still too early in the startup lifecycle.

## Fix applied

After `GhosttyView::on_layout()` applies the surface frame, Con now schedules a Ghostty refresh on GPUI's next frame whenever the surface is effectively visible.

## What we learned

For embedded native GPU surfaces, "correct layout" and "safe time to repaint" are not always the same moment. Startup needed a deferred repaint aligned to the framework's next-frame boundary.
