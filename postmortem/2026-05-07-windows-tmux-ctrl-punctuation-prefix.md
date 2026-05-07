# Windows tmux Ctrl-Punctuation Prefix

## What Happened

Issue #148 reported that this tmux config worked in other terminals but not in
Con on Windows:

```tmux
unbind C-b
set -g prefix C-]
set -g prefix2 C-h
```

`C-h` worked, but `C-]` did not.

## Root Cause

Con's Windows and Linux preview terminal views translate GPUI key events into
terminal bytes themselves. That translation handled `Ctrl+A` through `Ctrl+Z`,
but it did not handle the defined ASCII control punctuation chords:

- `Ctrl+@` / `Ctrl+Space` -> NUL (`0x00`)
- `Ctrl+2` -> NUL (`0x00`)
- `Ctrl+3` -> ESC (`0x1b`)
- `Ctrl+4` -> FS (`0x1c`)
- `Ctrl+5` -> GS (`0x1d`)
- `Ctrl+6` -> RS (`0x1e`)
- `Ctrl+7` / `Ctrl+/` -> US (`0x1f`)
- `Ctrl+8` -> DEL (`0x7f`)
- `Ctrl+[` -> ESC (`0x1b`)
- `Ctrl+\` -> FS (`0x1c`)
- `Ctrl+]` -> GS (`0x1d`)
- `Ctrl+^` -> RS (`0x1e`)
- `Ctrl+_` -> US (`0x1f`)
- `Ctrl+~` -> RS (`0x1e`)
- `Ctrl+?` -> DEL (`0x7f`)

tmux treats `C-]` as `0x1d`, so Con's letter-only mapper meant the prefix never
reached tmux.

macOS was not affected because it sends keys through Ghostty's native key
pipeline instead of Con's portable VT key mapper.

## Fix Applied

Con now has one shared ASCII-control helper used by:

- Windows terminal key handling
- Linux terminal key handling
- the surface control API's `keys.send` parser

That keeps physical user input and orchestrator-driven surface input aligned.
The helper intentionally does not map shifted bracket variants like `Ctrl+}` or
`Ctrl+{`, so Windows/Linux app shortcuts such as `Ctrl+Shift+]` for tab
switching stay app-level.

This is the complete legacy C0 control-byte layer, not the complete modern
keyboard-protocol layer. Full parity with Ghostty on Windows and Linux should
eventually route portable key events through libghostty-vt's key encoder
(`ghostty_key_encoder_*`) so terminal state such as Kitty keyboard protocol,
modifyOtherKeys, keypad application mode, and fixterms stays owned by Ghostty
instead of being hand-maintained in Con.

## Copy-On-Select Note

The same issue asked about copy-on-select under tmux. This fix does not claim to
solve that larger workflow. Local Con selection can be copied through the normal
terminal copy action, and tmux mouse mode still requires terminal-level handling
or tmux/OSC52 clipboard integration. That should be tracked separately so we do
not hide a clipboard protocol gap behind the key-prefix fix.

## What We Learned

Terminal control-key support is not just letters. If Con owns key translation on
a platform, it must implement the complete legacy C0 control-byte layer and keep
those semantics shared with the control-plane surface API. For anything beyond
that layer, the long-term answer is not a larger Rust table; it is reusing
Ghostty's VT key encoder.
