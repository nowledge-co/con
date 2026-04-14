# SSH Workspace Continuity Regressions

## What happened

Two product failures showed up together in pre-release SSH testing:

- before the first assistant response, con visibly typed the shell probe payload into the focused pane
- after con created SSH panes for host work, follow-up requests created duplicate host panes instead of reusing the existing ones

The result was noisy and untrustworthy. The terminal looked busy before the model answered, and multi-host follow-up work felt stateless.

## Root cause

There were two distinct design mistakes.

### 1. The harness treated visible shell probing as default preflight

The read-only fact preflight still auto-ran `probe_shell_context` whenever the focused pane allowed it. That preserved correctness, but it leaked a visible probe command into the user’s terminal before unrelated tasks.

### 2. Remote workspace reuse was tied too tightly to fresh shell proof

`ensure_remote_shell_target` and related work-target logic strongly preferred `exec_visible_shell`. That works for panes with fresh shell integration, but it breaks down for ordinary SSH workspaces where:

- con created the pane with `ssh host`
- the pane still looks prompt-like
- there is no tmux or TUI contradiction
- but shell integration is not currently proving a fresh prompt

In that case the pane is still a valid remote workspace for follow-up host work, but the old logic treated it as non-reusable.

## Fix applied

- Removed automatic visible shell probing from the harness preflight.
- Kept only silent tmux snapshot preload when a real tmux query attachment already exists.
- Added typed `remote_workspaces` inventory to terminal context and prompt state.
- Added a remote workspace anchor model that can come from:
  - proven remote host facts
  - con-managed SSH continuity from recent `ssh host` action history plus current prompt-like screen evidence
- Updated pane summaries and work-target hints to expose tab-wide remote host workspaces, not just focused-pane truth.
- Updated SSH target reuse and remote work-target resolution to reuse con-managed SSH panes instead of insisting on fresh shell proof every turn.

## What we learned

- “No heuristics” does not mean “ignore causality.” Action history is a real product signal when it is source-tagged and bounded by contradiction checks.
- Silent fact gathering and visible fact gathering must be different product categories.
- Follow-up remote work needs a tab-wide workspace inventory, not only focused-pane state.
