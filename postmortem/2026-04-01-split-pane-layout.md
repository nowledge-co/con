# Split Pane Layout, Focus, and Drag-to-Resize

**Date:** 2026-04-01
**Severity:** High — split panes unusable (overlap, no focus switch, broken resize)

## What happened

After implementing split panes (Cmd+D), three critical bugs made the feature unusable:

1. **Left pane not resizing** — after split, the original (left) terminal kept rendering at full width, overlapping behind the new right pane
2. **Cannot focus left pane** — clicking the left terminal after creating a right split had no effect; keyboard input always went to the right pane
3. **Drag-to-resize broken** — divider drag either didn't work or used wrong total size

## Root causes

### 1. Double-wrapped flex layout (layout overlap)

The Leaf node rendered with `flex_basis(relative(0.)).flex_grow().flex_shrink()`. The Split then wrapped that in another div with `flex_basis(relative(ratio)).flex_grow().flex_shrink()`. The inner flex properties were meaningless because the wrapping div wasn't a flex container — it was a plain block div. GPUI/Taffy couldn't propagate size constraints through this nesting, so the terminal kept its full-width bounds.

Additionally, the split container used `.flex_1().min_w_0().min_h_0()` instead of `.size_full()`, and split children lacked explicit cross-axis sizing (`.h_full()` for horizontal splits).

**Key insight:** Zed's `split_editor_view.rs` uses a precise pattern:
```rust
// Parent: h_flex().size_full()
// Each child:
div()
    .flex_shrink()
    .min_w_0()
    .h_full()                                    // explicit cross-axis
    .flex_basis(DefiniteLength::Fraction(ratio))
    .overflow_hidden()
    .child(content)
```
No double wrapping. No flex_grow on children (only flex_basis + flex_shrink).

### 2. GPUI entity render independence (focus bug)

In GPUI, each Entity renders independently. `cx.notify()` on a terminal entity does NOT trigger the workspace entity's `render()`. The workspace's `sync_focus()` (which checks `terminal.focus_handle(cx).is_focused(window)`) only ran during workspace render — which never happened when a terminal was clicked.

The terminal's `on_mouse_down` called `window.focus(&handle, cx)` (correctly transferring window focus) and `cx.notify()` (only marking the terminal dirty). The workspace remained stale with `focused_pane_id` pointing to the right pane.

### 3. Arc recreated each render cycle (drag bug)

The `pending_drag_init` Arc bridged between the divider's plain `Fn` closure (in pane_tree render) and the workspace's `cx.listener` drag handler. It was created inside the workspace's `render()` method, so every render cycle produced a new Arc. The divider's `on_mouse_down` wrote to frame N's Arc. The workspace's `on_mouse_move` on frame N+1 read from a new empty Arc.

## Fix applied

1. **Layout:** Eliminated double wrapping. Leaf returns `div().size_full().border_1().child(terminal)`. Split handles all sizing with direction-aware children matching Zed's pattern. Container uses `div().flex().size_full()`.

2. **Focus:** Added `FocusChanged` event to `TerminalView`, emitted in `on_mouse_down`. Workspace subscribes and directly calls `pane_tree.focus(pane_id)` + re-asserts `terminal.focus_handle(cx).focus(window, cx)`. Added `pane_id_for_terminal()` to PaneTree for reverse lookup.

3. **Drag:** Moved `pending_drag_init` Arc to workspace struct field (persists across renders). Fixed `total_size` to subtract known chrome (agent panel width, tab bar + input bar height) instead of using raw window bounds.

## What we learned

- **GPUI entity independence is the #1 gotcha.** Child entity changes do NOT propagate to parent entities. Cross-entity communication requires events (`cx.emit` + `cx.subscribe_in`).
- **Read Zed's actual source for layout patterns.** Guessing at GPUI flex behavior wastes time. Zed's codebase has working examples for every common layout pattern.
- **`overflow_hidden()` placement matters.** On the terminal div (which uses `relative()` + absolute children), it previously caused blank panes. On the split sizing div (one level up), it works correctly for clipping overflow.
- **Don't add flex_grow + flex_basis together on split children.** flex_basis(ratio) alone with flex_shrink is sufficient. Adding flex_grow makes both children compete equally for extra space, potentially ignoring the ratio.
- **Arc bridges between plain closures and entity listeners must persist across render cycles.** Store them on the struct, not as locals in render().
