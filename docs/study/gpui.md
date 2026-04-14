# Study: GPUI

## Overview

con currently builds against the upstream GPUI crates from the Zed repository, not a local GPUI-CE checkout. The framework remains Apache 2.0 licensed.

## Key Properties

- **Rendering**: Metal (macOS), Blade/OpenGL (Linux), Blade/D3D (Windows)
- **Text**: Core Text (macOS), cosmic-text (Linux), DirectWrite (Windows)
- **Layout**: taffy (flexbox/CSS Grid)
- **IME**: Full InputHandler trait — macOS AppKit, Linux X11 xim, Windows WM_IME
- **Source**: upstream `zed-industries/zed` git dependency (crate name: `gpui`)

## Programming Model

Three registers:
1. **Entity** — state management (like React state + context)
2. **Views** — high-level declarative UI (struct + Render trait)
3. **Elements** — low-level imperative UI (custom paint, canvas)

Terminal rendering uses the **Element** register: custom `canvas()` calls to paint cells directly to GPU.

## Terminal Canvas Pattern

```rust
// Pseudocode for terminal cell rendering
fn paint_terminal(grid: &Grid, cx: &mut PaintContext) {
    for row in 0..grid.rows {
        for col in 0..grid.cols {
            let cell = grid.cell(row, col);
            // Background
            cx.fill_rect(cell_rect(row, col), cell.bg_color);
            // Text
            cx.draw_text(cell.char, cell_origin(row, col), cell.fg_color, &font);
        }
    }
}
```

GPUI's text shaping handles ligatures, emoji, CJK — same quality as Zed's editor.

## Reference Sources

Read these upstream or read-only reference sources when you need framework details:

- upstream `gpui` crates under the Zed repository checkout
- `3pp/` reference material in this repo, when present, as read-only study material only

## Dependency

In our workspace manifest, GPUI resolves from the upstream Zed git source rather than a local path dependency.
