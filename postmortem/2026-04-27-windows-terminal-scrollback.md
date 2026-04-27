# Windows terminal scrollback

## What Happened

Windows panes accepted mouse-wheel events, but normal shell scrollback did not move. Wheel gestures only reached applications that explicitly enabled terminal mouse tracking.

## Root Cause

The Windows backend uses `libghostty-vt` plus our own GPUI/D3D renderer, not Ghostty's native surface. The app-layer wheel handler correctly forwarded SGR wheel reports to mouse-tracking TUIs, but returned early when mouse tracking was disabled. Unlike macOS, there was no native Ghostty surface underneath to perform viewport scrolling for the primary screen.

## Fix Applied

- Exposed `ghostty_terminal_scroll_viewport` through the Rust VT wrapper.
- Routed non-mouse-tracking wheel gestures to libghostty-vt viewport scrolling.
- Accumulated high-resolution touchpad deltas so fractional pixel scrolls become whole terminal-row scrolls without jumping one row per tiny event.
- Mirrored Ghostty's alternate-screen behavior by converting wheel gestures to cursor keys when alternate-scroll mode is active and mouse tracking is not.

## What We Learned

Windows and Linux cannot assume Ghostty surface behavior exists just because the VT parser is Ghostty. Any host interaction that the macOS embedded surface handles natively must be deliberately re-exposed through the carved-out VT API and wired in the platform view.
