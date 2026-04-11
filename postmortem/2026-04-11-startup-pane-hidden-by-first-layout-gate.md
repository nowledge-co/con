## What happened

After the recent Ghostty visibility gating change, restored panes could boot into a permanently blank state. The native surface existed, but it remained hidden until some later interaction forced another visibility update.

## Root cause

`GhosttyView::update_frame()` returned immediately when the incoming layout bounds matched `last_bounds`. That early return also skipped clearing `awaiting_first_layout_visibility`.

This mattered because control/bootstrap code can create a surface before GPUI's steady-state layout settles. If the first "real" layout reused the same bounds value that was already stored, the gate never dropped, so `set_visible(true)` kept resolving to `effective_visible = false`.

## Fix applied

`update_frame()` now treats "bounds changed" and "first layout gate clear" as separate concerns:

- frame/size updates still only happen when bounds actually change
- the first-layout visibility gate is always cleared and visibility replayed once the first real layout arrives

## What we learned

Visibility gates tied to layout need an explicit release path that does not depend on geometry deltas. "Same bounds" is still a meaningful layout confirmation.
