## What happened

The first terminal pane could still appear blank on startup even after the native frame placement was corrected. Toggling another UI surface, such as the agent panel, made it appear immediately.

## Root cause

The embedded Ghostty surface could reach its correct visible frame during the normal layout pass without receiving a corresponding repaint at that moment. Later UI toggles called `set_visible(true)`, which did trigger `terminal.refresh()`, so the terminal suddenly appeared.

## Fix applied

After `GhosttyView::update_frame()` reapplies the native `NSView` frame, Con now explicitly refreshes Ghostty whenever the surface is effectively visible.

## What we learned

For embedded native GPU surfaces, correct geometry and correct repaint timing are separate requirements. Fixing the frame without forcing a repaint still leaves startup vulnerable to a blank first frame.
