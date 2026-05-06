# Pane Drag Reorder / Pane-to-Tab Drag — Implementation Notes

**Date**: 2026-05-06
**Branch**: `wey-gu/pane-title-bar`

---

## Overview

Pane title dragging now uses the same high-level drag payload as tab dragging (`DraggedTab`), but pane-origin drags render their visible title preview from `ConWorkspace` instead of relying on GPUI's built-in drag-preview positioning.

The supported behaviors are:

1. Drag a pane title within the pane content area to preview and apply split-layout pane moves.
2. Drag a pane title into the horizontal tab strip to promote that pane into a new tab at a live-reordered slot.
3. While over the tab strip, render a ghost tab in the strip so pane-to-tab movement feels like normal Chrome-style tab drag.
4. While outside the tab strip, destroy the ghost tab and show only the pane split preview when a valid split target exists.

The important design decision is that **pane-origin drags do not use GPUI's visible drag preview**. GPUI still carries the drag payload and routes `on_drag_move` / `on_drop`, but Con draws its own small floating pane title overlay at the live cursor position.

---

## Why this design

GPUI positions drag previews from the original drag source hitbox using an internal cursor offset. For horizontal tabs this is fine because the source element is already tab-sized. For pane title bars it is not: the source can be the full pane width, so the preview visually appears to originate from the pane's left edge instead of centering under the cursor.

Earlier attempts to compensate inside `DraggedTab::render()` with `.absolute()` or `.relative()` still left the visual preview tied to GPUI's drag-preview root. The stable solution is:

- Keep the entire pane title bar draggable.
- Return a zero-size GPUI preview for `DraggedTabOrigin::Pane`.
- Render the visible pane title preview as a workspace overlay using `PaneTitleDragState.current_pos`.

This keeps the product behavior deterministic:

```text
floating_preview_left = cursor.x - preview_width / 2
floating_preview_top  = cursor.y - preview_height / 2
```

So the title preview is centered under the mouse no matter where the user starts dragging in the pane title bar.

---

## Key Types

### `DraggedTab`

Defined in `crates/con-app/src/sidebar.rs`.

`DraggedTab` is now the shared drag payload for:

- vertical sidebar session drag
- horizontal tab strip drag
- pane title drag

Relevant fields:

```rust
pub struct DraggedTab {
    pub session_id: u64,
    pub label: SharedString,
    pub icon: &'static str,
    pub origin: DraggedTabOrigin,
    pub preview_constraint: Option<DraggedTabPreviewConstraint>,
    pub pane_id: Option<usize>,
}
```

`origin` is the discriminator. Drop and drag-move handlers must branch by origin instead of inferring drag type from global GPUI drag state.

### `DraggedTabOrigin`

```rust
pub enum DraggedTabOrigin {
    Sidebar,
    HorizontalTabStrip,
    Pane,
}
```

Rules:

- `Sidebar`: affects only vertical sidebar reorder.
- `HorizontalTabStrip`: affects horizontal tab reorder.
- `Pane`: affects pane split preview and pane-to-tab promotion.

### `PaneTitleDragState`

Defined in `crates/con-app/src/workspace.rs`.

Tracks the visible workspace overlay and pane drop target:

```rust
struct PaneTitleDragState {
    title: SharedString,
    current_pos: Point<Pixels>,
    active: bool,
    target: Option<PaneDropTarget>,
}
```

`current_pos` is updated from pane-origin `on_drag_move` handlers and drives the floating title preview position.

### `PaneDropTarget`

```rust
enum PaneDropTarget {
    NewTab { slot: usize },
    Split(PaneSplitDropTarget),
}
```

- `Split(...)`: cursor is over a valid pane edge target.
- `NewTab { slot }`: cursor is over the horizontal tab strip and the pane would be promoted at `slot`.
- `None`: no actionable target; floating title may still follow the cursor.

---

## Drag Source: Pane Title Bar

