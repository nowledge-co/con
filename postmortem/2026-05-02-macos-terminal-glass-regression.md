# macOS Terminal Glass Regression

## What Happened

The macOS leak-light hardening made Con look effectively opaque even when Terminal Glass and blur were enabled. The regression was most visible on modern macOS, where the terminal should composite Ghostty's translucent background over the blurred window backdrop.

## Root Cause

The AppKit backing view added for seam safety used the same alpha as Ghostty's terminal background. Ghostty already renders terminal background opacity itself, so the native backing caused a second composition pass behind the Metal surface. With the default glass value, two translucent layers compounded into a nearly opaque result.

The same seam hardening also precomposed static chrome surfaces to fully opaque colors. That hid transparent seam leaks but removed the intended UI glass from the tab bar, input bar, side bar, and agent panel.

## Fix Applied

Modern macOS now keeps the native terminal backing transparent and lets Ghostty own terminal opacity/blur. macOS 12 and older keep the opaque fallback backing because terminal glass is intentionally disabled there.

Static chrome surfaces again use the configured UI opacity. Opaque terminal-colored coverage remains limited to tiny seam guards while terminal glass is active. The native full-window transition underlay is only allowed when the terminal is already opaque, because showing it behind transparent Ghostty cells visibly disables glass for the duration of the animation.

The remaining right-panel, bottom-bar, and pane-divider seam leaks were addressed with macOS-only seam overdraw. This keeps the cover on the moving boundary without inserting extra flex height into the terminal layout and without affecting Windows or Linux. Pane dividers keep a subtle 1px visible line, with an opaque terminal-background strip underneath to cover native-view timing gaps.

## What We Learned

Leak-light fixes must separate three layers:

- Terminal content opacity belongs to Ghostty only.
- Static GPUI chrome should respect the user's UI opacity setting.
- Opaque mattes are only acceptable for tiny seams. A full-window native underlay is acceptable only when the terminal is already opaque.

Any future AppKit backing added under a Metal terminal surface must be checked for double-compositing before shipping.
