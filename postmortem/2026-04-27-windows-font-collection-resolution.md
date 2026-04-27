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
- Missing fonts fall back to Segoe UI consistently for both metrics and rasterization.
- Font-size rebuilds reuse the same resolved collection instead of reintroducing the mismatch.

## What We Learned

For DirectWrite terminal rendering, "family string" alone is not enough state. The family and collection are a single resolution result, and every later text-format or metric operation must use that same result. Otherwise DirectWrite can silently measure one font and draw another.