Implemented in `crates/con-app/src/pane_tree.rs`.

The entire pane title bar is the drag source:

```rust
bar.on_drag(dragged, move |dragged: &DraggedTab, _offset, _window, cx| {
    cx.stop_propagation();
    cx.new(|_| dragged.clone())
})
```

The pane title drag payload uses:

```rust
DraggedTab {
    session_id,
    label: pane_title,
    icon: "phosphor/terminal.svg",
    origin: DraggedTabOrigin::Pane,
    preview_constraint: None,
    pane_id: Some(pane_id),
}
```

The visible pane-origin preview is rendered by the workspace overlay rather than GPUI's drag preview. The `_offset` argument from `on_drag` is intentionally ignored for pane drags because the overlay is positioned from live cursor coordinates.

---

## Visible Preview Rendering

### GPUI preview for pane-origin drags

Implemented in `DraggedTab::render()` in `crates/con-app/src/sidebar.rs`.

For `DraggedTabOrigin::Pane`, the GPUI drag preview intentionally renders as a zero-size element:

```rust
if self.origin == DraggedTabOrigin::Pane {
    div().size(px(0.0))
}
```

This prevents the built-in GPUI preview from appearing at a position tied to the wide pane title bar source.

### Workspace floating title overlay

Implemented near the root render path in `crates/con-app/src/workspace.rs`.

When `pane_title_drag.active` is true, workspace renders a small title-like overlay:

- width: `120px` (~12 characters)
- height: `28px`
- centered under cursor using `pane_drag_floating_preview_origin()`
- label: `PaneTitleDragState.title`

Core helper:

```rust
fn pane_drag_floating_preview_origin(
    cursor: Point<Pixels>,
    preview_size: Size<Pixels>,
) -> Point<Pixels> {
    point(
        cursor.x - preview_size.width / 2.0,
        cursor.y - preview_size.height / 2.0,
    )
}
```

When the pane is over the tab strip, the overlay can use tab-bar vertical clamping via `tab_drag_preview_origin(...)`. When outside the tab strip it uses direct cursor centering.

---

## Drag Move Routing

Pane-origin drags are handled in two places.

### Pane content area

In the terminal content area, `on_drag_move::<DraggedTab>` handles only:

```rust
event.drag(cx).origin == DraggedTabOrigin::Pane
```

It:

1. Reads `pane_id` from the drag payload.
2. Computes candidate split target from pane bounds.
3. Filters no-op moves with `PaneTree::is_noop_pane_move(...)`.
4. Updates `PaneTitleDragState.current_pos` every move.
5. Sets `PaneTitleDragState.target = Some(PaneDropTarget::Split(...))` when valid.
6. Clears split target when there is no valid split target.

This state drives the split preview overlay.

### Horizontal tab strip

The tab strip has a pane-origin `on_drag_move::<DraggedTab>` handler that:

1. Ignores non-pane origins.
2. Requires cursor to be inside the tab strip bounds.
3. Computes `drop_slot` from real tab bounds using `pane_title_drag_tab_slot(...)`.
4. Updates `tab_strip_drop_slot`.
5. Updates `PaneTitleDragState.current_pos`.
6. Sets `PaneTitleDragState.target = Some(PaneDropTarget::NewTab { slot })`.

This switches the visual mode from split preview to tab ghost insertion.

---

## Ghost Tab / Chrome-style Tab Strip Preview

When a pane-origin drag is inside the horizontal tab strip:

- `tab_strip_drop_slot` stores the visual insertion slot.
- the tab strip render path inserts a ghost tab at that slot.
- existing tabs shift around the ghost using the same render-order logic used by live tab reorder.

This makes pane-to-tab promotion feel like normal tab dragging:

```text
[ tab A ][ tab B ][ ghost pane ][ tab C ]
```

Moving the cursor left/right updates the slot and re-renders the ghost position. Moving out of the tab strip removes the ghost state.

