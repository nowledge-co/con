# Implementation: Terminal Rendering Pipeline

## Overview

con has two terminal rendering backends, selected at compile time by platform. Both are abstracted behind the `TerminalPane` enum so workspace and agent code is backend-agnostic.

## Backend 1: Ghostty (macOS Primary)

```
Ghostty libghostty C API
     │
     ├── GhosttyApp::init()         — one per process
     └── GhosttyTerminal::new()     — one per pane
              │
              ├── ghostty_surface_t  — owns PTY, parser, screen state
              ├── Metal renderer     — GPU text/cell rasterization
              ├── NSView             — composited into GPUI window
              └── Action callbacks   — SET_TITLE, PWD, COMMAND_FINISHED, etc.
```

**Data flow:**

1. User types → GPUI key event → `ghostty_surface_key()` → ghostty processes input → writes to PTY
2. PTY output → ghostty parses VT sequences → updates internal screen → Metal renders to NSView
3. Ghostty fires action callbacks → `TerminalState` updated (title, pwd, exit code, duration)
4. GPUI composites ghostty's NSView alongside its own Metal layer

**Key insight:** Ghostty handles everything from PTY to pixels. con's role is embedding the NSView, forwarding input, and consuming action callbacks for agent integration.

### Action Callbacks

The action callback receives a `(tag, union)` pair. Currently handled:

| Action | What it provides |
|--------|-----------------|
| SET_TITLE | Terminal title (OSC 0/2) |
| PWD | Working directory (OSC 7) |
| RENDER | Signals a frame was rendered |
| COMMAND_FINISHED | Exit code (i16) + duration (ns) from OSC 133;D |
| SHOW_CHILD_EXITED | Shell process exited |
| COLOR_CHANGE | Terminal color scheme changed (OSC 10/11) |
| RING_BELL | BEL character received |

59 additional actions are acknowledged but not yet handled (return `true` to prevent ghostty from logging warnings).

### Clipboard

- **Read** (ghostty wants to paste): Reads `NSPasteboard.generalPasteboard`, calls `ghostty_surface_complete_clipboard_request()`
- **Write** (ghostty wants to copy): Writes to `NSPasteboard.generalPasteboard` via `setString:forType:`

### Threading

Ghostty manages its own threads internally. The action callback fires on ghostty's thread — we lock `TerminalState` (parking_lot::Mutex) to update state, then GPUI's main thread reads it during render/query.

## Backend 2: vte + GPUI Canvas (Fallback)

```
PTY read (async)
     │
     ▼
vte 0.15 Parser
(DEC ANSI state machine)
     │
     ├── print(char)
     ├── execute(C0/C1)
     ├── csi_dispatch(params)
     ├── osc_dispatch(command)
     ├── esc_dispatch(intermediate, final)
     └── dcs_*(data)
          │
          ▼
     Grid (Rust)
     ├── cells[row][col] = Cell { char, style, dirty }
     ├── cursor { row, col, shape, visible }
     ├── scrollback: VecDeque<Row>
     ├── selection: Option<Selection>
     └── dirty_rows: BitVec
          │
          ▼
     GPUI Element (render loop)
     ├── only repaint dirty_rows
     ├── background: fill_rect per cell (batched by color)
     ├── text: draw_text per run (batch consecutive same-style chars)
     ├── cursor: overlay rect or bar
     ├── selection: highlight overlay
     └── scrollbar: optional overlay
```

### Grid Implementation

We implement our own Grid rather than using Ghostty's Screen/PageList because:
1. Ghostty's Screen is Zig — can't efficiently share mutable state across FFI
2. Our Grid is designed for GPUI's paint model (dirty tracking, batch-friendly)
3. Simpler scrollback (VecDeque vs Ghostty's pinned PageList)

```rust
struct Grid {
    cols: usize,
    rows: usize,
    cells: Vec<Vec<Cell>>,      // current screen
    scrollback: VecDeque<Vec<Cell>>,
    cursor: Cursor,
    dirty: BitVec,              // one bit per row
    alternate: Option<Vec<Vec<Cell>>>,  // alternate screen buffer
}

struct Cell {
    char: char,
    style: Style,  // fg, bg, bold, italic, underline, etc.
}
```

### Rendering Strategy

1. **Batch by style**: Group consecutive cells with identical style into text runs. One `draw_text` call per run, not per cell.
2. **Dirty rows only**: BitVec tracks which rows changed since last paint. Only repaint those.
3. **Background batching**: Group adjacent cells with same bg color into single `fill_rect`.
4. **60fps target**: At 200 cols x 50 rows = 10,000 cells. With batching, ~500-1000 draw calls. Well within GPU budget.

### Threading

```
Main thread (GPUI)              IO thread
     │                              │
     │                         PTY read loop
     │                              │
     │                         parse bytes
     │                              │
     │                         update grid (lock)
     │                              │
     │  ◄── notify_dirty ──────────┘
     │
  repaint (read grid, lock)
```

- Grid protected by `Mutex<Grid>` or GPUI's `Entity<Grid>`
- IO thread reads PTY, parses, updates grid, signals dirty
- Main thread reads grid during paint, resets dirty flags

## TerminalPane Abstraction

Both backends are unified via the `TerminalPane` enum in `crates/con/src/terminal_pane.rs`:

```rust
pub enum TerminalPane {
    Legacy(Entity<TerminalView>),
    #[cfg(target_os = "macos")]
    Ghostty(Entity<GhosttyView>),
}
```

Every method dispatches to the appropriate backend. Agent-facing methods (`content_lines`, `recent_lines`, `last_exit_code`, `take_command_finished`, `is_busy`, `detected_remote_host`, `search_text`) work identically regardless of backend.
