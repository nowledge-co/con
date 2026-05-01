# macOS Chrome Seam Leaks

## What Happened

On macOS, quickly hiding/showing the agent panel, hiding/showing the input bar, toggling the tab bar, or dragging pane/chrome seams could expose a thin bright flash between the embedded terminal surface and GPUI chrome. The effect was most visible with a dark terminal theme inside a light transparent window/backdrop.

## Root Cause

Con's macOS root window is intentionally transparent so Ghostty's native `NSView` can provide terminal glass. Some moving seams were either fully transparent for a frame, covered with UI chrome colors (`theme.background` / `theme.title_bar`) instead of the adjacent terminal color, or covered with a terminal-colored GPUI strip that still used terminal opacity. GPUI strips do not get Ghostty's native blur/compositing, so translucent seam covers still revealed the WindowServer backdrop during fast layout changes.

The earlier full-surface matte approach solved the leak but created a worse blink by veiling the entire terminal. The correct level of intervention is the seam, not the whole terminal.

## Fix Applied

- Agent-panel, input-bar, and top-bar transition seam covers now use the active terminal theme background at full opacity on macOS.
- Static chrome edges that sit next to the terminal, including the vertical-tabs edge and input-bar edge, use the same terminal-derived seam color.
- Pane dividers receive the same terminal-derived divider color from the workspace, so split seams match the adjacent terminal surface instead of the UI chrome.
- Windows and Linux keep their existing GPUI divider colors; the leak is specific to the macOS transparent-window/native-NSView composition path.

## What We Learned

- Treat embedded-terminal seams as native-surface integration points, not normal app borders.
- A seam cover must visually match the adjacent surface and be opaque unless it is backed by the same native blur/compositing path as the terminal.
- Do not hide edge artifacts with full-area mattes. They trade a small seam bug for a whole-terminal blink and make future animation polish harder.
