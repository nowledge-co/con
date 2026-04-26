# GPUI `on_drag_move` fires on every listener, not just the hovered one

Date: 2026-04-24
Issue: #66 (vertical tabs polish — drag to reorder)
Branch: `vertical-tab`

## What happened

While wiring up drag-to-reorder for the vertical tabs panel I added
the obvious-looking handlers to each row:

```rust
.on_drag(dragged, |d, _, _, cx| cx.new(|_| d.clone()))
.on_drag_move::<DraggedTab>(cx.listener(move |this, _ev, _, cx| {
    this.drop_slot = Some(i);
    cx.notify();
}))
.on_drop(cx.listener(move |this, dragged: &DraggedTab, _, cx| {
    let from = dragged.index;
    let to = i;
    if from != to { cx.emit(SidebarReorder { from, to }); }
}))
```

In testing the drag chip rendered correctly, the drop indicator line
flashed, but the drop event fired with `from = 0, to = 0` even when
the user dragged from row 2 onto row 0. Sometimes drop didn't fire at
all if the cursor went above the panel.

## Root cause

`InteractiveElement::on_drag_move<T>` is documented as firing during
a drag — but it fires **on every element with a matching listener
type**, regardless of whether the cursor is currently over THAT
element. That's the GPUI source:

```rust
self.mouse_move_listeners.push(Box::new(
    move |event, phase, hitbox, window, cx| {
        if phase == DispatchPhase::Capture
            && let Some(drag) = &cx.active_drag
            && drag.value.as_ref().type_id() == TypeId::of::<T>()
        { (listener)( /* fires unconditionally */ ); }
    },
));
```

So with three rows, every cursor move during a drag fired three
`on_drag_move` callbacks (rows 0, 1, 2 in dispatch order), and the
last one to write `drop_slot = Some(i)` won. That made the indicator
ping-pong between rows on every frame, and `drop_slot` always
ended up at row 2 regardless of cursor position.

The drop handler is fine — it's anchored on the hit-tested element —
but my flawed `drop_slot` state misled the user about where the
drop would land, and on cursor positions outside any row it stayed
stale forever.

## Fix

Filter inside the listener using the bounds GPUI thoughtfully passes
on `DragMoveEvent`:

```rust
.on_drag_move::<DraggedTab>(cx.listener(move |this, ev, _, cx| {
    let p = ev.event.position;
    let b = ev.bounds;
    if p.x < b.origin.x || p.x >= b.origin.x + b.size.width
        || p.y < b.origin.y || p.y >= b.origin.y + b.size.height
    {
        return;
    }
    if this.drop_slot != Some(i) {
        this.drop_slot = Some(i);
        cx.notify();
    }
}))
```

`DragMoveEvent` carries `event.position` (cursor in window coords)
and `bounds` (this listener's element bounds in window coords). The
explicit point-in-bounds check restores the "only the hovered row
updates drop_slot" semantics I assumed in the first place.

Plus a render-time clear so the indicator goes away after a drag-
cancel (`mouseup` outside any row → no `on_drop` fires anywhere):

```rust
fn render(...) {
    if self.drop_slot.is_some() && !cx.has_active_drag() {
        self.drop_slot = None;
    }
    ...
}
```

## What we learned

- **`on_drag_move` is global per type**, not per element. Treat it
  like a "the drag is alive" pulse and gate every state mutation by
  `event.bounds.contains(event.event.position)`.
- **There is no `on_drag_end` hook** for the source element in GPUI,
  so any visual state that should clear on drag-cancel needs to be
  reset from the next `Render::render` after `cx.has_active_drag()`
  flips back to `false`.
- A 3-line debug `log::info!` with cursor + bounds in the move
  handler turned the bug obvious in a single drag (the cursor at y=60
  was clearly outside row 0's `y=79..111` bounds; that's why the
  filter was rejecting everything in the first attempt at the fix).
  Worth keeping that as a debug-build assertion in the future if
  drag-and-drop misbehaves again.
