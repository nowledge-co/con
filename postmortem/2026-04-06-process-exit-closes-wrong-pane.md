# on_terminal_process_exited closes wrong pane

**Date**: 2026-04-06

## What happened

Agent called `create_pane` twice to SSH into two hosts. Both returned successfully (pane_index 2 and 3). `read_pane` on both showed SSH sessions connected. ~40 seconds later, `send_keys pane_index:3` failed with "Invalid pane index 3. Use list_panes to see available panes (1-2)." Pane 3 vanished between read and use.

## Root cause

`on_terminal_process_exited` called `pane_tree.close_focused()` — closing the **focused** pane, not the pane whose process actually exited.

The handler receives `entity: &Entity<GhosttyView>` identifying which terminal died, but ignored it (parameter named `_entity`) and called `close_focused()` unconditionally. If pane B is focused when pane A's process exits, pane B gets killed instead of pane A.

In the observed scenario: pane 3 was focused (create_pane focuses the new pane). If any earlier pane's SSH connection dropped, `close_focused()` would close pane 3 — the most recently created pane, which was alive and well.

## Fix

Three changes, addressing the root cause and two compounding issues found during audit:

1. **Close the correct pane.** Added `close_pane(pane_id: PaneId)` to `PaneTree`. `close_focused()` now delegates to it. `on_terminal_process_exited` uses `pane_id_for_entity(entity.entity_id())` to find the dead pane and close it specifically.

2. **Search all tabs.** The handler only searched `self.tabs[self.active_tab]`, missing dead panes in background tabs. Now iterates all tabs. If the dead pane is the last pane in a non-active tab, calls `close_tab_by_index()` to properly clean up the tab.

3. **Emit exactly once.** `GhosttyProcessExited` was emitted every 8ms tick after process death (~125 no-op events/sec). Added `process_exit_emitted: bool` guard to `GhosttyView` — the event fires once, the handler runs once.

## What we learned

1. **Event handlers must use the event source.** When a framework delivers an event with the source entity, the handler must act on that entity, not on unrelated state (focused pane). Named `_entity` made the unused parameter invisible in review.

2. **"Close focused" is a user action, not a system action.** Users close the focused pane (Cmd+W). System process-exit should close the dead pane. These are different operations that share tree-pruning logic but differ in target selection.

3. **Don't assume current context in event handlers.** Active tab, focused pane — these are user-facing state. System events (process exit) can arrive for any entity in any tab. Handlers must locate the entity first, not assume it lives in the current focus.
