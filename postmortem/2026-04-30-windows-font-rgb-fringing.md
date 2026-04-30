# Windows Font RGB Fringing (2026-04-30)

## What happened

After the Windows font collection and CJK fallback fixes, issue #78
still had one visible quality problem: terminal text looked slightly
thin, and zoomed screenshots showed red/green/blue edges around glyphs,
especially around CJK strokes.

The reporter's description was precise: this was not missing glyphs or
cell metrics anymore. It was ClearType-style subpixel color fringing.

## Root Cause

The Windows renderer rasterized terminal glyphs into an offscreen
`BGRA8` atlas with Direct2D ClearType enabled, then sampled the atlas in
our D3D11 shader as per-channel RGB coverage.

ClearType is designed for direct presentation to a known physical LCD
subpixel layout over an opaque background. Con's Windows path is
different:

- glyphs are first drawn into an offscreen atlas,
- the terminal frame is copied and handed to GPUI,
- the window can be transparent,
- screenshots and browser previews scale the image again,
- users may review the result on a different device with a different
  subpixel layout.

In that pipeline, RGB subpixel coverage becomes colored fringe instead
of perceived sharpness.

## Fix Applied

`crates/con-ghostty/src/windows/render/atlas.rs` now configures
DirectWrite/Direct2D for grayscale text:

- `D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE`,
- `DWRITE_PIXEL_GEOMETRY_FLAT`,
- `DWRITE_RENDERING_MODE_NATURAL_SYMMETRIC`,
- ClearType level `0.0`,
- slightly higher enhanced contrast to offset the perceived weight loss
  from dropping subpixel AA.

`crates/con-ghostty/src/windows/render/shaders.hlsl` now collapses the
sampled atlas RGB to one scalar glyph coverage before compositing. That
keeps the final frame neutral even if a driver or remote-display path
falls back to per-channel coverage internally.

## What We Learned

For Con's Windows backend, terminal glyphs should optimize for stable
offscreen composition, screenshots, transparency, and cross-device
review before LCD-subpixel sharpness. Subpixel AA only belongs in a
direct-present text renderer with a known final display target.

