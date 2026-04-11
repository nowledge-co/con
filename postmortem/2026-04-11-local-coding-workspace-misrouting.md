# Local Coding Workspace Misrouting

## What happened

The operator benchmark for the local Codex dev loop exposed a misleading local-workspace shape:

- a pane launched for local Codex work could still be treated as a reusable generic local shell pane on a later turn
- freshly created local coding panes could fail their startup cwd step when the project path started with `~`

The symptom looked like weak agent reasoning, but the control plane itself was ambiguous.

## Root cause

There were two separate causes:

1. Local shell reuse only excluded remote, tmux, and disconnected continuity. It did not exclude recent local agent-cli continuity, so a pane that had just been used for `codex` could still be scored as a good shell candidate.
2. Local startup commands quoted the cwd verbatim. When the benchmark passed `~/dev/temp/...`, the shell saw `cd '~/dev/temp/...'`, which disables tilde expansion and fails even though the intended path exists.

## Fix applied

- Added a first-class `ensure_local_coding_workspace` tool that prepares or reuses the whole local coding pair in one step:
  - one interactive agent-cli pane
  - one separate shell pane for file/test/git work
- Tightened local shell reuse so panes with recent local agent-cli continuity are no longer silently reused as generic shell targets.
- Normalized `~` project paths before shell quoting for local agent and local shell startup commands.
- Updated prompt/playbook guidance so the agent prefers the paired-workspace helper when it needs both the CLI and shell sides of a local coding workflow.

## What we learned

- Local coding is its own control shape. It should not be reconstructed every turn from generic pane heuristics.
- Benchmark failures that look like planning mistakes can still be caused by lower-level bootstrap bugs. The scoring loop is useful only if we keep separating routing failures from setup failures.
