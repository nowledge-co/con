# Pane Runtime Tracker

## What happened

con had already stopped using title and screen-pattern heuristics for tmux and SSH identity, but the runtime model was still too shallow.

Pane state was rebuilt from each observation frame plus a few sticky facts. That left a gap:

- con knew some authoritative shell facts right after a probe
- con knew some causal facts about what it had just done to a pane
- but it did not preserve those facts as part of one typed pane state model

The result was a brittle product story. The prompt, `list_panes`, and runtime guards were better than before, but they still lacked a durable model of how the pane was reached.

## Root cause

The architecture was still snapshot-first.

That is the wrong fit for an AI-controlled terminal because terminals are causal systems:

- con creates panes
- con executes visible shell commands
- con sends raw input
- con runs typed probes
- users then continue interacting manually

A single frame cannot capture that history cleanly.

## Fix applied

con now uses a reducer-backed per-pane runtime tracker.

The tracker merges:

- Ghostty observation frames
- typed shell-probe results
- con-originated actions such as pane creation, visible shell exec, raw input, and process exit

The runtime state now carries:

- recent pane actions
- typed shell-context snapshots
- shell-context freshness
- nested runtime stacks derived from current facts plus validated history

Fresh shell prompts now clear active tmux/app state unless a fresh typed shell probe re-establishes that state. Historical actions remain visible as causal evidence, but they no longer masquerade as foreground truth.

## What we learned

- "No heuristics" is necessary but not sufficient. We also need durable state reduction.
- Shell probes are much more valuable when they become part of pane state instead of one-off tool responses.
- Action history is powerful, but only when it is freshness-tagged and prevented from unlocking control by itself.
- For terminal AI, the real architecture is not just observation or control. It is observation + action + invalidation.
