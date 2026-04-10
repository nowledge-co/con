# Playbook: Local Codex Two Sum Dev Loop

## Goal

Verify that Con can prepare and reuse a local Codex workspace for a small real coding loop, not just a one-line readiness check.

## Setup

- Local machine
- `codex` installed in PATH
- Project root: `~/dev/temp/con-bench-twosum`
- Python 3 available locally

## Prompt sequence

1. `Please prepare a Codex workspace in ~/dev/temp/con-bench-twosum`
2. `In that workspace, create a small Python two_sum implementation and a test file, then run the tests.`
3. `Now break one test on purpose and ask Codex to fix it without creating a new target.`
4. `Summarize which pane or target you used for each step.`

## Success looks like

- The agent prepares one stable local Codex target and keeps reusing it
- The workspace path stays exactly under `~/dev/temp/con-bench-twosum`
- The code, test, and run loop all happen in the same prepared target
- Follow-up repair work stays on that target instead of opening another pane
- The final answer can name the reused pane or target clearly

## Failure looks like

- A new pane or target is created on each turn
- The path drifts away from the requested project root
- File edits and test execution happen in different workspaces without explanation
- The agent loses track of whether it is talking to Codex or the outer shell
- The run claims success without actually running the tests

## Score

Score each dimension 0-3:

- `target_preparation`
- `target_reuse`
- `workspace_correctness`
- `execution_loop`
- `follow_up_repair`
