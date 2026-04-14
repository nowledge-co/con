# Local Codex Session Resume

Use this playbook to validate that Con can keep one Codex pane alive across unrelated shell work and return to it for a later interactive turn.

## Environment

- Local machine
- Project root: `~/dev/temp/con-bench-codex-resume`
- Coding CLI: `codex`

## Goal

Prove that Con can:

1. prepare one Codex target plus one paired local shell target
2. bootstrap a small Python project and initial commit in the shell lane
3. use the existing Codex pane for a first review/suggestion turn
4. return to the paired shell lane for a follow-up code/test change
5. reuse the same Codex pane for a later follow-up review turn
6. summarize continuity without creating duplicate panes

## Operator prompts

1. `Please prepare a Codex workspace in ~/dev/temp/con-bench-codex-resume by launching Codex there and preparing a paired local shell target for file, git, and test work.`
2. `Keep the Codex target prepared, but use its paired local shell target to create a tiny Python project in ~/dev/temp/con-bench-codex-resume with greet.py and test_greet.py, initialize git, make an initial commit, and run python3 -m unittest -q. Only touch the Codex pane if a blocking trust or continue prompt must be cleared first.`
3. `In the existing Codex pane, answer in exactly two bullets: one small repo-hygiene improvement you would make next, and one one-line commit message idea for the current initial project. Keep it concise and do not create a new target.`
4. `Back in the paired local shell target, add a greet_many helper to greet.py, update the tests, rerun python3 -m unittest -q, and show git diff --stat. Do not create a new target and do not ask Codex again yet.`
5. `Return to the same existing Codex pane and answer in exactly two bullets: whether the greet_many change looks consistent with the earlier structure, and a revised one-line commit message proposal. Do not create a new target.`
6. `Summarize which target handled shell work, which target handled the first Codex turn, which target handled the resumed Codex turn, and confirm the workspace path.`
