# macOS Terminal Scroll Precision

## What Happened

Fast scrolling in macOS terminal panes felt less smooth and less direct than standalone Ghostty, especially with trackpads over scrollback-heavy terminal content.

## Root Cause

Con forwarded GPUI scroll-wheel events to `ghostty_surface_mouse_scroll`, but reused the normal keyboard modifier bitmask as the scroll modifier bitmask. Ghostty's scroll API expects a different packed `ScrollMods` struct where bit 0 means high-precision scrolling. As a result, precise AppKit trackpad deltas arrived at Ghostty as coarse non-precision wheel ticks.

Con also divided precise deltas by the window scale factor. Ghostty's own AppKit host sends `NSEvent.scrollingDeltaX/Y` directly and applies a 2x precise-scroll multiplier before handing the event to Ghostty core.

## Fix Applied

- Added an explicit `GHOSTTY_SCROLL_MODS_PRECISION` FFI constant.
- Sent GPUI `ScrollDelta::Pixels` with Ghostty's precision bit instead of keyboard modifier bits.
- Matched Ghostty's AppKit host by applying the same 2x multiplier to precise scroll deltas and not dividing by backing scale.
- Cached native scroll-container frame synchronization so repeated terminal drains do not reapply unchanged AppKit frames.

## What We Learned

Ghostty's mouse scroll modifier type is not interchangeable with key or mouse-button modifiers. Any embedded host code that calls Ghostty's C API must mirror the upstream AppKit adapter closely, especially for input paths where small semantic differences are felt immediately by users.
