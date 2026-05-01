# macOS Chrome Seam Leaks

## What Happened

On macOS, quickly hiding/showing the agent panel, hiding/showing the input bar, toggling the tab bar, or dragging pane/chrome seams could expose a thin bright flash between the embedded terminal surface and GPUI chrome. The effect was most visible with a dark terminal theme inside a light transparent window/backdrop.

## Root Cause

Con's macOS root window is intentionally transparent so Ghostty's native `NSView` can provide terminal glass. Some moving seams were either fully transparent for a frame, covered with UI chrome colors (`theme.background` / `theme.title_bar`) instead of the adjacent terminal color, or covered with a terminal-colored GPUI strip that still used terminal opacity. GPUI strips do not get Ghostty's native blur/compositing, so translucent seam covers still revealed the WindowServer backdrop during fast layout changes.

The first follow-up fix still treated the symptom as a 1–4 px border problem. That was incomplete. During fast hide/show and split changes, AppKit's native terminal view and GPUI's flex layout can be temporarily out of phase. Any exposed pixel outside the exact strip still fell through to the transparent window backing. The right agent panel was the remaining bad case because it animated layout width; its boundary could move far more than the seam cover in a single ease-out frame.

The earlier full-surface matte approach solved the leak but created a worse blink by veiling the entire terminal. The correct level of intervention is the seam, not the whole terminal.

## Fix Applied

- Agent-panel, input-bar, and top-bar transition seam covers now use the active terminal theme background at full opacity on macOS.
- Static chrome edges that sit next to the terminal, including the vertical-tabs edge and input-bar edge, use the same terminal-derived seam color.
- Pane dividers receive the same terminal-derived divider color from the workspace, so split seams match the adjacent terminal surface instead of the UI chrome.
- macOS chrome surfaces that sit outside the native terminal area are now precomposed over the terminal background before being painted by GPUI. They visually read like translucent chrome over the terminal, but they no longer blend against the desktop through a clear window pixel.
- The native Ghostty host view now keeps a tiny backing overdraw under GPUI seams while keeping the Metal surface aligned to the pane bounds. If GPUI and AppKit disagree for a frame during split, zoom, sidebar, agent-panel, or input-bar motion, the revealed backing is terminal-colored instead of clear.
- During macOS chrome transitions, the workspace enables a shared native underlay below all Ghostty hosts in the window. Visibility is tracked with a parent-scoped owner set so one pane cannot hide the shared underlay while another pane is still transitioning. It fills otherwise-clear window backing during input-bar, agent-panel, tab-strip, and vertical-tabs geometry races, then hides after the transition guard expires.
- On macOS, the right agent panel no longer uses its animated width as terminal layout input. The terminal/panel boundary snaps to stable open/closed geometry while the panel content animates, avoiding a fast-moving native/GPUI seam that no fixed strip can cover reliably.
- Windows and Linux keep their existing GPUI divider colors; the leak is specific to the macOS transparent-window/native-NSView composition path.

## What We Learned

- Treat embedded-terminal seams as native-surface integration points, not normal app borders.
- A seam cover must visually match the adjacent surface and be opaque unless it is backed by the same native blur/compositing path as the terminal.
- When chrome is outside the native terminal view but visually adjacent to it, precompose the chrome over the terminal background on macOS. Relying on alpha against a transparent root lets the desktop become part of the UI color.
- Native embedded surfaces should slightly overdraw their backing under GPUI seams, while keeping terminal content geometry exact. Exact-fit native frames are fragile when two layout systems update in different phases.
- A temporary native underlay is preferable to a GPUI matte for transition races: below-terminal backing can catch clear pixels without veiling terminal text.
- Do not feed decorative chrome animation directly into native terminal layout when that chrome owns a terminal boundary. Animate visual reveal separately from terminal geometry.
- Do not hide edge artifacts with full-area mattes. They trade a small seam bug for a whole-terminal blink and make future animation polish harder.
