# TUI text cut off in Linux split panes

**Date**: 2026-05-05
**Issue**: #142

## What happened

On Linux, TUI applications (htop, vim, less, etc.) displayed text that was cut off at the right and bottom edges of the pane. The same TUI in a full-window terminal or in Ghostty's own split rendered correctly.

## Root cause

`estimate_surface_size()` in `linux_view.rs` computed the PTY grid dimensions (columns × rows) from the full pane `bounds` in pixels. However, `render()` wraps the terminal content in a div with `.px(TERMINAL_PADDING_X_PX)` and `.py(TERMINAL_PADDING_Y_PX)` — 12 px horizontal and 10 px vertical padding on each side.

This meant the PTY was told it had, for example, 220 columns when the drawable area only fit 196. TUI apps that fill the full reported grid width drew into the rightmost ~24 columns, which were outside the padded content area and got clipped by `overflow_hidden()`.

The same mismatch applied vertically: the bottom few rows were clipped.

## Fix applied

In `estimate_surface_size()`, subtract `TERMINAL_PADDING_X_PX * 2` and `TERMINAL_PADDING_Y_PX * 2` (scaled to physical pixels) from the bounds before dividing by cell dimensions. The PTY grid is now sized to the inner drawable area, matching what `render()` actually paints into.

```rust
let pad_x_px = (TERMINAL_PADDING_X_PX * scale_factor).round() as u32;
let pad_y_px = (TERMINAL_PADDING_Y_PX * scale_factor).round() as u32;
let inner_width_px = width_px.saturating_sub(pad_x_px * 2).max(1);
let inner_height_px = height_px.saturating_sub(pad_y_px * 2).max(1);
// columns/rows computed from inner_width_px / inner_height_px
```

## What we learned

The PTY grid size estimate and the render layout must agree on the drawable area. Any padding, margin, or chrome (tab strip, scrollbar) that reduces the visible terminal area must be subtracted from the bounds before computing columns/rows — otherwise TUI apps that query `TIOCGWINSZ` will draw into columns/rows that are never rendered.

When the real glyph-atlas grid renderer lands, it should derive its cell grid from the same inner bounds, not the outer pane bounds.
