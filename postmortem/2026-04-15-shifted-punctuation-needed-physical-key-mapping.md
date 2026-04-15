# Shifted punctuation needed physical key mapping

## What happened

Embedded terminal input broke `:` in Vim, which meant users could not reliably enter command mode and exit.

## Root cause

Con mapped many printable keys by string value only. Shifted punctuation such as `:` fell through to the text-input fallback instead of being sent as a real key event on the semicolon key with the shift modifier.

That fallback path is not equivalent for terminal applications like Vim, which expect the physical key event and modifiers.

## Fix applied

- Expanded GPUI-to-Ghostty key mapping to treat shifted punctuation as the same physical key with the correct unshifted codepoint
- Covered the full US punctuation set instead of fixing `:` alone

## What we learned

Terminal embedding cannot treat printable text and keyboard events as interchangeable. TUI apps care about the physical key, modifiers, and unshifted identity, especially for normal-mode command keys.
