# Implementation: Terminal Rendering Pipeline

## Overview

Data flows from PTY → parser → grid → GPUI canvas. Each stage is decoupled.

## Pipeline

```
PTY read (async)
     │
     ▼
libghostty-vt Parser
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

## Grid Implementation

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

## Rendering Strategy

1. **Batch by style**: Group consecutive cells with identical style into text runs. One `draw_text` call per run, not per cell.
2. **Dirty rows only**: BitVec tracks which rows changed since last paint. Only repaint those.
3. **Background batching**: Group adjacent cells with same bg color into single `fill_rect`.
4. **60fps target**: At 200 cols x 50 rows = 10,000 cells. With batching, ~500-1000 draw calls. Well within GPU budget.

## Threading

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