Important: pane-origin ghost insertion must not be treated as a real tab reorder source. `DraggedTabOrigin::Pane` has no existing tab index to remove.

---

## Drop Behavior

### Drop on pane content

If `PaneTitleDragState.target == Some(PaneDropTarget::Split(target))`:

```rust
pane_tree.move_pane(
    pane_id,
    target.target_pane_id,
    target.direction,
    target.placement,
)
```

Then clear drag state and notify.

If there is no split target, dropping on pane content does nothing.

### Drop on tab strip

If `DraggedTabOrigin::Pane`, the tab strip drop handler promotes the pane into a new tab at the active slot:

```rust
detach_pane_to_new_tab_at_slot(pane_id, slot, window, cx)
```

Then clears:

- `tab_strip_drop_slot`
- `tab_drag_target`
- active dragged tab session guard
- pane drag state

### Drop of normal horizontal tab

Normal tab reorder still uses:

```rust
reorder_tab_by_id(dragged.session_id, to, cx)
```

but only for `DraggedTabOrigin::HorizontalTabStrip`.

---

## State Isolation Rules

Drag logic must not infer pane drag from `cx.has_active_drag()` alone. Always prefer `DraggedTabOrigin` when a drag payload is available.

Required invariants:

1. Sidebar drags do not affect horizontal tab strip state.
2. Horizontal tab drags set `active_dragged_tab_session_id` and may live-reorder tabs.
3. Pane drags do not set `active_dragged_tab_session_id`.
4. Pane drags update `PaneTitleDragState` and may update `tab_strip_drop_slot` only while inside the tab strip.
5. Split preview and tab ghost preview are mutually exclusive.
6. GPUI's visible pane-origin drag preview remains hidden; workspace owns the visible pane title overlay.

---

## Name / Label Handling

The pane drag label is captured from the active pane title at drag creation time and stored in `DraggedTab.label` / `PaneTitleDragState.title`.

The floating overlay and split preview use this title directly. This avoids showing a hard-coded `Terminal` label while dragging.

When promoting a pane to a new tab, the created tab should preserve a useful label from the pane/terminal. If the terminal title is temporarily empty, fallback should be the captured pane drag label rather than an empty string.

---

## Testing

### Unit tests

Relevant tests in `workspace.rs` cover:

- floating pane preview origin centers under cursor
- tab-like preview size
- pane-title drag active state
- tab strip preview active state
- pane split preview region math
- live tab reorder slot mapping
- pane-to-tab slot calculation

Key test:

```rust
pane_drag_floating_preview_origin_places_title_under_cursor
```

This verifies:

```text
origin = cursor - preview_size / 2
```

### Manual smoke tests

1. Split left/right, drag right pane title from the left side, center, and right side of its title bar. The floating title should stay centered under the cursor.
2. Drag a pane title over another pane edge. Split preview should appear only for valid edge targets.
3. Drop on a valid split preview. The pane should move to that split position.
4. Drag a pane title into the horizontal tab strip. A ghost tab should appear and move left/right with the cursor.
5. Move back out of the tab strip. The ghost tab should disappear.
6. Drop pane in the tab strip. The pane should become a new tab at the ghost slot.
7. Drag existing horizontal tabs. Normal Chrome-style tab reorder should still work.
8. Drag sidebar sessions. Horizontal tab strip state should not change.

---

## Known Follow-ups

- Consider splitting `tab_strip_drop_slot` into separate `tab_reorder_drop_slot` and `pane_to_tab_drop_slot` to make state ownership explicit.
- If pane-to-tab labels still become empty after promotion, thread the captured drag label into `detach_pane_to_new_tab_at_slot(...)` as an explicit fallback.
- Keep render-path debug logging out of the final branch; drag move events are high-frequency.

---

## Out of Scope

- Cross-window pane drag.
- Multi-pane selection drag.
- Animated split-layout transitions during pane drag.
- Dragging from terminal content; the pane title bar is the drag source.
