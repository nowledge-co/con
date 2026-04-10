# Playbook: Local Codex Workspace

## Goal

Verify that Con can prepare and reuse a local Codex CLI workspace on this machine without losing the pane target across turns.

## Setup

- Local machine
- `codex` installed in PATH
- Project path: `~/dev/temp`

## Prompt sequence

1. `Please prepare a Codex workspace in ~/dev/temp`
2. `Reuse that Codex target and ask it to print ONLY READY_CODEX_OK`

## Success looks like

- The agent routes the work to a stable pane or tmux target instead of spawning duplicates every turn
- It uses the local project path you requested
- A newly created local pane reports `surface_ready=true` and `is_alive=true` instead of returning a placeholder pane
- The follow-up turn reuses the existing Codex target
- It does not confuse the Codex target with the outer shell pane once the target is prepared

## Failure looks like

- A fresh pane or target is created on every follow-up
- The project path is ignored or silently changed
- Pane creation claims success but the resulting pane is not actually initialized
- Raw shell execution is sent into the wrong foreground target
- The agent cannot explain which pane or target it is reusing

## Score

Score each dimension 0-3:

- `target_reuse`
- `path_correctness`
- `control_safety`
- `follow_up_continuity`
