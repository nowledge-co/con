# Study: GPUI (Community Edition)

## Overview

GPUI-CE is the community fork of Zed's GPU-accelerated UI framework. Apache 2.0 licensed.

## Key Properties

- **Rendering**: Metal (macOS), Blade/OpenGL (Linux), Blade/D3D (Windows)
- **Text**: Core Text (macOS), cosmic-text (Linux), DirectWrite (Windows)
- **Layout**: taffy (flexbox/CSS Grid)
- **IME**: Full InputHandler trait — macOS AppKit, Linux X11 xim, Windows WM_IME
- **Version**: 0.3.3 (crate: gpui-ce, lib name: gpui)

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

## Scaffolding

`3pp/create-gpui-app` provides templates:
- `default/` — single binary app
- `workspace/` — multi-crate workspace

We'll use the workspace pattern but customize for our crate structure.

## Key Files in 3pp

- `3pp/gpui-ce/src/gpui.rs` — main exports
- `3pp/gpui-ce/examples/learn/` — layout, styling, text, animation examples
- `3pp/create-gpui-app/templates/` — project scaffolding templates
- `3pp/awesome-gpui/README.md` — community apps including `termy` (terminal emulator)

## Dependency

In our Cargo.toml:
```toml
gpui = { path = "3pp/gpui-ce", package = "gpui-ce" }
```
