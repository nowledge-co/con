# Playbook: Local OpenCode Two Sum Dev Loop

## Goal

Verify that Con can prepare and reuse a local OpenCode workspace for a small real coding loop, not just a one-line readiness check.

## Setup

- Local machine
- `opencode` installed in PATH
- Project root: `~/dev/temp/con-bench-opencode-twosum`
- Python 3 available locally

## Prompt sequence

1. `Please prepare an OpenCode workspace in ~/dev/temp/con-bench-opencode-twosum`
2. `Using that OpenCode target plus its paired local shell target, create a small Python two_sum implementation and a unittest-based test file, then run python3 -m unittest -q.`
3. `Now break one unittest on purpose. If OpenCode is waiting at a trust or continue prompt for this directory, accept it in the same target. Then ask OpenCode to fix the failing test without creating a new target, and rerun python3 -m unittest -q.`
4. `Summarize which pane or target you used for each step.`

## Success looks like

- The agent prepares one stable local OpenCode target and keeps reusing it
- The workspace path stays exactly under `~/dev/temp/con-bench-opencode-twosum`
- The code, test, and run loop all happen in the same prepared workspace pair
- Follow-up repair work stays on that pair instead of opening another pane
- The final answer can name the reused pane or target clearly

## Failure looks like

- A new pane or target is created on each turn
- The path drifts away from the requested project root
- File edits and test execution happen in different workspaces without explanation
- The agent loses track of whether it is talking to OpenCode or the outer shell
- The run claims success without actually running the tests

## Score

Score each dimension 0-3:

- `target_preparation`
- `target_reuse`
- `workspace_correctness`
- `execution_loop`
- `follow_up_repair`
