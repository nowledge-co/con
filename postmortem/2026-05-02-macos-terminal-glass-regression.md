# macOS Terminal Glass and Chrome Seam Regression

## What Happened

Con repeatedly regressed while trying to remove macOS "leak light" at the seams between embedded Ghostty terminal `NSView`s and GPUI chrome. The visible failures were:

- fast hide/show of the input bar, agent panel, vertical tab bar, or top tab strip could reveal bright desktop pixels through the transparent window backing
- split-pane dividers could lose their visible edge, or become too heavy/ugly when overcovered
- some hardening attempts made the whole terminal effectively opaque even when Terminal Glass and blur were enabled
- some hardening attempts fixed the leak but introduced a second blink where the terminal became non-transparent during chrome motion

The issue is modern-macOS-specific. macOS 12 and older intentionally keep terminal glass disabled through the Monterey fallback described in `2026-04-15-monterey-transparent-terminal.md`.

## Root Cause

Con's macOS window is intentionally transparent so the embedded Ghostty Metal surface can provide terminal glass underneath GPUI chrome. The hard part is that two layout systems are involved:

- GPUI paints chrome and computes flex layout
- AppKit positions the embedded Ghostty `NSView`

During fast chrome motion those two commits are not atomic. For one frame, GPUI can expose a new region before AppKit has expanded the Ghostty surface into it. If the root window backing is clear, that pixel shows the desktop instead of terminal glass.

The first seam fixes treated the problem as a normal border problem. That was incomplete. A 1-4 px strip can cover a stable boundary, but it cannot cover a boundary whose old and new geometry are both visible during the same layout race.

The second class of fixes used opaque native or GPUI backing below the terminal. That hid the leak but broke glass:

- Ghostty already renders terminal background opacity itself, so an AppKit backing view with terminal alpha double-composited behind the Metal surface.
- A full-window native underlay made the terminal look opaque for the duration of chrome animation.
- Fully opaque chrome precomposition removed the intended UI glass from the tab bar, input bar, sidebar, and agent panel.

The key mistake was trying to solve a native-surface timing bug with either tiny decorative borders or whole-window mattes. One is too small; the other is too broad.

## Rejected Or Superseded Attempts

- Full-terminal GPUI mattes: fixed clear gaps but produced an obvious terminal flash. Keep the older `2026-04-15-*matte*` postmortems as historical proof that full-area mattes are the wrong layer.
- Thick pane-divider overdraw: made split edges visually ugly and still did not solve chrome seams.
- Shared native underlay during every chrome transition: caught some gaps, but made transparent terminals look non-transparent while animation was running.
- Treating all macOS versions the same: old macOS needs the Monterey opaque-window fallback, but modern macOS should keep glass.
- Making static GPUI chrome fully opaque: reduced leak risk but violated the user's configured UI opacity.

## Fix Applied

Modern macOS now keeps the native terminal backing transparent and lets Ghostty own terminal opacity/blur. macOS 12 and older keep the opaque fallback backing because terminal glass is intentionally disabled there.

Static chrome surfaces again use the configured UI opacity. Opaque terminal-colored coverage remains limited to tiny seam guards while terminal glass is active. The native full-window transition underlay is only allowed when the terminal is already opaque, because showing it behind transparent Ghostty cells visibly disables glass for the duration of the animation.

Modern macOS uses GPUI's native blurred window backdrop when terminal blur is enabled and the effective terminal opacity is translucent. That way, if a rare clear timing gap appears, it reveals the same glass material instead of raw desktop pixels.

Terminal-adjacent chrome geometry now snaps on macOS instead of animating the terminal boundary. The visual panels can still animate their own content, but the actual terminal layout should not chase a moving decorative edge.

Short reservation guards keep the previous chrome slot alive, empty, and glass-compatible while AppKit expands Ghostty's native view into the new bounds. A short release cover handles the first frame after the reservation drops. This is deliberately macOS-only; Windows and Linux do not embed a separate AppKit `NSView` under GPUI.

Pane dividers are visible again using a subtle terminal-derived foreground tint precomposed over the terminal background. The divider pixel stays opaque enough to avoid a clear gap, but it does not become a heavy bar.

## Current Status

The current fix restores Terminal Glass and blur on modern macOS, keeps the Monterey fallback separate, and avoids the obvious opaque-animation blink. Manual testing shows the leak frequency is much lower, with the remaining rare case most often seen during extremely rapid input-bar toggles.

This is not a fully closed problem. The remaining gap is a native-layout transaction issue: GPUI chrome and embedded AppKit surface placement can still disagree for a frame under abusive repeated toggles. More timing overlays may reduce probability, but they risk reintroducing the same opacity and border regressions.

## TODO

- Reproduce the rare remaining input-bar leak with instrumentation around GPUI layout time, AppKit `NSView` frame apply time, and release-cover expiry.
- Prefer a native transaction-level fix if GPUI exposes one: update terminal-adjacent GPUI chrome and Ghostty host frames in one AppKit-layout boundary instead of covering after the fact.
- If no native transaction hook exists, make the release-cover lifetime adaptive to observed AppKit frame catch-up instead of a fixed short duration.
- Keep terminal glass validation in the release smoke test: transparent terminal, blur on, dark terminal theme, light desktop, rapid toggles for input bar, agent panel, vertical tabs, top tabs, split/unzoom.
- Never reintroduce full-window or full-terminal mattes for this class of bug. They hide clear pixels by breaking the product's glass design.
- Keep Windows and Linux out of this path. Their terminal renderers do not have the same AppKit child-view composition race.

## What We Learned

Leak-light fixes must separate four layers:

- Terminal content opacity belongs to Ghostty only.
- Static GPUI chrome should respect the user's UI opacity setting.
- Native window backing should be glass-compatible on modern macOS, not clear desktop and not opaque matte.
- Opaque coverage is only acceptable for tiny stable seams or short release guards; it must never veil the terminal content.

Any future AppKit backing added under a Metal terminal surface must be checked for double-compositing before shipping. Any future chrome animation touching a terminal edge must first answer whether it is decorative motion or terminal-layout motion; decorative motion can animate, terminal-layout motion should snap or be committed atomically.
