# Pane Divider Seam and Title Rendering Polish

## What happened

Dragging the pane divider between terminal and editor panes could reveal transparent window backing along the seam. Inactive pane and editor tab titles also looked rough at the glyph edges because muted text was rendered with extra opacity over translucent chrome.

## Root cause

Pane split resizing used the transparent drag capture layer directly over mixed native terminal and GPUI editor surfaces. Unlike the agent panel resize path, it did not draw an opaque themed overdraw strip while the divider was moving.

The first attempted fix drew a global mouse-following strip. That covered the leak but was not tied to the actual split divider geometry, so it could drift away from the rendered edge while resizing.

Pane divider drag also started from an absolute mouse coordinate but updated horizontal movement in content-local coordinates once a leading sidebar was present. That made the divider jump as soon as the first drag move arrived. The drag start was also deferred until the first mouse move, leaving a short window where fast cursor movement over native terminal surfaces could happen before the resize capture overlay existed.

Inactive pane chrome also combined a translucent title bar with low-opacity text. That made antialiasing blend against changing native or desktop backdrops rather than a stable themed surface.

## Fix applied

- Moved the opaque seam cover onto the active split divider itself, so the overdraw is anchored to the real rendered edge.
- Matched pane resize coordinate math to the actual open sidebar width when Files/Search is expanded.
- Started pane divider drag immediately on mouse-down and converted horizontal start positions to the same content-local coordinate system used by move updates.
- Made pane title chrome opaque and removed extra text alpha from inactive pane/editor tab labels.
- Removed the active-pane dot in favor of title contrast and stable centered title geometry.
- Kept inactive labels muted through the theme token while using medium weight for cleaner mono glyph rendering.

## What we learned

Any resizable seam between native terminal surfaces and GPUI surfaces needs an explicit opaque cover during motion, and that cover must be attached to the real divider rather than approximated from the cursor. Muted inactive chrome should preserve a stable backing layer; stacking translucency on the background and the text creates fragile font-edge rendering.
