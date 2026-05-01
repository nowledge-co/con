# Monterey Transparent Terminal Window

## What Happened

On macOS Monterey 12.7.4, the beta app could open a fully transparent terminal window with no visible prompt or input surface. The app chrome existed, but the embedded terminal content effectively disappeared into the desktop background.

## Root Cause

Con uses a transparent GPUI window so the embedded Ghostty `NSView` can render terminal glass underneath the GPUI chrome. That path depends on macOS WindowServer composition, Ghostty's Metal-backed embedded view, non-opaque terminal background, and optional blur. On macOS 12, that combination is fragile enough to produce a blank transparent surface instead of a readable terminal.

The failure mode is compatibility-specific rather than a normal layout issue: newer macOS versions render the same glass path correctly, while Monterey can fail before the user sees any terminal text.

## Fix Applied

Con now detects the runtime macOS major version and disables terminal glass on macOS 12 and older:

- terminal background opacity is forced to `1.0`
- terminal blur is forced off
- the user's configured glass values remain saved and take effect again on supported macOS versions

We later found that the first fallback pass overshot: it also made the GPUI root opaque. Because Con hosts the embedded Ghostty `NSView` *under* GPUI, that beige fallback surface painted over the terminal and produced a different failure mode: the window was no longer transparent, but the terminal area stayed blank.

The corrected Monterey fallback keeps two separate rules:

- the top-level macOS window is opaque, so the desktop cannot bleed through
- the GPUI root above the embedded terminal remains transparent, so the Ghostty surface under it is still visible

This preserves the embedded Ghostty architecture while giving Monterey users a solid window frame without hiding the terminal itself.

New feedback on beta 37 showed a third failure mode: the terminal area became an opaque black rectangle, but shell output was still not visible. That points at the embedded Ghostty layer rather than the GPUI chrome: Ghostty turns the passed surface `NSView` into a layer-hosting view by assigning its IOSurface `CALayer` directly, and older AppKit compositors can fail to keep that hosted layer's bounds synchronized when Con mutates the native frame from GPUI layout callbacks.

On macOS 12 and older, Con now explicitly re-syncs the Ghostty-owned hosted layer's frame, bounds, contents scale, and display invalidation whenever the embedded surface backing or deterministic surface frame is updated. The fallback still does not make the GPUI root opaque, and it remains macOS-only. Modern macOS keeps Ghostty's existing layer ownership unchanged because forcing the hosted layer geometry there changes edge composition around the terminal surface.

## What We Learned

Native transparency around embedded Metal views must be treated as an OS-version capability, not just a user preference. Just as important: for embedded native views, "make the window opaque" and "make the UI root opaque" are not interchangeable fixes. The window can safely fall back to opaque while the host UI above the embedded surface still needs transparency to avoid painting over the terminal.

For embedded layer-hosting views, the host app sometimes owns geometry invariants. Passing an `NSView` to Ghostty is not enough if Con later drives that view's frame outside normal AppKit layout; on old macOS releases, the hosted CALayer must be kept in lockstep with the view bounds. On modern macOS, Ghostty's own layer geometry should be left alone unless a concrete compositor bug proves otherwise.
