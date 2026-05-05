# Pane Title Bar — Implementation Plan

**Spec**: `docs/superpowers/specs/2026-05-06-pane-title-bar-design.md`  
**Branch**: `wey-gu/pane-title-bar`

---

## Task 1: Add title bar rendering to `pane_tree.rs`

**File**: `crates/con-app/src/pane_tree.rs`

### Steps

1. Add three new callback type parameters to `PaneTree::render()`, `render_node()`, and `render_zoomed_leaf()`:
   - `close_pane_cb: Arc<dyn Fn(PaneId, &mut Window, &mut App) + 'static>`
   - `toggle_zoom_cb: Arc<dyn Fn(PaneId, &mut Window, &mut App) + 'static>`
   - `begin_pane_title_drag_cb: Arc<dyn Fn(PaneId, Point<Pixels>) + 'static>`
   - `has_splits: bool`
   - `is_zoomed: bool` (whether this pane is currently the zoomed pane — for the menu label)

2. Add a private `render_pane_title_bar()` function:

```rust
fn render_pane_title_bar(
    pane_id: PaneId,
    title: String,
    is_focused: bool,
    has_splits: bool,
    is_zoomed: bool,
    close_pane_cb: Arc<dyn Fn(PaneId, &mut Window, &mut App) + 'static>,
    toggle_zoom_cb: Arc<dyn Fn(PaneId, &mut Window, &mut App) + 'static>,
    begin_pane_title_drag_cb: Arc<dyn Fn(PaneId, Point<Pixels>) + 'static>,
    cx: &App,
) -> AnyElement
```

   Layout: `h(px(28))`, `flex()`, `items_center()`, `px(px(6))`, `bg(title_bar_bg)`

   - Left: `⋮` button (`dots-three-vertical.svg`, `size(px(20))`) with `context_menu`:
     - "Maximize" / "Restore" item → calls `toggle_zoom_cb(pane_id)`
     - "Close Pane" item (only when `has_splits`) → calls `close_pane_cb(pane_id)`
   - Center: `div().flex_1().flex().justify_center()` with title text
   - Right: `✕` button (`x.svg`, `size(px(20))`, only when `has_splits`) → calls `close_pane_cb(pane_id)`
   - Whole bar: `on_mouse_down` records drag start via `begin_pane_title_drag_cb(pane_id, event.position)`

3. In `render_leaf()`: when `has_splits` is true, prepend the title bar above the existing surface strip + terminal.

   Title text = active surface title → `terminal.title(cx)` → fallback `"Terminal"`.

4. Thread all new callbacks through `render_node()` and `render_zoomed_leaf()` (same Arc-clone pattern as existing callbacks).

### Verification
- `cargo build -p con` compiles without errors.

---

## Task 2: Wire new callbacks in `workspace.rs` + add `detach_pane_to_new_tab()`

**File**: `crates/con-app/src/workspace.rs`

### Steps

1. Add `pane_title_drag: Option<PaneTitleDragState>` field to `ConWorkspace`:

```rust
struct PaneTitleDragState {
    pane_id: PaneId,
    start_pos: Point<Pixels>,
    active: bool,  // threshold (8px) crossed
}
```

2. In the `pane_tree.render(...)` call site (~line 10266), add the three new callbacks:

   **close_pane_cb**: calls `self.close_pane_in_tab(self.active_tab, pane_id, window, cx)` then `cx.notify()`.

   **toggle_zoom_cb**: focuses the pane first (`self.tabs[self.active_tab].pane_tree.focus(pane_id)`), then dispatches `TogglePaneZoom` action equivalent (call `self.toggle_pane_zoom_for_pane(pane_id, window, cx)` — new helper).

   **begin_pane_title_drag_cb**: sets `self.pane_title_drag = Some(PaneTitleDragState { pane_id, start_pos: pos, active: false })`.

3. Add `has_splits` and `is_zoomed` arguments — derive from `pane_count > 1` and `zoomed_pane_id`.

4. Add `on_mouse_move` handler on the terminal area container to:
   - If `pane_title_drag` is `Some` and not yet active: check if distance from start > 8px → set `active = true`, `cx.notify()`.
   - If active: update `tab_strip_drop_slot` to `Some(tab_count)` when cursor y < `current_top_bar_height()`, else `None`.

5. Add `on_mouse_up` handler on the terminal area container:
   - If `pane_title_drag` is `Some` and `active` and cursor y < `current_top_bar_height()`: call `self.detach_pane_to_new_tab(pane_id, window, cx)`.
   - Always clear `pane_title_drag = None` and `tab_strip_drop_slot = None`.

6. Add `detach_pane_to_new_tab(pane_id, window, cx)`:
   - Collect `surface_infos` for `pane_id` from active tab.
   - Collect terminals (active surface first).
   - Call `close_pane_in_tab(active_tab, pane_id, window, cx)`.
   - Create new tab with the first terminal (reuse `new_tab` logic but with existing terminal).
   - For remaining surfaces: call `create_surface_in_pane` on the new tab's pane tree.
   - Activate the new tab.

7. Add `toggle_pane_zoom_for_pane(pane_id, window, cx)` helper:
   - `self.tabs[self.active_tab].pane_tree.focus(pane_id)`
   - Then same body as `toggle_pane_zoom()`.

### Verification
- `cargo build -p con` compiles without errors.
- Manual smoke test: split pane, title bar appears, maximize/restore works, close works, drag to top creates new tab.

---

## Task 3: Add missing Phosphor icons

**Files**: `assets/icons/phosphor/`

Check which icons are needed and not yet present:
- `dots-three-vertical.svg` — already present ✓
- `x.svg` — already present ✓  
- `arrows-out-simple.svg` — already present ✓
- `arrows-in-simple.svg` — already present ✓

No action needed if all present. If any missing, copy from `3pp/phosphor-icons/SVGs/regular/`.

---

## Notes

- The `PaneTitleDragState` is stored on `ConWorkspace`, not `PaneTree` — this keeps drag lifecycle in the workspace where mouse events are handled globally.
- The drop indicator reuses the existing `tab_strip_drop_slot` mechanism — when a pane title drag is active and cursor is in the top bar zone, set `tab_strip_drop_slot = Some(tab_count)` to show the "append" indicator at the right end of the tab strip.
- `detach_pane_to_new_tab` must handle the edge case where `close_pane_in_tab` returns false (only 1 pane) — in that case, do nothing.
