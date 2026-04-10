# What happened

Two separate crashes showed up during normal agent use and app quit:

1. A long tool-result log preview panicked inside the provider layer because a debug string was sliced at byte 500 even when that byte landed inside a multi-byte UTF-8 character.
2. On quit, the workspace could still receive a late render or action path after the last tab was gone, while helper paths still assumed `self.tabs[self.active_tab]` existed.

# Root cause

- The provider logging path used raw byte slicing for preview truncation.
- The workspace kept a strong "there is always an active tab" assumption in render-time and action-time helpers, but teardown violates that assumption briefly.

# Fix applied

- Tool-result preview truncation now uses UTF-8-safe truncation.
- Workspace hot paths that can run during teardown now guard on active-tab existence.
- Render now returns an empty surface when there is no active tab instead of touching the focused terminal.
- Unicode-sensitive title/model formatting paths in the same area were hardened as well.

# What we learned

- Debug/logging paths need the same Unicode discipline as user-facing UI.
- The workspace cannot rely on a permanent non-empty-tab invariant during shutdown.
- "Late event after teardown" needs to be treated as a normal product condition, not a surprising edge case.
