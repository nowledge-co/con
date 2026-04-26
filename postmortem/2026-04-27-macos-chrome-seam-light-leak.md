# macOS Chrome Seam Light Leak

## What happened

Fast dragging the edge between the terminal and agent panel could expose a thin transparent gap. With a light macOS window backdrop and a dark terminal theme, that gap appeared as a white flash. Bottom input-bar and vertical-tab sidebar transitions had the same class of issue.

## Root cause

Terminal pane dividers did not show the same failure mode because they sit inside the pane tree between adjacent terminal surfaces. The problematic seams are chrome-to-terminal seams: GPUI chrome moves while the embedded Ghostty `NSView` is composed by AppKit underneath it. A one-frame or subpixel mismatch can expose the transparent macOS window backdrop before GPUI chrome or the native terminal catches up.

The first attempted fix painted GPUI seam mattes during specific transitions. That was not sufficient because the exposed gap is below the GPUI layer in the native AppKit composition path, and it also missed vertical-tab sidebar transitions.

## Fix applied

Each macOS Ghostty wrapper now owns four native sibling `NSView` seam guards around the embedded terminal view. They are positioned below GPUI chrome and colored from the active terminal background plus terminal opacity, so fast-moving agent-panel, input-bar, and vertical-tab seams have a stable native underlay. The guards are hidden in the steady state and only expand while chrome is moving, avoiding permanent hard borders. The terminal view's logical size, grid size, transparency settings, blur settings, and all non-macOS paths remain unchanged.

## What we learned

For embedded native terminal surfaces on macOS, moving seams need to be fixed in the same native compositing layer as the embedded view. GPUI-only overlays are useful for normal chrome polish, but they are not a reliable mask for AppKit child-view timing gaps.
