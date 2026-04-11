## What happened

Control-created panes could still appear oversized and offset across the workspace before they were laid out into their real split position.

The symptom showed up as a second terminal view visibly exceeding the intended pane bounds immediately after pane creation from `con-cli` or other control-plane workflows.

## Root cause

`GhosttyView::ensure_initialized_for_control(...)` correctly marked a control-created surface as `awaiting_first_layout_visibility` when it had no real pane bounds yet.

However, later calls to `GhosttyView::set_visible(true)` did not respect that guard. They directly unhid the underlying `NSView`, even though the surface was still waiting for its first real `on_layout` frame. That let the fallback native frame become visible before GPUI committed the pane’s actual geometry.

## Fix applied

`GhosttyView::set_visible(...)` now computes effective visibility as:

- requested visible
- and not awaiting first real layout

That keeps the native Ghostty view hidden until `update_frame(...)` clears the first-layout gate.

## What we learned

For embedded native surfaces, “desired visibility” and “safe to publish visibility” are different states.

Any code path that can unhide the surface must respect layout readiness, otherwise later focus/bootstrap helpers can accidentally bypass the initialization guard and expose placeholder geometry.
