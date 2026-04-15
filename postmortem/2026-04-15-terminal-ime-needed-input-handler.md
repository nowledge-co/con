# Terminal IME needed a GPUI input handler

## What happened

Chinese IME composition appeared in the OS candidate UI, but committed text did not reach the embedded terminal.

After the first fix, two follow-up edge cases appeared:

- pinyin/preedit keystrokes could leak into the terminal before the final Chinese text was committed
- when the Chinese IME was switched to English mode, plain ASCII commits were dropped

## Root cause

Con forwarded GPUI key events directly to Ghostty, but did not register a GPUI `InputHandler` for the terminal surface.

On macOS, GPUI only routes committed IME text through the platform text-input path when the focused element has an input handler and opts into IME for printable keys. Without that handler, composed text never had a terminal commit target.

The first repair also exposed a second ordering problem. If printable keys were routed to Ghostty directly while an IME input source was active, pinyin keystrokes reached the PTY before AppKit committed the composed text. But simply dropping ASCII `insertText` commits was too broad, because Chinese IME English mode commits legitimate ASCII through the same AppKit path.

## Fix applied

- Added a lightweight terminal input handler for `GhosttyView`
- Forwarded committed IME text through `ghostty_surface_text`
- Stored minimal marked-text state so AppKit can manage the composition lifecycle
- Opted terminal focus into IME routing for printable keys
- Used Ghostty's real IME cursor anchor for candidate-window placement
- Made terminal key forwarding explicitly consume GPUI key events after Ghostty receives them
- Stopped filtering ASCII `insertText` commits, so Chinese IME English mode can type normally

## What we learned

Terminal key events and IME text input are separate platform paths. A native terminal embed needs both: raw key events for TUI correctness, and an input handler for composed text.

The key invariant is: raw terminal key events should consume the GPUI key event once forwarded, while AppKit `insertText` should remain the source of truth for IME commits. Heuristics like "drop ASCII commits" break real IME modes.
