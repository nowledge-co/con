# 2026-05-07 — Windows CJK baseline rendering

## What Happened

Windows users reported two related rendering problems:

- CJK characters in the same terminal row could sit at slightly different
  vertical positions (#124).
- After the earlier ClearType RGB-fringing fix, CJK glyphs no longer showed
  colored edges but still felt too thin (#78).

The reports were hard to verify locally because no Windows test machine was
available in this pass, so the fix had to be grounded in the renderer code and
validated through CI plus a Windows dev build.

## Root Cause

The Windows terminal renderer rasterizes one terminal glyph at a time into a
DirectWrite/Direct2D glyph atlas. For glyphs missing from the primary terminal
font, DirectWrite selects fallback fonts during `DrawText`.

That is safe for coverage, but the old code top-aligned each one-character
fallback layout rectangle to the atlas slot. CJK fallback fonts do not all share
the primary terminal face's baseline metrics, and a one-character `DrawText`
call does not have surrounding run context to normalize the line baseline. The
result was visible per-glyph vertical drift.

The thin-stroke report came from the same rendering pipeline after we disabled
ClearType subpixel color coverage. Grayscale text avoids RGB fringe in our
offscreen BGRA atlas, but CJK fallback glyphs lost more perceived weight than
Latin text. A single global contrast value was too blunt: pushing it high enough
for CJK would also over-darken Latin and Nerd-Font prompt glyphs.
The right typography fix is to ask DirectWrite for a more appropriate East
Asian text weight first, then apply a moderate raster contrast adjustment only
to CJK text. Contrast alone is a last-mile rasterization tweak, not a substitute
for glyph weight.

## Fix Applied

- CJK/wide glyph rasterization now creates a tiny DirectWrite text layout for
  the glyph, reads the layout line baseline, and shifts the atlas layout rect so
  the fallback glyph baseline matches the primary terminal cell baseline.
- The atlas keeps grayscale antialiasing. Latin and Nerd-Font glyphs use the
  conservative contrast path; normal-weight CJK glyphs use a medium-weight
  DirectWrite text format and temporarily switch to a moderate CJK-only
  enhanced-contrast profile during rasterization, then the render target is
  restored before the next glyph.
- The Windows terminal view now measures and renders an inset content surface
  instead of painting cells flush against the pane edge. Hit testing, link hover,
  IME cursor bounds, scrollbar placement, and renderer dimensions all use the
  same inset content bounds. The inset gutter is painted with the terminal
  renderer's clear color and opacity so it stays visually attached to the
  terminal instead of showing transparent window content.

## What We Learned

- Per-glyph atlas renderers cannot treat DirectWrite fallback as purely a
  coverage problem. Baseline metrics are part of the glyph contract.
- `DrawText` is acceptable for a cached glyph atlas only if fallback glyphs are
  baseline-corrected. A full Zed-style glyph-run renderer remains the higher
  ceiling, but this fix addresses the concrete drift without replacing the
  renderer.
- Windows font-quality bugs need reporter-facing dev builds and profile logs.
  The tracker should keep #78 open until users confirm the perceived weight is
  comfortable on real Windows displays.
- Glyph weight and raster contrast are separate controls. We should prefer
  semantically correct font selection before tuning DirectWrite contrast values.
