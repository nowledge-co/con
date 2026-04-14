# Agent Panel Scrollbar Desync

**Date:** 2026-04-06

## What happened

The scrollbar thumb in the agent panel was not tracking the actual scroll position. Dragging the scrollbar or scrolling content caused visible desync — the thumb position didn't match where you were in the content.

## Root cause

Two issues:

### 1. `track_scroll` and `vertical_scrollbar` on the same element

The code had:

```rust
div()
    .id("agent-messages")
    .relative()
    .overflow_y_scroll()
    .track_scroll(&self.scroll_handle)      // on same div
    .vertical_scrollbar(&self.scroll_handle) // on same div
    ...
```

The correct gpui-component pattern requires **separation**:
- **Container** (relative-positioned parent): gets `.vertical_scrollbar(&handle)`
- **Content** (scrollable child): gets `.overflow_y_scroll().track_scroll(&handle)`

The scrollbar renders as an absolutely-positioned overlay inside the container. When placed on the same element as the scroll content, the scrollbar's position calculations don't match the actual scroll offset — it can't correctly measure the relationship between visible viewport and total content height.

### 2. Shared scroll handle between messages and history

Both the messages view and the history list used the same `scroll_handle`. When switching between them, the stale scroll state from one view leaked into the other, causing further desync.

## Fix applied

1. Split into container + content pattern:

```rust
// Container — owns the scrollbar overlay
div()
    .relative()
    .flex_1()
    .min_h_0()
    .child(messages_content)  // scrollable child
    .vertical_scrollbar(&self.scroll_handle)

// Content — scrolls and reports position
div()
    .id("agent-messages")
    .flex().flex_col()
    .overflow_y_scroll()
    .track_scroll(&self.scroll_handle)
    ...
```

2. Added separate `history_scroll_handle: ScrollHandle` for the history view.

## What we learned

- gpui-component's `ScrollableElement` trait adds the scrollbar as a **child element** of the target. The scrollbar is an absolute overlay that needs a `relative()` parent to anchor to. The scrollable content must be a **sibling** (or child of the container), not the same element.
- Reference implementations in gpui-component (List, Table, scrollbar_story) all follow the container/content split pattern. Always check `3pp/gpui-component/` examples before wiring up scroll.
- Each independent scrollable region needs its own `ScrollHandle` — sharing handles between views that are conditionally rendered causes state leaks.
