# Study: Ghostty Integration

## Decision: Full Ghostty via libghostty C API

We initially planned to use **libghostty-vt** — the standalone VT parser library extracted from Ghostty — and build our own Grid, renderer, and scrollback on top. After evaluation, we chose to embed **full Ghostty** instead via its libghostty C API.

### Why Full Ghostty Over libghostty-vt

| Consideration | libghostty-vt (parser only) | libghostty (full terminal) |
|---|---|---|
| VT compliance | Parser actions only — we build Grid, screen state, scrollback | Complete: parser + screen + scrollback + renderer |
| Rendering | Our own GPUI canvas rendering | Metal GPU rendering via NSView |
| Shell integration | We implement OSC 133 handling | Built-in: COMMAND_FINISHED with exit code + duration |
| Clipboard | We implement via PTY + pasteboard | Built-in: read/write callbacks |
| Maintenance | We maintain ~3000 lines of Grid + rendering | ~800 line FFI wrapper |
| Build | Zig toolchain + bindgen | Pre-built libghostty, Rust FFI only |

**The deciding factor:** COMMAND_FINISHED. Ghostty fires this action when OSC 133;D arrives, providing the exit code and nanosecond-precision duration of the completed command. This single capability eliminates the 3-second blind timeout in `terminal_exec`, making agent command execution instant and reliable. Building this from libghostty-vt would have required reimplementing Ghostty's shell integration handling.

### Architecture

```
con process
├── GhosttyApp (singleton)
│   └── ghostty_app_t — runtime config, font discovery
│
├── GhosttyTerminal (per pane)
│   ├── ghostty_surface_t — owns PTY, parser, screen, renderer
│   ├── NSView — Metal rendering surface, composited in GPUI window
│   ├── TerminalState — action-driven state (title, pwd, exit code, busy)
│   └── Callbacks:
│       ├── action_callback — 66 actions (7 handled, 59 acknowledged)
│       ├── read_clipboard_callback — paste from NSPasteboard
│       ├── write_clipboard_callback — copy to NSPasteboard
│       ├── close_surface_callback — child process exited
│       └── size_report_callback — terminal dimensions
│
└── GhosttyView (GPUI Element)
    ├── Forwards key/mouse events to ghostty_surface_key/mouse
    ├── Manages NSView lifecycle (add/remove from window)
    └── Handles focus tracking
```

### What libghostty-vt Would Have Given Us

(Preserved for reference — this was our original plan.)

| Component | What it does | C API header |
|-----------|-------------|--------------|
| **Parser** | DEC ANSI state machine. States: ground, escape, CSI, DCS, OSC. | via Zig module |
| **OSC parser** | Window title, hyperlinks (OSC 8), semantic prompts (OSC 133) | `vt/osc.h` |
| **SGR parser** | Text attributes: bold, italic, underline, 256-color, RGB | `vt/sgr.h` |
| **Key encoder** | Kitty keyboard protocol, xterm/VT102 escape sequences | `vt/key.h` |
| **Paste validator** | Bracketed paste safety checks | `vt/paste.h` |

It does **not** provide: Screen/Grid state, rendering, PTY management, or scrollback. All of those would have been our responsibility.

### Current Action Coverage

7 of 66 ghostty actions are handled with specific logic. The remaining 59 return `true` (acknowledged) to prevent ghostty from logging unhandled-action warnings.

**Handled:**
- `SET_TITLE` — updates terminal tab title
- `PWD` — updates working directory (OSC 7)
- `RENDER` — signals frame rendered, sets `needs_render`
- `COMMAND_FINISHED` — captures exit code + duration, clears busy state, records history
- `SHOW_CHILD_EXITED` — marks terminal as exited
- `COLOR_CHANGE` — triggers re-render
- `RING_BELL` — plays system beep via `NSBeep()`

**Not yet handled (future work):**
- `OPEN_URL` — hyperlink clicks
- `START_SEARCH` / `END_SEARCH` — native terminal search
- `MOUSE_SHAPE` — cursor shape changes
- `DESKTOP_NOTIFICATION` — system notifications from terminal apps
- `SET_MOUSE_VISIBILITY` — show/hide cursor during typing
