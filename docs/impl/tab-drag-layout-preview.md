# Tab Drag Layout Preview — Design Spec

**Date**: 2026-05-06
**Branch**: `wey-gu/pane-title-bar`

---

## Overview

This is a forward-looking design note for a tab-to-tab split-merge workflow. It is not part of the current pane-title drag implementation.

When dragging a horizontal tab over another tab, Con could preview the layout that will result if the dragged tab is dropped into the target tab as a split. The preview starts only when the cursor is in a non-center edge zone of another tab.

The proposed visual direction is **Live split preview**: the target tab content area visually splits in-place, with the dragged tab layout tinted differently from the target tab layout. On drop, the dragged tab's entire pane layout is merged into the target tab and the dragged tab is removed from the tab strip.

---

## Goals

- Dragging a tab over another tab's edge shows a live split-layout preview.
- The preview distinguishes dragged content from existing target content using subtle accent tinting.
- Dropping on an edge merges the dragged tab's whole pane tree into the target tab.
- Existing tab reorder behavior remains available when dragging through the tab center/reorder zones.
- Preview and final drop behavior match.

---

## Non-goals

- Cross-window tab merging.
- Rendering live terminal snapshots inside the preview.
- Animating the pane tree during hover beyond immediate preview state updates.
- Merging only the focused pane from the dragged tab.
- Supporting vertical sidebar tab drag-to-split in this iteration.

---

## Interaction Model

### Drag source

The drag source is the existing horizontal tab drag (`DraggedTab`). This feature applies to the horizontal tab strip.

### Target zones

When the dragged tab is over another tab item, resolve a target zone from that tab item's bounds:

- Top 25% → split vertically, dragged tab before target layout.
- Bottom 25% → split vertically, dragged tab after target layout.
- Left 25% → split horizontally, dragged tab before target layout.
- Right 25% → split horizontally, dragged tab after target layout.
- Center → no split target; retain existing reorder behavior.

If zones overlap at corners, pick the nearest edge by normalized distance.

Dragging over the source tab never creates a split target.

### Preview behavior

When an edge split target is active:

- Hide the tab-strip reorder slot indicator.
- Render a live split preview over the workspace content area.
- The preview should make the final result clear:
  - Target tab layout remains visible in its future region.
  - Dragged tab layout is represented in the incoming region with accent tint.
  - A subtle split seam separates the two regions.

When the cursor leaves an edge zone:

- Clear the split preview.
- Resume existing reorder indicator behavior if the cursor is in a reorder slot.

### Drop behavior

On mouse release / drop:

- If a split target is active:
  - Remove the dragged tab from `tabs`.
  - Merge its entire `PaneTree` into the target tab's `PaneTree`.
  - Remove the dragged tab from the tab strip.
  - Make the target tab active after index remapping.
  - Sync native view visibility, focus state, input bar pane list, and session state.
- If no split target is active:
  - Preserve existing tab reorder behavior.

---

## Visual Design

The preview follows Con's existing design language:

- No shadows.
- Avoid heavy borders.
- Use opacity-based fills and semantic accent color.
- Keep typography compact and mono for pane/tab labels.

Preview layers:

1. **Target region**
   - Existing terminal content remains visible.
   - Apply a very light neutral overlay if needed for contrast.

2. **Incoming dragged-tab region**
   - Accent tint, e.g. `theme.primary.opacity(0.14..0.20)`.
   - Render a skeleton label using the dragged tab display label.
   - If practical, represent multiple panes as nested skeleton rectangles; otherwise start with a single labeled region.

3. **Split seam**
   - 1–2px accent line between regions.

The preview should render above terminal content and below modal overlays.

---

## Architecture

The remainder of this document is a forward-looking design for tab-to-tab split merge. It is not part of the pane-title drag-to-tab implementation that currently ships on this branch. The current production `TabDragTarget` is reorder-only, and the merge APIs below are proposed interfaces for a follow-up PR.

### Drag target state

Proposed broader drag target shape:

```rust
enum TabDragTarget {
    Reorder { slot: usize },
    Split(TabSplitDropTarget),
}

struct TabSplitDropTarget {
    dragged_tab_index: usize,
    target_tab_index: usize,
    direction: SplitDirection,
    placement: SplitPlacement,
    tab_bounds: Bounds<Pixels>,
}
```

`tab_strip_drop_slot` can remain for compatibility, but split preview state should be separate or derived from `TabDragTarget`.

### Hit testing

Add a pure helper for tab edge resolution:

```rust
fn tab_split_drop_target_from_position(
    dragged_tab_index: usize,
    target_tab_index: usize,
    cursor: Point<Pixels>,
    bounds: Bounds<Pixels>,
) -> Option<TabSplitDropTarget>
```

This mirrors the pane title drag four-edge helper.

### PaneTree merge API

Proposed layout-level API to merge a whole incoming tree:

```rust
pub fn merge_tree(
    &mut self,
    incoming: PaneTree,
    direction: SplitDirection,
    placement: SplitPlacement,
)
```

Rules:

- Preserve both existing layouts as subtrees.
- Insert the incoming tree before/after based on placement.
- Normalize pane IDs and split IDs so they remain unique in the merged tree.
- Clear zoom state.
- Focus the incoming tree's focused pane after merge, unless UX review suggests focusing the target tree instead.

### Workspace tab merge API

Proposed workspace-level operation:

```rust
fn merge_tab_into_tab(
    &mut self,
    from_index: usize,
    to_index: usize,
    direction: SplitDirection,
    placement: SplitPlacement,
    window: &mut Window,
    cx: &mut Context<Self>,
)
```

Responsibilities:

- Validate indices and ignore self-merge.
- Remove source tab.
- Adjust target index if source tab came before target tab.
- Call `PaneTree::merge_tree`.
- Set active tab to the merged target.
- Sync input bar panes, terminal focus states, native view visibility, tab summaries, and session persistence.

---

## Testing Plan

### Unit tests

1. **Tab split edge hit testing**
   - Top, bottom, left, right zones resolve correctly.
   - Center returns `None`.
   - Source tab returns `None`.
   - Corner overlap chooses nearest normalized edge.

2. **PaneTree merge**
   - Horizontal before/after preserves tree shape.
   - Vertical before/after preserves tree shape.
   - Pane IDs are unique after merge.
   - Split IDs are normalized after merge.
   - Zoom is cleared.

3. **Workspace tab merge index remapping**
   - Source before target maps target index down by one after removal.
   - Source after target keeps target index.
   - Active tab becomes merged target.

### Manual verification

- Drag tab over another tab top edge: preview appears above existing layout; drop merges above.
- Drag tab over bottom/left/right edges: preview and drop match.
- Drag through center: normal reorder behavior remains.
- Drag over source tab: no split preview.
- Drag away and release: no merge.
- Multi-pane dragged tab remains multi-pane after merge.

---

## Open Decisions

Open for a follow-up PR. The current production `TabDragTarget` remains reorder-only.
