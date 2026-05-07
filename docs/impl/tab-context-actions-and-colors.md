# Tab Context Actions and Accent Colors — Implementation Notes

**Date**: 2026-05-07
**Branch**: `pane-improve`

---

## Overview

This document covers the implementation of:

1. **Live tab drag reorder** — position-based slot calculation that works even when the cursor bypasses intermediate tabs.
2. **Horizontal drag constraint** — the tab drag preview is locked to the tab bar's y-axis so the ghost tab cannot drift vertically.
3. **Tab context menu** — shared right-click menu for horizontal tab strip and vertical sidebar, with Rename, Duplicate, Close variants, and accent color selection.
4. **Tab accent colors** — per-tab color that tints the tab header, pane title bar, and sidebar pill/row.
5. **Full-layout tab duplication** — Duplicate Tab preserves the entire pane tree layout and each pane's CWD.
6. **Vertical sidebar drag improvements** — sidebar-origin and pane-origin drags are gated on full container bounds, preventing stale drop slots when the cursor leaves the sidebar.

---

## 1. Live Tab Drag Reorder

### Problem

The original slot calculation was per-tab: each tab element's `on_drag_move` updated `tab_strip_drop_slot` only when the cursor was directly over that tab. If the user dragged quickly and the cursor skipped over intermediate tabs, the slot was never updated and the reorder appeared stuck.

### Solution

A container-level `on_drag_move` handler on the tab strip root element calls `horizontal_tab_slot_from_bounds`, which scans the full `tab_strip_tab_bounds` array and finds the correct slot from the cursor's x position regardless of which individual tab element received the event.

```rust
pub(super) fn horizontal_tab_slot_from_bounds(
    cursor: Point<Pixels>,
    tab_bounds: &[Bounds<Pixels>],
    tab_count: usize,
) -> Option<usize>
```

- Returns `None` when `tab_bounds` is empty (first drag frame before prepaint).
- Returns `Some(slot)` where `slot` is `0..=tab_count`.
- Slot is in drop-slot space: slot `i` means "insert before visual tab `i`".
- Slot is clamped to `tab_count` to prevent out-of-bounds reorder.

The probe position used for slot calculation is the **overlay center** (`tab_drag_overlay_probe_position`), not the raw mouse position. This keeps the slot stable when the cursor moves vertically outside the tab bar while the locked preview stays inside it.

When `horizontal_tab_slot_from_bounds` returns `None` (cursor left valid tab bounds), the container handler clears `tab_strip_drop_slot` and `tab_drag_target` immediately so stale reorder state does not persist.

---

## 2. Horizontal Drag Constraint

### Problem

GPUI positions drag previews relative to the drag source element. For tab drags, the preview could drift vertically when the user moved the cursor below the tab bar, making the ghost tab appear in the terminal content area.

### Solution

`DraggedTabPreviewConstraint` is stored in the `DraggedTab` payload and used by `DraggedTab::render()` to compute a constrained origin:

```rust
pub struct DraggedTabPreviewConstraint {
    pub bar_top: Pixels,
    pub bar_height: Pixels,
    pub cursor_offset_x: Pixels,
    pub min_left: Pixels,
    pub max_left: Pixels,
}
```

The render function computes:

```rust
fn constrained_drag_preview_x_shift(constraint, mouse_x) -> Pixels {
    let clamped_x = mouse_x.clamp(constraint.min_left, constraint.max_left);
    clamped_x - constraint.cursor_offset_x - mouse_x
}
```

The y position is locked to `bar_top` regardless of cursor y. The x position follows the cursor but is clamped to `[min_left, max_left]` so the preview never overflows the tab strip horizontally.

The constraint is populated in `on_drag` using the cursor offset from the source tab element and the current tab bar geometry.

---

## 3. Tab Context Menu

### Shared builder

`crates/con-app/src/tab_context_menu.rs` provides a single `build_tab_context_menu` function used by both the horizontal tab strip and the vertical sidebar panel rows.

```rust
pub(crate) fn build_tab_context_menu(menu: PopupMenu, opts: TabMenuOptions) -> PopupMenu
```

`TabMenuOptions` carries orientation-specific callbacks as `Option<WinCb>`:

