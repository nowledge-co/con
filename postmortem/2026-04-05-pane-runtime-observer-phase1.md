# What happened

con's pane intelligence had improved scope modeling, but the implementation was still fundamentally snapshot-based.

That meant every consumer could see the same pane slightly differently:

- the agent prompt
- `list_panes`
- the sidebar
- smart-input remote command classification

The result was drift. A pane could lose remote or tmux identity when a sparse frame no longer contained the right hint, even though the visible runtime had not really changed.

# Root cause

Runtime inference was being recomputed ad hoc from one observation frame at a time.

This created two structural problems:

1. important advisory facts such as remote host, tmux, and external agent CLI identity had no persistence model
2. different consumers could bypass the shared runtime model and read raw pane fields directly

# Fix applied

We implemented the first shipped pane runtime observer layer:

- added `PaneRuntimeObserver` and `PaneEvidence`
- moved `PaneRuntimeState::from_observation` to a one-shot adapter over the observer
- each tab now keeps a `PaneId -> PaneRuntimeObserver` map
- agent context, `list_panes`, sidebar naming, input-bar pane state, and smart-input remote classification now use the observer output
- advisory facts such as remote host, tmux, and agent CLI identity persist across sparse frames with bounded retention
- those facts are invalidated when a fresh shell returns

# What we learned

- A scope model without a state model is still fragile.
- "Use the same types everywhere" is not enough if consumers can still bypass the observer and read raw fields directly.
- The next real ceiling is now explicit: stronger foreground-runtime truth has to come from Ghostty exports, not more local heuristics.
