## What happened

Dragging a pane divider or the agent-panel divider while the pane-scope overlay was visible could crash Con inside Ghostty with a `selectionScrollTick` panic.

## Root cause

Ghostty still had an active mouse-selection scroll state when Con began resizing the embedded terminal surfaces. The resize path changed pane geometry without first releasing the terminal's left-mouse selection gesture, so Ghostty's selection-scroll timer continued against a surface whose layout was being mutated.

## Fix applied

- Added an explicit `release_mouse_selection()` path on `TerminalPane`.
- Called it before pane split drags start and before agent-panel resize drags start.
- Kept the scope overlay live-updating during resize, but stopped the terminal from carrying an in-progress selection gesture into the resize lifecycle.

## What we learned

- Embedded Ghostty surfaces need input-state cleanup before layout mutation, not just before teardown.
- Divider drags are effectively a native interaction boundary; they should release terminal mouse gestures before resizing.
