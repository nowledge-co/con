# Windows Terminal Scroll Direction

## What Happened

On Windows, Con's terminal viewport scrolled in the opposite direction from the user's configured classic scroll direction. The visible behavior felt like natural scrolling even when the system was configured for classic scrolling.

## Root Cause

The Windows GPUI event stream uses the same terminal/editor convention as Zed: positive vertical scroll means "scroll up". Our Windows terminal backend passed that sign directly into `libghostty-vt`, whose viewport API uses negative rows for "up" and positive rows for "down". The same inverted assumption also affected alternate-screen cursor-key fallback and SGR mouse-wheel reports.

## Fix Applied

The Windows host view now keeps GPUI scroll rows as the user intent, maps positive rows to cursor-up / SGR scroll-up, and only inverts the sign at the `libghostty-vt` viewport boundary. Small unit tests lock the three sign conventions:

- SGR wheel button `64` is scroll-up.
- Alternate-screen positive scroll rows send cursor-up.
- `libghostty-vt` viewport deltas invert GPUI rows.

## What We Learned

Do not treat platform scroll deltas as raw terminal deltas. The UI layer and terminal parser can legitimately use opposite signs, so each boundary needs a named conversion instead of inline sign arithmetic.
