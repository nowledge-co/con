# Postmortem: Pane-title drag cannot reorder tabs left/right after entering tab strip

**Date**: 2026-05-06
**Branch**: wey-gu/pane-title-bar

---

## What happened

Dragging a pane title bar upward into the tab strip correctly triggered the
ghost-tab preview (the pane-to-new-tab promotion path). However, once the
cursor was in the tab strip, moving it left or right had no effect — the ghost
tab stayed pinned at the rightmost slot and never moved to reflect the cursor
position.

---

## Root cause

`tab_strip_tab_bounds` is a `Vec<Bounds<Pixels>>` populated by `on_prepaint`
callbacks on each real tab element. The index used as the key was the **tab's
original data index** (`index`), not its **visual render position**
(`render_pos`).

When a pane-title drag is active, a ghost tab placeholder is inserted into the
tab strip at the current drop slot (`render_pos`). The ghost tab had no
`on_prepaint` callback, so it never wrote its bounds into the array. This left
a **gap** in the bounds array: the entries for tabs that rendered *after* the
ghost were still at their old positions (from the previous frame, before the
ghost was inserted), creating a hole of ~169 px between bounds[N-1] and
bounds[N].

`pane_title_drag_tab_slot` iterates the bounds array looking for the first tab
whose midpoint is to the right of the cursor. Because of the gap, the cursor
had to travel all the way past the gap before the next midpoint was reached,
making the slot appear frozen over a large x range.

Additionally, the bounds array was never cleared between frames, so stale
entries from a previous layout persisted when the ghost tab changed position.

---

## Fix applied

Three changes to `workspace.rs`:

1. **Clear bounds each frame**: at the start of the `show_horizontal_tabs`
   block, `tab_strip_tab_bounds` is cleared so stale entries from the previous
   frame cannot persist.

2. **Track visual render position**: introduced a `visual_pos` counter that
   increments for every element actually pushed into `tabs_container` —
   including ghost tabs. `on_prepaint` now uses `visual_pos` (not `index`) as
   the key, so the bounds array is a contiguous, render-order snapshot of
   exactly what is on screen.

3. **Ghost tabs register their bounds**: both the mid-strip ghost tab and the
   end-of-strip ghost tab now have `on_prepaint` callbacks that write their
   bounds at their respective `visual_pos` slots.

4. **Slot conversion**: `pane_title_drag_tab_slot` now receives
   `visual_count = tab_bounds.len()` (which includes the ghost) and returns a
   visual slot. The caller converts it back to a real drop slot: any visual
   slot greater than the current drop slot is decremented by 1 (because the
   ghost occupies one position in the visual array that does not correspond to
   a real tab).

---

## What we learned

- `on_prepaint` bounds arrays indexed by data index break as soon as the
  render order diverges from the data order (e.g. ghost insertions, live
  reorder previews). Always index by **visual render position** when the
  purpose is hit-testing against what is actually on screen.
- Stale bounds from a previous frame can silently produce wrong slot
  calculations. Clear shared bounds caches at the start of each render pass
  that rebuilds them.
- Debug logging (`eprintln!` of cursor x, bounds array, and computed slot)
  made the gap immediately visible in the first log dump — the bounds showed
  `x=593..762` missing, which was exactly the ghost tab's width.
