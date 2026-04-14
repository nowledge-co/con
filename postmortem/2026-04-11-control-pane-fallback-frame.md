# Control-Created Pane Fallback Frame

## What happened

When `con-cli` created or restored a pane before GPUI had laid it out, the embedded Ghostty `NSView` could appear at an obviously wrong size and position. In practice this looked like a large terminal surface floating across the workspace and overlapping other panes.

## Root cause

`GhosttyView::ensure_initialized_for_control` was designed to make a PTY/surface available for control-plane use before the normal render/layout pass had happened.

That path used a fallback window-sized bounds value and then immediately:

1. initialized the native Ghostty surface
2. applied the fallback frame
3. cleared the "awaiting first layout" visibility gate
4. made the native `NSView` visible

This meant the placeholder frame escaped into the real UI instead of staying hidden until GPUI supplied the pane's actual bounds.

## Fix applied

If a control-created pane has never received a real layout yet:

- initialize the Ghostty surface with fallback bounds for sizing only
- keep `awaiting_first_layout_visibility = true`
- do **not** publish the fallback frame via `update_frame`
- wait for the first real `on_layout` to set bounds and reveal the native view

If the pane already has real bounds, the control path still updates its frame immediately.

## What we learned

- Native-view initialization and native-view visibility must be treated as separate concerns.
- Fallback geometry is acceptable for bootstrapping a PTY, but not acceptable as visible UI state.
- Control-plane shortcuts are especially dangerous around first-layout timing because they bypass the normal user-driven render path.
