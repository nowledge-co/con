# Agent-CLI Turn Gap

## What happened

The benchmark-backed local Codex dev loop improved once con started preparing a paired local shell plus agent-cli workspace, but the repair turn still underperformed. The agent reused the same Codex target correctly, then fell back to raw pane interaction and reran tests before the interactive CLI had visibly settled.

## Root cause

Con had preparation helpers for local and tmux agent-cli targets, but it did not have a typed control primitive for the next step: "send one prompt into the existing interactive CLI target, wait for it to finish enough to inspect, then continue." That left the model to reconstruct this from `send_keys`, `tmux_send_keys`, `read_pane`, and `wait_for`, which made timing and continuity brittle.

## Fix applied

- Added `agent_cli_turn`, a typed tool for existing Codex / Claude Code / OpenCode targets.
- The tool supports:
  - visible local agent-cli panes
  - tmux agent-cli targets through native tmux control
- The tool sends one prompt, waits for the target to settle, and returns a fresh snapshot.
- Updated `resolve_work_target(intent=\"agent_cli\")` so reusable agent-cli targets now point at `agent_cli_turn` instead of raw key tools.
- Updated prompt and playbook guidance to prefer `agent_cli_turn` for follow-up interactive CLI work.

## What we learned

- Preparing a target and continuing a target are different control-plane operations. They deserve different typed tools.
- The benchmark was correct to pressure the repair turn. The missing piece was not more prose; it was a control primitive that closes the interaction loop.
- This pattern generalizes beyond Codex. Any interactive agent CLI in a visible pane or tmux target benefits from the same "submit turn, wait, inspect" abstraction.
