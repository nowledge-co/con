# macOS Terminal Glass Regression

## What Happened

The macOS leak-light hardening made Con look effectively opaque even when Terminal Glass and blur were enabled. The regression was most visible on modern macOS, where the terminal should composite Ghostty's translucent background over the blurred window backdrop.

## Root Cause

The AppKit backing view added for seam safety used the same alpha as Ghostty's terminal background. Ghostty already renders terminal background opacity itself, so the native backing caused a second composition pass behind the Metal surface. With the default glass value, two translucent layers compounded into a nearly opaque result.

The same seam hardening also precomposed static chrome surfaces to fully opaque colors. That hid transparent seam leaks but removed the intended UI glass from the tab bar, input bar, side bar, and agent panel.

## Fix Applied

Modern macOS now keeps the native terminal backing transparent and lets Ghostty own terminal opacity/blur. macOS 12 and older keep the opaque fallback backing because terminal glass is intentionally disabled there.

Static chrome surfaces again use the configured UI opacity. Opaque terminal-colored coverage remains limited to tiny seam guards while terminal glass is active. The native full-window transition underlay is only allowed when the terminal is already opaque, because showing it behind transparent Ghostty cells visibly disables glass for the duration of the animation.

The remaining right-panel and bottom-bar seam leaks were first reduced with macOS-only absolute seam overdraw. This keeps the cover on the moving boundary without inserting extra flex height into the terminal layout and without affecting Windows or Linux.

Pane-divider overdraw was explicitly rejected: it made split edges visibly ugly and still did not address the underlying native-view timing gap. Future work should avoid thickening visible dividers and instead solve the native backing problem directly.

The deeper fix is to make the top-level macOS backing glass-compatible. On modern macOS, Con now uses GPUI's native blurred window backdrop when terminal blur is enabled, so any clear timing gap between GPUI chrome and embedded Ghostty NSViews reveals the same glass material instead of raw desktop pixels.

Even with a blurred top-level backing, rapid bottom-bar, right-panel, top-tab-strip, and vertical-tabs toggles can still show a seam because GPUI layout and embedded AppKit view placement do not commit atomically. On macOS those terminal-adjacent geometry changes now snap instead of animate. Short reservation guards keep the previous chrome slot alive, empty, and glass-compatible while AppKit expands Ghostty's native view into the new bounds. This preserves glass, avoids hard borders, and avoids changing Windows or Linux animations.

## What We Learned

Leak-light fixes must separate three layers:

- Terminal content opacity belongs to Ghostty only.
- Static GPUI chrome should respect the user's UI opacity setting.
- Opaque mattes are only acceptable for tiny seams. A full-window native underlay is acceptable only when the terminal is already opaque.

Any future AppKit backing added under a Metal terminal surface must be checked for double-compositing before shipping.
