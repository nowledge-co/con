# macOS Chrome Seam Light Leak

## What happened

Fast dragging the edge between the terminal and agent panel could expose a thin transparent gap. With a light macOS window backdrop and a dark terminal theme, that gap appeared as a white flash. The bottom input bar transition had the same class of issue.

## Root cause

The workspace already had seam covers for some chrome transitions, but they were translucent UI-colored surfaces and only covered agent-panel open/close animation. They did not cover active agent-panel resize dragging, and translucent covers can still reveal the macOS transparent window backdrop during compositor timing gaps.

## Fix applied

During macOS agent-panel drag/open-close and bottom input-bar transitions, con now paints a temporary seam matte using the active terminal background color at full opacity. The normal transparent/borderless design remains unchanged outside those moving seams. Non-macOS behavior keeps the previous translucent seam cover.

## What we learned

For embedded native terminal surfaces on macOS, moving seams need an opaque underlay that matches the adjacent terminal surface. A translucent chrome-colored cover can still leak the window backdrop and reads as flicker when the terminal theme is dark.
