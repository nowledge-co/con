# Terminal Agent Improvement Log

Tracked benchmark-backed iteration notes for Con's terminal agent.

## 2026-04-10 11:01 UTC · operator-local-codex-devloop · 12/15 · release_floor

Strong same-target continuity, but the Codex trust prompt and final repair completion still require extra handling.

Score breakdown:
- Target Preparation: 2/3
- Target Reuse: 3/3
- Workspace Correctness: 3/3
- Execution Loop: 2/3
- Follow-up Repair: 2/3

Product changes:
- Added benchmark/operator timeouts so stuck agent asks fail cleanly instead of hanging the whole run.
- Added a tracked improvement log and trend-chart support for repeated benchmark iterations.

Lessons:
- Operator runs must handle coding-cli trust prompts without losing target continuity.
- The benchmark should distinguish partial execution success from a fully closed repair loop.

Next focus:
- Reduce trust-prompt friction in coding-cli target preparation.
- Make the repair step verify green completion before the operator run ends.

Notes:
- A live operator rerun exposed an unbounded con-cli agent ask during the Codex repair step; that failure is now part of the loop design.

## 2026-04-10 12:47 UTC · operator-local-codex-devloop · 5/15 · below_floor

Codex target launched, but the dev loop stalled as soon as the interactive CLI took over the pane.

Score breakdown:
- Target Preparation: 2/3
- Target Reuse: 1/3
- Workspace Correctness: 2/3
- Execution Loop: 0/3
- Follow-up Repair: 0/3

Product changes:
- Ran in an isolated benchmark app with its own XDG data/config home.
- Used the mixed 10-iteration batch to measure stability across local Codex, dual-host SSH, and ssh→tmux workflows.

Lessons:
- Preparing a coding-cli target is not enough; the operator loop still needs a reliable execution lane for file and test work.

Next focus:
- Pair local coding-cli preparation with a stable shell-work target or a cleaner local agent-cli control path.

Notes:
- Batch run 20260410T122538Z iteration 01.

## 2026-04-10 12:47 UTC · operator-ssh-dual-host-maintenance · 12/15 · release_floor

Dual-host routing and apt-state handling worked, but the third follow-up timed out before the maintenance chain closed cleanly.

Score breakdown:
- Host Routing: 3/3
- Workspace Reuse: 3/3
- Privilege Handling: 3/3
- Recovery: 1/3
- Result Clarity: 2/3

Product changes:
- Ran in an isolated benchmark app with its own XDG data/config home.
- Used the mixed 10-iteration batch to measure stability across local Codex, dual-host SSH, and ssh→tmux workflows.

Lessons:
- Remote host routing is ahead of long multi-turn continuity; the later turns still need tighter decomposition.

Next focus:
- Break long remote maintenance flows into shorter bounded turns and improve follow-up reuse after host checks.

Notes:
- Batch run 20260410T122538Z iteration 02.

## 2026-04-10 12:47 UTC · operator-ssh-tmux-devloop · 5/15 · below_floor

The agent connected to haswell, attached tmux, and prepared a shell target, but the second tmux coding turn timed out.

Score breakdown:
- tmux Targeting: 2/3
- Target Stability: 1/3
- Execution Correctness: 0/3
- Separation of Work: 0/3
- Truthfulness: 2/3

Product changes:
- Ran in an isolated benchmark app with its own XDG data/config home.
- Used the mixed 10-iteration batch to measure stability across local Codex, dual-host SSH, and ssh→tmux workflows.

Lessons:
- Initial tmux orientation is materially better than sustained tmux work execution.

Next focus:
- Stabilize second-turn tmux file-work execution after target preparation.

Notes:
- Batch run 20260410T122538Z iteration 03.

## 2026-04-10 12:47 UTC · operator-local-codex-devloop · 9/15 · below_floor

The workspace and Codex target were prepared cleanly, but execution still blurred shell work and CLI interaction and the repair turn timed out.

Score breakdown:
- Target Preparation: 3/3
- Target Reuse: 2/3
- Workspace Correctness: 3/3
- Execution Loop: 1/3
- Follow-up Repair: 0/3

Product changes:
- Ran in an isolated benchmark app with its own XDG data/config home.
- Used the mixed 10-iteration batch to measure stability across local Codex, dual-host SSH, and ssh→tmux workflows.

Lessons:
- Local coding-cli runs still confuse shell execution with in-CLI interaction under pressure.

Next focus:
- Make local coding-cli loops explicitly separate shell actions from prompts sent into the CLI.

Notes:
- Batch run 20260410T122538Z iteration 04.

## 2026-04-10 12:47 UTC · operator-ssh-dual-host-maintenance · 8/15 · below_floor

Healthcheck routing stayed correct, but the package-manager follow-up did not complete within the bounded turn.

