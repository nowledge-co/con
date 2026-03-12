# Study: libghostty-vt

## Overview

libghostty-vt is the embeddable VT parsing library extracted from Ghostty. It provides terminal sequence parsing, key encoding, and text attribute handling via a C API.

## What It Gives Us

| Component | What it does | C API header |
|-----------|-------------|--------------|
| **Parser** | DEC ANSI state machine (vt100.net spec). States: ground, escape, CSI, DCS, OSC. Actions: print, execute, csi_dispatch, esc_dispatch, osc_dispatch. | via Zig module |
| **OSC parser** | Window title, hyperlinks (OSC 8), semantic prompts (OSC 133), color schemes | `vt/osc.h` |
| **SGR parser** | Text attributes: bold, italic, underline, 256-color, RGB | `vt/sgr.h` |
| **Key encoder** | Kitty keyboard protocol, xterm/VT102 escape sequences, dead keys, modifiers | `vt/key.h` |
| **Paste validator** | Bracketed paste safety checks | `vt/paste.h` |
| **Color utilities** | RGB extraction, palette management | `vt/color.h` |

## What It Does NOT Give Us

- **Screen/Grid state** — Terminal.zig and Screen.zig are in the full libghostty, not libghostty-vt. We need to implement our own grid that consumes parser actions.
- **Rendering** — Metal/OpenGL renderers are Ghostty app code, not library code.
- **PTY management** — Not in library scope. We use `portable-pty`.
- **Scrollback** — PageList/Page are full-app code.

## Build

```bash
cd 3pp/ghostty
zig build lib-vt                    # builds libghostty_vt shared lib
zig build lib-vt -Dtarget=x86_64-linux-gnu  # cross-compile
```

Outputs: `zig-out/lib/libghostty-vt.{so,dylib}`, headers at `zig-out/include/ghostty/`

Minimum Zig version: **0.15.2** (from build.zig.zon)

## Integration Plan

1. `con-terminal/build.rs` runs `zig build lib-vt` against `3pp/ghostty`
2. `bindgen` generates Rust FFI from `include/ghostty/vt.h`
3. Safe Rust wrapper in `con-terminal/src/vt.rs`
4. Our own `Grid` struct consumes parser actions and maintains screen state
5. Grid exposes cell data for GPUI canvas rendering

## Key Insight

libghostty-vt is a **parser + encoder library**, not a full terminal emulator. We must implement:
- Grid/Screen (cell storage, cursor tracking, scrollback)
- Dirty tracking for efficient rendering
- Selection (copy/paste rectangle tracking)
- Alternate screen buffer switching

This is deliberate — it keeps our grid implementation GPUI-friendly rather than fighting Ghostty's Zig data structures across FFI.

## Examples

See `3pp/ghostty/example/c-vt*/` for C integration patterns. Key files:
- `c-vt/main.c` — OSC parser usage
- `c-vt-key-encode/main.c` — Key encoding with Kitty protocol
- `c-vt-sgr/main.c` — SGR attribute parsing
