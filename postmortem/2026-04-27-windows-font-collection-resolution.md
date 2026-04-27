# Windows font collection resolution

## What Happened

Issue #78 reported incomplete and misaligned text on Windows when the terminal font was set to a system font such as JetBrains Maple Mono. The screenshot showed narrow glyph ink inside overly wide cells, producing words that looked split apart.

## Root Cause

The Windows DirectWrite renderer normalized the bundled IoskeleyMono family name correctly, but it kept passing the bundled private font collection to `CreateTextFormat` even when the requested family was a user-selected system font. Cell metrics could then be measured from a system font face while glyph rasterization was resolved through the bundled collection's fallback path.

That split violated the terminal renderer invariant: text format, primary face, and cell metrics must resolve from the same font source.

## Fix Applied

`GlyphCache` now resolves the font family and owning collection together:

- Bundled IoskeleyMono uses the private bundled collection.
- User-selected system fonts use DirectWrite's system collection by passing `None`.
- Missing fonts fall back to installed monospace system fonts before Segoe UI, so an unavailable default font cannot stretch ASCII through a proportional UI face.
- Font-size rebuilds reuse the same resolved collection instead of reintroducing the mismatch.
- Wide glyphs rasterize into two terminal cells using `unicode-width`, removing the hand-written East Asian Width table that could drift from Unicode coverage.

## What We Learned

For DirectWrite terminal rendering, "family string" alone is not enough state. The family and collection are a single resolution result, and every later text-format or metric operation must use that same result. Otherwise DirectWrite can silently measure one font and draw another.

CJK fallback is a separate invariant: when the primary mono font does not contain a codepoint, DirectWrite may choose a CJK fallback face, but the atlas still has to allocate the terminal display width for that codepoint. A wide CJK glyph drawn into a one-cell slot will look like clipped text followed by a blank spacer cell.

Do not maintain Unicode range tables by hand in renderer code. If libghostty-vt exposes its width lookup through FFI later, the atlas should call that directly. Until then, `unicode-width` is the single Rust-side source of truth, and ambiguous-width characters stay one cell unless the terminal layer grows an explicit ambiguous-width mode.
