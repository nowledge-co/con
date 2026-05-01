# Windows and Linux CJK IME input

## What happened

Windows and Linux terminal panes accepted raw key events and clipboard paste, but CJK IME commits did not reach the PTY reliably. Plain keyboard input was translated directly in `windows_view.rs` and `linux_view.rs`, while GPUI's platform IME plumbing had no focused terminal `InputHandler` registered for those panes.

## Root cause

The macOS terminal path already registers an input handler around the embedded Ghostty view. The Windows and Linux preview terminal views did not. GPUI already receives Windows IMM32 composition events, X11 XIM commits/preedit, and Wayland text-input-v3 commits/preedit, but without an active `InputHandler` it cannot deliver committed text or ask for a cursor rectangle for candidate placement.

## Fix applied

Added non-macOS terminal input handlers that:

- keep IME marked text and selected-range state for composition tracking;
- send committed text to the focused terminal PTY;
- report cursor-relative bounds so candidate windows anchor at the terminal cursor;
- route plain printable input through the platform text-input path while preserving terminal handling for control, alt/meta, and special keys.

The macOS `ghostty_view.rs` path was left unchanged.

## What we learned

The non-macOS terminal views need to participate in GPUI text input even though they still own terminal-specific key encoding. Printable text and IME commits should enter through `InputHandler`; terminal-only key semantics should stay in the keydown encoder. Composition is not just a commit event: the marked-text caret range must also be tracked so Windows and Linux IMEs can place candidate UI at the right terminal cell during preedit.
