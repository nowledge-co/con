# SVG Icons Invisible Without Explicit text_color

**Date**: 2026-04-04

## What Happened

Tab bar buttons (new tab "+", agent panel toggle) were completely invisible — users had to blind-click where they expected the buttons to be. The buttons were correctly positioned with proper cursor and hover behavior, but the icons rendered as transparent.

## Root Cause

In GPUI, SVG icons using `stroke="currentColor"` (the Phosphor icon convention) require `.text_color()` to be set **directly on the `svg()` element**, not just on the parent container. Setting `.text_color()` on a parent `div()` does NOT propagate to child SVG stroke colors.

**Broken pattern:**
```rust
div()
    .text_color(theme.muted_foreground) // This does NOT propagate to SVG
    .child(svg().path("phosphor/plus.svg").size(px(14.0))) // No color → invisible
```

**Working pattern:**
```rust
div()
    .child(
        svg()
            .path("phosphor/plus.svg")
            .size(px(14.0))
            .text_color(theme.muted_foreground) // Must be on the svg() element
    )
```

## Fix Applied

Added `.text_color()` to all SVG elements that were missing it — 5 instances across workspace.rs and settings_panel.rs.

## What We Learned

**Rule**: Every `svg()` element in GPUI must have its own `.text_color()` call. Never rely on parent container color inheritance for SVG icons. This is different from HTML/CSS where SVG `currentColor` inherits from the parent's `color` property.
