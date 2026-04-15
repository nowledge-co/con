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

This preserves the embedded Ghostty architecture while giving Monterey users an opaque, readable terminal.

## What We Learned

Native transparency around embedded Metal views must be treated as an OS-version capability, not just a user preference. When the fallback for a visual effect is an unusable terminal, compatibility gates should prefer solid rendering over preserving the effect.