| Field | Horizontal | Vertical |
|---|---|---|
| `rename` | ✓ | ✓ |
| `duplicate` | ✓ | ✓ |
| `reset_name` | when user label set | when user label set |
| `move_up` | — | when index > 0 |
| `move_down` | — | when index < total-1 |
| `close_to_right` | when not last tab | — |
| `close_tab` | ✓ | ✓ |
| `close_others` | when > 1 tab | when > 1 tab |
| `set_color` | ✓ | ✓ |

### Color swatch row

The color picker is a single `PopupMenuItem::element` that renders all 9 swatches (No Color + 8 accent colors) in one `h_flex` row. Each swatch is a separate `div` with its own `on_mouse_down` handler.

A shared `Rc<Cell<u8>>` carries the pressed swatch index from `on_mouse_down` to the `ElementItem`'s `on_click` handler, which fires when the menu row is clicked and closes the menu:

```rust
let pressed: Rc<Cell<u8>> = Rc::new(Cell::new(u8::MAX)); // u8::MAX = sentinel "nothing pressed"
// on_mouse_down per swatch:
pressed_ref.set(idx_u8);
// on_click on the row:
if (idx as usize) < ACCENT_COLORS.len() {
    set_color(ACCENT_COLORS[idx], window, cx);
}
```

The selected swatch shows an opacity-based background ring (no `border_*` calls, per design guidelines). Unselected swatches have a transparent wrapper.

---

## 4. Tab Accent Colors

### Data model

`TabAccentColor` is defined in `con-core/src/session.rs`:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TabAccentColor {
    Red, Orange, Yellow, Green, Teal, Blue, Purple, Pink,
    /// Forward-compat catch-all: unknown tags from newer builds deserialize here.
    #[serde(other)]
    Unknown,
}
```

`#[serde(other)]` ensures that a session file written by a newer build (with additional colors) does not cause a deserialization failure when opened by an older build. Unknown colors render as a neutral green fallback.

`color: Option<TabAccentColor>` is stored on `TabState` (persisted) and `Tab` (runtime). It is serialized to `session.json` and restored on startup.

### Color utilities

`crates/con-app/src/tab_colors.rs` provides:

```rust
pub(crate) fn tab_accent_color_hsla(color: TabAccentColor, cx: &App) -> Hsla
pub(crate) fn active_tab_indicator_color() -> Hsla
```

`tab_accent_color_hsla` maps each variant to an `(h, s, lightness_light, lightness_dark)` tuple and selects the appropriate lightness based on `cx.theme().is_dark()`. This module is intentionally separate from `tab_context_menu.rs` so that `pane_tree.rs` can use it without depending on the menu module.

### Visual application

| Surface | Active / Focused | Inactive / Unfocused |
|---|---|---|
| Horizontal tab header | accent, alpha 0.35 | accent, alpha 0.12 |
| Horizontal tab hover | — | accent, alpha 0.18 |
| Vertical rail pill | accent, alpha 0.35 | accent, alpha 0.12 |
| Vertical panel row | accent, alpha 0.35 | accent, alpha 0.12 |
| Pane title bar | accent, alpha 0.35 | monochrome (no accent) |
| Active indicator dot | accent color | — |
| Active indicator dot (no color) | green `hsla(142°, 0.60, 0.42, 1.0)` | — |

The active indicator dot is shown only on the active/focused tab. Inactive tabs show no dot.

### Sync

`set_tab_color` calls `sync_sidebar(cx)` after updating `self.tabs[index].color` so the sidebar's `SessionEntry` is refreshed immediately without waiting for the next tab switch.

---

## 5. Full-Layout Tab Duplication

### Previous behavior

`on_sidebar_duplicate` and the horizontal `duplicate_tab` action both created a new `PaneTree::new(terminal)` from only the focused pane's CWD. Multi-pane layouts were lost.

### New behavior

Both paths now use `PaneTree::to_state` + `PaneTree::from_state`:

```rust
let layout = self.tabs[index].pane_tree.to_state(cx, false);
let focused_pane_id = Some(self.tabs[index].pane_tree.focused_pane_id());
let pane_tree = PaneTree::from_state(&layout, focused_pane_id, &mut make_terminal);
```

`to_state(cx, false)` serializes the full pane tree including split ratios, each pane's CWD, and the focused pane ID. `from_state` rebuilds the tree, spawning a fresh terminal in each pane's CWD.