Score breakdown:
- Host Routing: 3/3
- Workspace Reuse: 1/3
- Privilege Handling: 1/3
- Recovery: 1/3
- Result Clarity: 2/3

Product changes:
- Ran in an isolated benchmark app with its own XDG data/config home.
- Used the mixed 10-iteration batch to measure stability across local Codex, dual-host SSH, and ssh→tmux workflows.

Lessons:
- The first dual-host turn is solid; the second turn is where continuity still falls off.

Next focus:
- Reduce the scope of the second maintenance turn or improve host-workspace recall under follow-up load.

Notes:
- Batch run 20260410T122538Z iteration 05.

## 2026-04-10 12:47 UTC · operator-ssh-tmux-devloop · 2/15 · below_floor

This tmux run failed in the first turn, so it mostly measured setup fragility rather than tmux execution quality.

Score breakdown:
- tmux Targeting: 1/3
- Target Stability: 0/3
- Execution Correctness: 0/3
- Separation of Work: 0/3
- Truthfulness: 1/3

Product changes:
- Ran in an isolated benchmark app with its own XDG data/config home.
- Used the mixed 10-iteration batch to measure stability across local Codex, dual-host SSH, and ssh→tmux workflows.

Lessons:
- The ssh-to-tmux bootstrap path is still unstable enough to dominate some runs.

Next focus:
- Make remote tmux bootstrap more deterministic before judging deeper tmux dev-loop behavior.

Notes:
- Batch run 20260410T122538Z iteration 06.

## 2026-04-10 12:47 UTC · operator-local-codex-devloop · 13/15 · target_met

This was the first clean local Codex loop: same target reuse, working files, green tests, and a plausible repair pass.

Score breakdown:
- Target Preparation: 2/3
- Target Reuse: 3/3
- Workspace Correctness: 3/3
- Execution Loop: 3/3
- Follow-up Repair: 2/3

Product changes:
- Ran in an isolated benchmark app with its own XDG data/config home.
- Used the mixed 10-iteration batch to measure stability across local Codex, dual-host SSH, and ssh→tmux workflows.

Lessons:
- The local coding-cli path can succeed today when the pane state and timing line up.

Next focus:
- Turn this one good run into the normal case instead of the lucky case.

Notes:
- Batch run 20260410T122538Z iteration 07.

## 2026-04-10 12:47 UTC · operator-ssh-dual-host-maintenance · 8/15 · below_floor

The agent again routed the first dual-host turn correctly, then timed out in the package-state follow-up.

Score breakdown:
- Host Routing: 3/3
- Workspace Reuse: 1/3
- Privilege Handling: 1/3
- Recovery: 1/3
- Result Clarity: 2/3

Product changes:
- Ran in an isolated benchmark app with its own XDG data/config home.
- Used the mixed 10-iteration batch to measure stability across local Codex, dual-host SSH, and ssh→tmux workflows.

Lessons:
- Dual-host continuity is improving more slowly than the first-turn host identification path.

Next focus:
- Focus on second-turn SSH workspace reuse, not just initial host creation.

Notes:
- Batch run 20260410T122538Z iteration 08.

## 2026-04-10 12:47 UTC · operator-local-codex-devloop · 11/15 · release_floor

The run improved by using a separate shell pane for tests, but the repair turn still failed to close.

Score breakdown:
- Target Preparation: 3/3
- Target Reuse: 2/3
- Workspace Correctness: 3/3
- Execution Loop: 3/3
- Follow-up Repair: 0/3

Product changes:
- Ran in an isolated benchmark app with its own XDG data/config home.
- Used the mixed 10-iteration batch to measure stability across local Codex, dual-host SSH, and ssh→tmux workflows.

Lessons:
- A paired shell-plus-CLI shape works better for local coding than trying to do everything inside the CLI pane.

Next focus:
- Promote the paired shell-plus-CLI pattern into a first-class local work-target strategy.

Notes:
- Batch run 20260410T122538Z iteration 09.

## 2026-04-10 12:47 UTC · operator-ssh-tmux-devloop · 13/15 · target_met

This tmux run was strong through three turns: correct target prep, correct file work, and honest tmux reasoning. It failed only on the long-running work separation step.

Score breakdown:
- tmux Targeting: 3/3
- Target Stability: 3/3
- Execution Correctness: 3/3
- Separation of Work: 1/3
- Truthfulness: 3/3

Product changes:
- Ran in an isolated benchmark app with its own XDG data/config home.
- Used the mixed 10-iteration batch to measure stability across local Codex, dual-host SSH, and ssh→tmux workflows.

Lessons:
- The current tmux stack can already handle real file-create/edit/run work when bootstrap succeeds.

Next focus:
- Finish the last gap: reliable separation of long-running tmux work from the main file-work target.

Notes:
- Batch run 20260410T122538Z iteration 10.

