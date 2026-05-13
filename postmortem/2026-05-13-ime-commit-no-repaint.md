# IME committed text reached PTY but terminal did not repaint

## What happened

On macOS, Chinese IME input (both system Pinyin and WeChat IME) showed the
candidate window correctly but committed text never appeared in the terminal.
The characters were silently dropped from the user's perspective.

## Root cause

`GhosttyInputHandler::replace_text_in_range` and
`replace_and_mark_text_in_range` both called `self.view.update(cx, |view, _|
{ ... })` — discarding the `&mut Context<GhosttyView>` closure argument — so
`cx.notify()` was never called after the IME commit.

When the user selects a candidate word, AppKit calls `insertText:` →
`replace_text_in_range`. The handler correctly called `terminal.send_text()`
and `terminal.refresh()`, so the bytes reached the PTY. But without
`cx.notify()`, GPUI did not schedule a redraw, so the terminal surface never
repainted and the committed text was invisible.

The same omission in `replace_and_mark_text_in_range` meant the preedit
(marked text) phase also did not trigger a repaint, though this was less
noticeable because Ghostty renders the composition inline.

`window.invalidate_character_coordinates()` was also missing from all three
callbacks (`replace_text_in_range`, `replace_and_mark_text_in_range`,
`unmark_text`), which could cause the IME candidate window to appear at a
stale position after the cursor moved.

## Fix applied

In all three `GhosttyInputHandler` callbacks:

- Changed `|view, _|` to `|view, cx|` and added `cx.notify()` so GPUI
  schedules a repaint after every IME state change.
- Changed `_window` to `window` and added
  `window.invalidate_character_coordinates()` so the candidate window
  position is refreshed each time.

## What we learned

`InputHandler` callbacks receive `&mut Window` and (via `view.update`) a
`&mut Context<V>`. Both must be used:

- `cx.notify()` — required after any state change that should be visible on
  screen. Without it GPUI will not redraw even if the underlying PTY state
  changed.
- `window.invalidate_character_coordinates()` — required after any IME state
  change so the platform can re-query `bounds_for_range` for candidate window
  placement.

The pattern `self.view.update(cx, |view, _| { ... })` silently discards the
context and is a footgun in input handler callbacks. Always name the second
closure argument and call `cx.notify()` when state changes.