The duplicated tab also copies:
- `user_label` — preserves any user-set tab name
- `color` — preserves the accent color
- `agent_routing` — preserves provider/model overrides

Conversation history, panel state, and shell history are intentionally not copied.

---

## 6. Vertical Sidebar Drag Bounds Gating

### Problem

GPUI bubbles `on_drag_move` to parent containers even when the cursor is outside their bounds. The sidebar's container-level handler was only gating pane-origin drags on `point_in_bounds`, not sidebar-origin drags. This meant a sidebar tab drag that moved horizontally out over the terminal area could still update `drop_slot` if the cursor's y coordinate aligned with a row.

### Fix

Both the rail container and the panel body `on_drag_move` handlers now gate on `point_in_bounds` for **all** drag origins, not just pane-origin:

```rust
// Before:
if is_pane && !point_in_bounds(&event.event.position, &event.bounds) { ... }

// After:
if !point_in_bounds(&event.event.position, &event.bounds) { ... }
```

When the cursor leaves the container bounds, `drop_slot` and `drag_preview` are cleared immediately and `cx.notify()` is called so the drop indicator disappears.

---

## Key Files

| File | Role |
|---|---|
| `crates/con-app/src/tab_colors.rs` | Color mapping utilities (new) |
| `crates/con-app/src/tab_context_menu.rs` | Shared context menu builder (new) |
| `crates/con-app/src/workspace/helpers.rs` | `horizontal_tab_slot_from_bounds`, `tab_drag_overlay_probe_position` |
| `crates/con-app/src/workspace/render/top_bar.rs` | Horizontal tab strip rendering, drag constraint, color dot |
| `crates/con-app/src/workspace/render.rs` | Container drag-move handler, stale slot clearing |
| `crates/con-app/src/workspace/tab_actions.rs` | `duplicate_tab`, `close_other_tabs`, `close_tabs_to_right`, `set_tab_color` |
| `crates/con-app/src/workspace/sidebar_settings.rs` | `on_sidebar_duplicate` (updated), `on_sidebar_set_color` |
| `crates/con-app/src/sidebar.rs` | Vertical drag bounds gating, accent color rendering |
| `crates/con-app/src/pane_tree.rs` | `tab_accent_color` propagation to pane title bar |
| `crates/con-core/src/session.rs` | `TabAccentColor` enum, `TabState.color` field |

---

## Testing

### Unit tests

Relevant tests in `crates/con-app/src/workspace/tests.rs`:

- `horizontal_tab_slot_from_bounds` — slot from cursor x, clamped to tab_count
- `horizontal_tab_reorder_probe_uses_locked_overlay_center_not_mouse_y` — probe uses overlay center
- `dragged_tab_preview_x_shift_clamps_inside_tab_bar` — x constraint clamping
- `dragged_tab_preview_y_shift_locks_to_source_tab_top` — y locked to bar top
- `tab_drag_source_is_hidden_only_for_active_dragged_session` — source tab hidden during drag

Relevant tests in `crates/con-app/src/sidebar.rs`:

- `vertical_slot_from_bounds_uses_row_midpoints` — slot from cursor y
- `vertical_drag_overlay_probe_locks_x_and_uses_overlay_center` — probe position

### Manual smoke tests

1. Drag a tab quickly past multiple tabs — slot should update correctly without getting stuck.
2. Drag a tab downward out of the tab bar — the ghost tab should stay locked to the tab bar y position.
3. Right-click a tab — context menu should show Rename, Duplicate Tab, Close Tab, Close Other Tabs, Close Tabs to the Right, and a row of color swatches.
4. Select a color — the tab header should immediately change color; the pane title bar should also reflect the color when focused.
5. Duplicate a multi-pane tab — the new tab should have the same split layout and each pane should open in the correct CWD.
6. In vertical layout, drag a tab sideways out of the sidebar — the drop indicator should disappear as soon as the cursor leaves the sidebar bounds.
7. In vertical layout, right-click a tab — same context menu should appear with Move Up / Move Down instead of Close Tabs to the Right.

---

## Known Follow-ups

- Tab-to-tab split merge (drag one tab onto another to merge pane trees) — design spec in `docs/impl/tab-drag-layout-preview.md`.
- Vertical sidebar drag-to-split (drag a sidebar tab onto the pane content area to split into a new pane).
- Animated tab reorder transitions.
