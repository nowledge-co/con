# Local Claude Code Git Workflow

Use this playbook to validate that Con can sustain a richer local coding workflow with a paired Claude Code pane and shell companion in the same project.

## Environment

- Local machine
- Project root: `~/dev/temp/con-bench-claude-git`
- Coding CLI: `claude`

## Goal

Prove that Con can:

1. prepare one Claude Code target plus one paired local shell target
2. create a small Python project and initial git commit in the shell lane
3. make a follow-up code/test change in the same workspace pair
4. ask Claude Code for an interactive review/message turn in the same existing target
5. summarize continuity without creating duplicate panes

## Operator prompts

1. `Please prepare a Claude Code workspace in ~/dev/temp/con-bench-claude-git by launching Claude Code there and preparing a paired local shell target for file, git, and test work.`
2. `Keep the Claude Code target prepared, but use its paired local shell target to create a tiny Python project in ~/dev/temp/con-bench-claude-git with mathops.py and test_mathops.py, initialize git, make an initial commit, and run python3 -m unittest -q. Only touch the Claude Code pane if a blocking trust or continue prompt must be cleared first.`
3. `Still in the same workspace pair, add a new pairwise_sum helper to mathops.py, update the tests, rerun python3 -m unittest -q, then show git status --short and git diff --stat from the paired local shell.`
4. `Ask Claude Code in the existing Claude Code pane for a brief review summary of the current diff and a one-line commit message proposal. Do not create a new target.`
5. `Summarize which target handled shell work, which target handled the Claude Code review turn, and confirm the workspace path.`
