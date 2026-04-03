# Pane Tool Grid Mutex Deadlock

**Date:** 2026-04-03

## What happened

The `list_panes` tool was correctly called by the model, the workspace received the request, entered `handle_pane_request`, and then the app hung. The tool timed out after 5 seconds, and subsequent `terminal_exec` calls also hung, requiring force-quit.

## Root cause

Re-entrant mutex deadlock in `handle_pane_request`. The code:

```rust
let grid = terminal.read(cx).grid();
let g = grid.lock();                    // Lock 1: acquires grid mutex
let title = terminal.read(cx).title();  // Lock 2: title() calls grid.lock() internally
```

`parking_lot::Mutex` is not re-entrant. The same thread tried to acquire the grid mutex twice, causing a deadlock on GPUI's main thread. This froze the entire event loop — no further polling, rendering, or input processing could happen.

## Fix

Read the `title` field directly from the already-locked grid guard:

```rust
let g = grid.lock();
let title = g.title.clone().unwrap_or_else(|| format!("Pane {}", idx + 1));
```

## What we learned

1. **Never call entity methods while holding a lock on their internals.** Methods like `terminal.title()` may internally lock the same mutex you already hold.
2. **`parking_lot::Mutex` is non-reentrant by design.** Migrating from `std::sync::Mutex` (which panics on poison) to `parking_lot::Mutex` (which deadlocks on re-entry) changes failure mode from noisy crash to silent hang.
3. **Diagnostic logging was critical.** Without the `handle_pane_request entered` log, the deadlock would have been invisible — it would have looked like the polling loop wasn't receiving the request.
