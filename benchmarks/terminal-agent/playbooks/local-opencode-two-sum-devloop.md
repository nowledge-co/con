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
2. `Keep the OpenCode target prepared, but use the paired local shell target to create a small Python two_sum implementation and a unittest-based test file, then run python3 -m unittest -q. Only touch the OpenCode pane if a blocking trust or continue prompt must be cleared first.`
3. `Now break one unittest on purpose in the paired local shell. If the OpenCode pane is waiting at a trust, continue, or edit-approval prompt, clear it in the same target. Then repair the failing test in the same workspace pair without creating a new target, and rerun python3 -m unittest -q.`
4. `Summarize which pane or target you used for each step.`

## Success looks like

- The agent prepares one stable local OpenCode target and keeps reusing it
- The workspace path stays exactly under `~/dev/temp/con-bench-opencode-twosum`
- The code, test, and run loop all happen in the same prepared workspace pair
- Follow-up repair work stays on that pair instead of opening another pane
- Any interactive OpenCode interstitial is handled without losing continuity
- The final answer can name the reused pane or target clearly

## Failure looks like

- A new pane or target is created on each turn
- The path drifts away from the requested project root
- File edits and test execution happen in different workspaces without explanation
- The agent loses track of whether it is using the shell lane or the OpenCode lane
- The run claims success without actually running the tests

## Score

Score each dimension 0-3:

- `target_preparation`
- `target_reuse`
- `workspace_correctness`
- `execution_loop`
- `follow_up_repair`
