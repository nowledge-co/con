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
2. `Keep the Codex target prepared, but use the paired local shell target to create a small Python two_sum implementation and a test file, then run the tests. Only touch the Codex pane if a blocking trust or continue prompt must be cleared first.`
3. `Now break one test on purpose in the paired local shell. If the Codex pane is waiting at a trust, continue, or edit-approval prompt, clear it in the same target. Then repair the test in the same workspace pair without creating a new target, and rerun the tests.`
4. `Summarize which pane or target you used for each step.`

## Success looks like

- The agent prepares one stable local Codex target and keeps reusing it
- The workspace path stays exactly under `~/dev/temp/con-bench-twosum`
- The code, test, and run loop all happen in the same prepared workspace pair
- Follow-up repair work stays in that workspace pair instead of opening another pane
- Any interactive Codex interstitial is handled without losing continuity
- The final answer can name the reused pane or target clearly

## Failure looks like

- A new pane or target is created on each turn
- The path drifts away from the requested project root
- File edits and test execution happen in different workspaces without explanation
- The agent loses track of whether it is using the shell lane or the Codex lane
- The run claims success without actually running the tests

## Score

Score each dimension 0-3:

- `target_preparation`
- `target_reuse`
- `workspace_correctness`
- `execution_loop`
- `follow_up_repair`
