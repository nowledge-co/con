# Tab Context Actions

PR: [#162](https://github.com/nowledge-co/con-terminal/pull/162) — `feat: improve tab drag interactions and context actions`

This document covers four related improvements shipped together:

1. Tab live-reorder drag — horizontal and vertical
2. Tab context menu — shared across orientations
3. Tab accent colors — persistent, rendered in tab chrome and pane title bars
4. Duplicate tab — full pane layout preservation

---

## Tab Live-Reorder Drag

### Problem

The previous horizontal tab drag used per-tab `on_drag_move` handlers to compute the drop slot. If the user dragged a tab without crossing its neighbors (e.g. dragging tab 3 to position 1 by going around the tab strip via the window chrome), the slot never updated because no intermediate tab's handler fired.

Vertical tab drag had no live-reorder at all.

### Solution

**Horizontal.** A container-level `on_drag_move` handler runs on every frame and calls `horizontal_tab_slot_from_bounds`, which computes the drop slot from the full `tab_strip_tab_bounds` array using cursor x vs. each tab's midpoint. This is position-based, not event-based, so it works regardless of the drag path.

The drag preview (ghost tab) is locked to the tab bar's y-axis. The preview x position is computed from the cursor's x offset within the source tab so the ghost tracks the cursor horizontally while staying pinned to the bar height. When the cursor leaves valid tab bounds, the stale drop target is cleared immediately.

**Vertical.** The sidebar rail and pinned-panel body use the same midpoint approach via `vertical_slot_from_bounds`. Both container-level and per-row `on_drag_move` handlers gate on `point_in_bounds` for all drag origins (sidebar and pane), so dragging outside the sidebar clears the drop indicator rather than keeping a stale slot active.

### Key helpers

```text
crates/con-app/src/workspace/helpers.rs
  horizontal_tab_slot_from_bounds(cursor, tab_bounds, tab_count) -> Option<usize>
    Position-based slot from full bounds array. Returns None when bounds is empty
    (first drag frame before prepaint). Slot is clamped to tab_count.

  tab_drag_overlay_origin(mouse, preview, min_left, max_left) -> Point<Pixels>
    Computes the ghost preview's top-left, locking y to the tab bar and
    clamping x so the preview stays inside the bar.

  tab_drag_overlay_probe_position(...) -> Point<Pixels>
    Returns the center of the ghost preview — used as the authoritative
    cursor position for slot computation so the slot tracks the ghost,
    not the raw mouse y.

crates/con-app/src/sidebar.rs
  vertical_slot_from_bounds(probe_y, bounds, session_count) -> Option<usize>
    Midpoint-based slot from tab row bounds array.
```

### Drag preview overlay

The ghost preview is rendered as an `absolute` overlay appended to the workspace `main_area` (a `.relative()` container), not inside the tab strip or sidebar. This ensures it stacks above the Metal NSView on macOS and above the terminal pane on all platforms.

The preview is cleared on `on_drop`, on `on_mouse_up` (mouseup outside any drop target), and whenever `has_active_drag` returns false during render.

---

## Tab Context Menu

### Overview

Both the horizontal tab strip and the vertical sidebar panel rows share a single context menu builder. Orientation-specific items are passed as `Option<WinCb>` fields so the builder stays generic.

```text
crates/con-app/src/tab_context_menu.rs
  TabMenuOptions { rename, duplicate, reset_name, move_up, move_down,
                   close_to_right, close_tab, close_others,
                   set_color, current_color }
  build_tab_context_menu(menu: PopupMenu, opts: TabMenuOptions) -> PopupMenu
```

### Menu items

| Item | Horizontal | Vertical |
|---|---|---|
| Rename | ✓ | ✓ |
| Duplicate Tab | ✓ | ✓ |
| Reset Name | when user_label set | when user_label set |
| Move Up | — | when index > 0 |
| Move Down | — | when index < total−1 |
| Close Tab | ✓ | ✓ |
| Close Other Tabs | when tabs > 1 | when tabs > 1 |
| Close Tabs to the Right | when not last tab | — |
| Tab Color | ✓ | ✓ |

### Stable target resolution

Menu actions must not retain a tab index as their authority. A context menu can
stay open while another tab before it closes, for example when a shell exits and
the workspace removes that tab. If callbacks keep the old index, destructive
actions such as Close Tab, Close Other Tabs, or Set Color can hit the tab that
shifted into that slot.

Both horizontal and vertical tab menus therefore capture the tab's stable
`summary_id` and resolve it back to the current index at click time. If the tab
has already been closed, the action is a no-op.

### Color swatch row

The color picker is a single `PopupMenuItem::element` that renders all 9 swatches (No Color + 8 accent colors) in a horizontal `h_flex` row. Each swatch is a `div` with `.id(ElementId::Integer(idx))` and an `on_mouse_down` handler that writes the chosen index into a shared `Rc<Cell<u8>>`. The `ElementItem`'s `on_click` reads the cell and calls `set_color`. The sentinel value `u8::MAX` means "nothing pressed yet".

Selected swatch is indicated by an opacity-based background on the wrapper div (no borders, per design language).

---

## Tab Accent Colors

### Data model

```text
crates/con-core/src/session.rs
  pub enum TabAccentColor {
      Red, Orange, Yellow, Green, Teal, Blue, Purple, Pink,
      #[serde(other)]
      Unknown,   // forward-compat catch-all for future colors
  }

crates/con-core/src/session.rs
  TabState.color: Option<TabAccentColor>   // persisted in session.json
```

`Unknown` is a `#[serde(other)]` catch-all so a session file written by a newer build (with additional colors) does not fail to deserialize on an older build — the unknown color renders as a neutral green fallback.

### Color helpers

```text
crates/con-app/src/tab_colors.rs
  tab_accent_color_hsla(color, cx) -> Hsla
    Maps TabAccentColor to an Hsla. Light/dark variants are separate
    lightness values (ll / ld) selected from cx.theme().is_dark().

  active_tab_indicator_color() -> Hsla
    Green dot used when a tab is active but has no accent color set.
    Centralised here so horizontal strip, rail pill, and panel row
    all use the same value.
```

### Rendering

**Horizontal tab strip** (`top_bar.rs`):
- Active tab: accent color at alpha 0.35 replaces `theme.background.opacity(elevated_ui_surface_opacity)`.
- Inactive tab: accent color at alpha 0.12; hover at alpha 0.18.
- Active indicator dot: accent color if set, otherwise `active_tab_indicator_color()`. Only shown on the active tab.

**Vertical rail pill** (`sidebar.rs`):
- Active pill: accent color at alpha 0.35 replaces `elevated_surface(theme, opacity)`.
- Inactive pill: accent color at alpha 0.12; hover at alpha 0.20.
- Active indicator dot: same logic as horizontal, bottom-right corner of pill.

**Vertical panel row** (`sidebar.rs`):
- Same active/inactive/hover alpha pattern as rail pill.
- Active indicator dot: bottom-right of icon stack.

**Pane title bar** (`pane_tree.rs`):
- Focused pane: accent color at alpha 0.35 replaces `theme.tab_bar_segmented`.
- Unfocused pane: always `theme.tab_bar_segmented.opacity(0.78)` — accent is reserved for the focused/active semantic state only.

### Persistence

`set_tab_color(index, color, cx)` in `tab_actions.rs`:
1. Updates `self.tabs[index].color`.
2. Calls `sync_sidebar(cx)` so the sidebar's `SessionEntry.color` is updated immediately (without this, the color only appears after switching tabs).
3. Calls `save_session(cx)` to persist to `session.json`.
4. Calls `cx.notify()`.

Session save/restore round-trips through `TabState.color` in `session.rs` and `session_state.rs`.

---

## Duplicate Tab

### Previous behavior

`on_sidebar_duplicate` and the horizontal `duplicate_tab` action both created `PaneTree::new(terminal)` from only the focused pane's CWD. Multi-pane layouts were lost.

### New behavior

Both paths now use `PaneTree::to_state` + `PaneTree::from_state`:

```rust
let layout = self.tabs[index].pane_tree.to_state(cx, false);
let focused_pane_id = Some(self.tabs[index].pane_tree.focused_pane_id());
let pane_tree = PaneTree::from_state(&layout, focused_pane_id, &mut make_terminal);
```

`to_state(cx, false)` serializes the full pane tree including split ratios, each pane's CWD, and surface metadata. `from_state` rebuilds the tree, spawning a fresh terminal in each pane's CWD. The `false` flag skips screen text capture (no need to copy scrollback into the duplicate).

Copied fields: `pane_tree` (full layout), `user_label`, `color`, `agent_routing`.
Not copied: `ai_label`, `ai_icon`, `session` (conversation), `panel_state`, `shell_history`.

The new tab is inserted at `index + 1` and immediately activated.

---

## Code map

```text
crates/con-core/src/session.rs
  + TabAccentColor enum with #[serde(other)] Unknown variant
  + TabState.color: Option<TabAccentColor>

crates/con-app/src/tab_colors.rs          (new file)
  + tab_accent_color_hsla()
  + active_tab_indicator_color()

crates/con-app/src/tab_context_menu.rs    (new file)
  + TabMenuOptions struct
  + build_tab_context_menu()

crates/con-app/src/workspace/tab_actions.rs
  + duplicate_tab()     — full pane tree copy
  + duplicate_tab_by_id(), close_*_by_id(), set_tab_color_by_id()
    stable-id wrappers for menu actions
  + close_other_tabs()  — with index bounds guard
  + close_tabs_to_right()
  + set_tab_color()     — calls sync_sidebar before save

crates/con-app/src/workspace/window_actions.rs
  + close_tab_by_id() — stable-id wrapper for menu actions

crates/con-app/src/workspace/sidebar_settings.rs
  + begin_tab_rename_by_id() — stable-id wrapper for menu actions

crates/con-app/src/workspace/helpers.rs
  + horizontal_tab_slot_from_bounds()
  + tab_drag_overlay_origin()
  + tab_drag_overlay_probe_position()

crates/con-app/src/workspace/render.rs
  ~ container on_drag_move: position-based slot via horizontal_tab_slot_from_bounds
  ~ clears stale drop target when cursor leaves valid tab bounds
  ~ passes tab_accent_color to pane_tree.render()

crates/con-app/src/workspace/render/top_bar.rs
  ~ tab bg uses accent color (active 0.35 / inactive 0.12 / hover 0.18)
  ~ active indicator dot: accent or green fallback
  ~ context menu via build_tab_context_menu()

crates/con-app/src/pane_tree.rs
  ~ render() / render_node() / render_zoomed_leaf() / render_leaf()
    all accept tab_accent_color: Option<TabAccentColor>
  ~ render_pane_title_bar(): accent bg on focused pane only

crates/con-app/src/sidebar.rs
  ~ rail pill and panel row: accent bg + indicator dot
  ~ container on_drag_move: point_in_bounds guard for all origins
  ~ panel body on_drag_move: same guard
  + SidebarPaneToTab event (pane drag → new tab)
  + SidebarSetColor event (color picker → workspace)
  ~ force_clear_drag_state: only notifies when state actually changed

crates/con-app/src/workspace/sidebar_settings.rs
  ~ on_sidebar_duplicate: delegates to duplicate_tab() so both orientations
    preserve the full pane tree, copy color, and insert next to the source tab
  + on_sidebar_set_color handler
```

---

## Testing

All changes are covered by the existing test suite (`cargo test --workspace`). Key tests added or updated:

- `horizontal_tab_slot_from_bounds` — slot from bounds array, clamp, empty bounds returns None
- `tab_drag_preview_origin_*` — y lock, x clamp, cursor-at-center
- `horizontal_tab_reorder_probe_uses_locked_overlay_center_not_mouse_y` — probe uses ghost center not raw mouse
- `vertical_slot_from_bounds_uses_row_midpoints` — midpoint logic
- `vertical_drag_overlay_probe_locks_x_and_uses_overlay_center` — x lock

---

## Not in scope

- Tab-to-tab split merge (drag a tab onto another tab to merge pane trees). Design spec: `docs/impl/tab-drag-layout-preview.md`.
- Tab groups / pinned tabs.
- Animated color transitions on tab background change.
