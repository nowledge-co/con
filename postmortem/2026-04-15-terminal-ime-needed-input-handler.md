# Terminal IME needed a GPUI input handler

## What happened

Chinese IME composition appeared in the OS candidate UI, but committed text did not reach the embedded terminal.

## Root cause

Con forwarded GPUI key events directly to Ghostty, but did not register a GPUI `InputHandler` for the terminal surface.

On macOS, GPUI only routes committed IME text through the platform text-input path when the focused element has an input handler and opts into IME for printable keys. Without that handler, composed text never had a terminal commit target.

## Fix applied

- Added a lightweight terminal input handler for `GhosttyView`
- Forwarded committed IME text through `ghostty_surface_text`
- Stored minimal marked-text state so AppKit can manage the composition lifecycle
- Opted terminal focus into IME routing for printable keys

## What we learned

Terminal key events and IME text input are separate platform paths. A native terminal embed needs both: raw key events for TUI correctness, and an input handler for composed text.
