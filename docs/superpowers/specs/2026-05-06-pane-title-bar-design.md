# Pane Title Bar — Design Spec

**Date**: 2026-05-06  
**Branch**: `wey-gu/pane-title-bar`

---

## Overview

Add a persistent title bar to every pane when there are 2+ panes in the split tree. The bar shows the pane title, an options menu (maximize / minimize), a close button, and supports drag-to-tab promotion.

---

## Scope

- **Only shown when `pane_count > 1`** — single-pane layout has no title bar.
- The existing surface strip (shown when `surfaces.len() > 1`) is preserved and sits below the title bar.
- No title editing in this iteration.

---

## Visual Design

```
┌─────────────────────────────────────────────┐
│ [⋮]      sundy@m1max: ~/work/con       [✕] │  ← title bar, h=28px
├─────────────────────────────────────────────┤
│  [surface strip — only when surfaces > 1]   │  ← existing, h=28px
├─────────────────────────────────────────────┤
│                                             │
│            terminal content                 │
│                                             │
└─────────────────────────────────────────────┘
```

### Metrics

| Property | Value |
|---|---|
| Height | `px(28)` |
| Background (focused) | `theme.tab_bar_segmented` |
| Background (unfocused) | `theme.tab_bar_segmented.opacity(0.78)` |
| Title font | `theme.font_family`, `px(12)`, `FontWeight::MEDIUM` |
| Title color (focused) | `theme.foreground.opacity(0.72)` |
| Title color (unfocused) | `theme.foreground.opacity(0.52)` |
| Button size | `px(20)` × `px(20)` |
| Button icon size | `px(11)` |
| Button hover bg | `theme.foreground.opacity(0.08)` |
| Button rounding | `px(4)` |
| Button icon color | `theme.foreground.opacity(0.52)` |

### Icons (Phosphor)

- Options menu: `phosphor/dots-three-vertical.svg`
- Close: `phosphor/x.svg`
- Maximize: `phosphor/arrows-out-simple.svg`
- Minimize (restore): `phosphor/arrows-in-simple.svg`

### Layout

```
[px(6) pad] [⋮ btn 20px] [flex-1 centered title] [✕ btn 20px] [px(6) pad]
```

- Title is centered in the remaining space using `flex-1` + `text_align(center)` (or `justify_center` on a flex row).
- Close button is hidden (`display: none` equivalent — just omit from render) when `pane_count == 1`.

---

## Options Menu

Clicking `⋮` opens a `context_menu` (gpui-component `ContextMenuExt`) with two items:

| Item | Condition | Action |
|---|---|---|
| **Maximize** | always shown; label changes to **Restore** when already zoomed | `toggle_zoom_cb(pane_id)` |
| **Close Pane** | only when `pane_count > 1` | `close_pane_cb(pane_id)` |

"Maximize" maps to the existing `toggle_zoom_focused` / `TogglePaneZoom` path.  
"Close Pane" maps to the existing `close_pane_in_tab` path.

---

## Close Button

- Calls `close_pane_cb(pane_id, window, cx)`.
- Hidden when `pane_count == 1` (no split to close into).
- Uses `window.prevent_default()` + `cx.stop_propagation()` on mouse-down, same pattern as the surface tab close button.

---

## Drag-to-Tab

### Interaction

1. User presses mouse-down on the title bar area.
2. User drags upward toward the window's top bar (tab strip / title bar area).
3. When the cursor enters the top-bar zone (y < `TOP_BAR_HEIGHT` from window top), a **drop indicator** appears in the tab strip — a highlighted insertion slot showing "drop here to create tab".
4. On mouse-up inside the top-bar zone: the pane is detached and promoted to a new tab.
5. On mouse-up outside the top-bar zone (or Escape): drag is cancelled, pane stays.

### State

A new `PaneDragState` stored in `PaneTree`:

```rust
pub struct PaneTitleDragState {
    pub pane_id: PaneId,
    pub start_pos: Point<Pixels>,   // where drag began
    pub active: bool,               // threshold crossed (>8px movement)
}
```

### Threshold

Drag becomes "active" (shows indicator) after cursor moves more than `8px` from start position in any direction. This prevents accidental drags on click.

### Drop indicator

Rendered in the workspace top bar (tab strip area) as a semi-transparent insertion marker — a vertical bar or highlighted tab slot at the right end of the tab strip. Uses `theme.accent_color` or `theme.foreground.opacity(0.20)` fill.

### Promotion logic — `detach_pane_to_new_tab(pane_id, window, cx)`

New method on `ConWorkspace`:

1. Collect all surface terminals for `pane_id` from the active tab's `pane_tree`.
2. Call `close_pane_in_tab(active_tab, pane_id, window, cx)` — removes the pane from the split tree.
3. Create a new tab using the first terminal from step 1 as the root pane.
4. If the pane had multiple surfaces, restore them into the new tab's pane tree via `create_surface_in_pane`.
5. Switch to the new tab.

---

## Callback additions to `PaneTree::render()`

Two new callbacks added alongside the existing ones:

```rust
pub fn render(
    &self,
    begin_drag_cb: impl Fn(SplitId, f32) + 'static,
    focus_surface_cb: impl Fn(SurfaceId, &mut Window, &mut App) + 'static,
    rename_surface_cb: impl Fn(SurfaceId, &mut Window, &mut App) + 'static,
    close_surface_cb: impl Fn(SurfaceId, &mut Window, &mut App) + 'static,
    // NEW:
    close_pane_cb: impl Fn(PaneId, &mut Window, &mut App) + 'static,
    toggle_zoom_cb: impl Fn(PaneId, &mut Window, &mut App) + 'static,
    begin_pane_title_drag_cb: impl Fn(PaneId, Point<Pixels>) + 'static,
    rename_editor: Option<SurfaceRenameEditor>,
    divider_color: Hsla,
    has_splits: bool,   // passed in from workspace (pane_count > 1)
    cx: &App,
) -> AnyElement
```

`has_splits` controls whether the title bar is rendered at all.

---

## Files Changed

| File | Change |
|---|---|
| `crates/con-app/src/pane_tree.rs` | Add `render_pane_title_bar()`, update `render_leaf()`, update `render()` / `render_node()` / `render_zoomed_leaf()` signatures, add `PaneTitleDragState` |
| `crates/con-app/src/workspace.rs` | Wire new callbacks in `pane_tree.render(...)`, add `detach_pane_to_new_tab()`, add drag state tracking, render drop indicator in top bar |

---

## Out of Scope

- Title editing from the title bar (double-click rename) — use existing surface rename flow.
- Drag pane to a different existing tab (only "new tab" drop target for now).
- Drag between windows.
