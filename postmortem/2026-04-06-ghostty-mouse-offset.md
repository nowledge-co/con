# Ghostty Mouse Selection Offset on Retina

**Date:** 2026-04-06

## What happened

Mouse selection in the ghostty terminal was offset by approximately 2 rows up and 10 characters right from the actual click position. The selection highlight appeared in the wrong location, making text selection unusable.

## Root cause

`view_local_px` in `ghostty_view.rs` converted GPUI window coordinates to view-local coordinates and then **multiplied by `scale_factor`** (2.0 on Retina), sending physical pixel coordinates to ghostty.

However, ghostty's `ghostty_surface_mouse_pos` C API expects **logical pixels** (points), not physical pixels. Ghostty scales internally via its own `content_scale` mechanism — the embedded apprt's `cursorPosToPixels()` multiplies by scale before passing to the core Surface.

The result: on a 2x Retina display, coordinates were doubled, causing the cursor to be computed at 2x the actual distance from the origin. The apparent offset was proportional to click distance from the top-left corner.

**Evidence chain:**
- `3pp/ghostty/src/apprt/embedded.zig` line ~855: `cursorPosCallback` calls `cursorPosToPixels()` which multiplies by content scale
- `3pp/ghostty/macos/Sources/Ghostty/SurfaceView_AppKit.swift` line ~1005: native macOS path sends logical points (not backing pixels)
- `3pp/ghostty/src/renderer/size.zig` line ~67: surface coordinate system documented as pixel units with (0,0) at top-left

The scroll wheel handler already had the correct pattern — it **divided** physical pixel deltas by scale to get logical coordinates.

## Fix applied

Removed the `* scale` multiplication from `view_local_px` (renamed to `view_local_pos`). GPUI's `Point<Pixels>` is already in logical pixels, and subtracting `bounds.origin` gives logical offset from the element origin — exactly what ghostty expects.

```rust
// Before (wrong — sending physical pixels)
fn view_local_px(&self, pos: Point<Pixels>) -> (f64, f64) {
    let scale = self.scale_factor as f64;
    (f64::from(pos.x - bounds.origin.x) * scale,
     f64::from(pos.y - bounds.origin.y) * scale)
}

// After (correct — sending logical pixels)
fn view_local_pos(&self, pos: Point<Pixels>) -> (f64, f64) {
    (f64::from(pos.x - bounds.origin.x),
     f64::from(pos.y - bounds.origin.y))
}
```

## What we learned

- When bridging coordinate systems between frameworks, always verify the **unit contract** of the receiving API by reading its source — don't assume physical vs logical.
- Ghostty's embedded platform API expects logical pixels for mouse input (it scales internally), matching macOS convention where `locationInWindow` returns points not pixels.
- The function name `view_local_px` was misleading — "px" is ambiguous between logical and physical. Renamed to `view_local_pos` to avoid future confusion.
- The scroll wheel handler was a Rosetta Stone: it already divided by scale, proving ghostty wants logical units. Cross-reference existing working code paths before writing new coordinate conversions.
