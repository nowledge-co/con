# Observed Workspace State vs Reusable Control

## What happened

E2E testing exposed three linked failures in the pane-control model:

- panes that visibly looked like `ssh -> tmux` still showed up as `unknown`, so the agent could not answer basic routing questions well
- panes with a visible `Connection to ... closed.` line were still reused as live remote SSH workspaces on follow-up turns
- the workspace root could panic during shutdown if a mouse-move or mouse-up handler fired after the active tab index outlived the tab list

The result was a product that felt inconsistent: conservative in summaries, but still capable of acting on stale workspace continuity.

## Root cause

We had a gap between two layers:

- the observation layer knew about prompt-like, login-banner, htop-like, and SSH-closed screens
- the reuse / routing layer only trusted typed runtime facts plus action-history anchors

That meant:

- tmux-like screens had no middle-tier representation, so they stayed `unknown`
- disconnected SSH panes still carried action-history anchors and remained eligible for reuse
- UI event handlers assumed `tabs[active_tab]` was always valid during teardown

In short, the model separated facts from observations, but it did not yet separate "useful orientation" from "safe reusable control" sharply enough.

## Fix applied

- Added `tmux_like_screen` as an explicit observation hint.
- Let tab-workspace summaries and tmux-target resolution use tmux-like observations for orientation only.
- Kept tmux native control gated on real shell/tmux attachments only.
- Excluded panes with `ssh_connection_closed` or `tmux_like_screen` observations from plain remote-shell reuse.
- Filtered `<remote_workspaces>` and work-target hints so they stop advertising disconnected or tmux-like panes as normal reusable remote shell workspaces.
- Guarded workspace mouse drag handlers against `active_tab >= tabs.len()` during teardown.

## What we learned

- Observation needs its own product role. If it is too weak, the terminal feels blind. If it is treated as truth, the terminal lies.
- Reuse logic must honor lifecycle state, not only historical causality.
- Shutdown paths need the same bounds discipline as normal interaction paths; GPUI event handlers can still fire while teardown is in flight.
