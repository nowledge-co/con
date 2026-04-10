# Pane Index vs Pane Id

## What happened

con exposed `pane_index` broadly in the agent tool surface.

That was good enough for one-turn interaction, but it was the wrong identity for multi-turn work. When users added or closed panes, the visible pane order could change while the agent still held onto an old `pane_index`.

That made follow-up actions fragile:

- a later `read_pane` or `send_keys` could hit the wrong pane
- tmux follow-up work depended too much on the layout staying unchanged
- the agent had to keep re-listing panes just to stay safe

## Root cause

We treated a layout coordinate like a durable resource identity.

The workspace already had a stable leaf id in `PaneTree`, but the agent surface mostly exposed only the positional index from the current flattened pane list.

That was a design mismatch:

- `pane_index` is a snapshot of the current layout
- `pane_id` is the identity of the pane itself

## Fix applied

- `list_panes` and prompt context now expose stable `pane_id`
- core pane tools now accept `pane_id` as a first-class selector
- workspace-side pane resolution now validates `pane_index` and `pane_id` against the same pane when both are supplied
- pane creation now returns both `pane_index` and `pane_id`
- tab summaries and remote workspace summaries now include both values

## What we learned

- A terminal split layout is not a durable address space
- Human-readable positions and machine-stable identities should both exist, but they should not be conflated
- The agent should use `pane_index` for explanation and `pane_id` for follow-up control
