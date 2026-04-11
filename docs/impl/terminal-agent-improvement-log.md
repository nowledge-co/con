# Terminal Agent Improvement Log

Tracked benchmark-backed iteration notes for Con's terminal agent.

## 2026-04-11 11:18 UTC · operator-local-codex-git-workflow · 15/15 · world_class

Strong paired-workspace execution end to end: shell-lane git/test work stayed deterministic, the interactive Codex lane was reused only for the requested review turn, and the final summary made continuity auditable.

Score breakdown:
- Target Preparation: 3/3
- Workspace Correctness: 3/3
- Git Loop Execution: 3/3
- Interactive Review Turn: 3/3
- Continuity and Reuse: 3/3

Product changes:
- Added transcript-aware judging and scoring support directly into `iterate.py` so successful operator runs produce comparable artifacts automatically.
- Promoted screen-observed local agent-lane detection and separated prompt text from submit keystrokes for interactive agent-CLI turns.
- Hardened pane-read waiting so slow interactive review turns treat transient read timeouts as recoverable instead of immediate benchmark failure.

Lessons:
- The paired local workspace model is now strong enough for git-backed coding loops, not just toy edit/test cycles.
- Interactive local agent turns need output-settle waiting, not shell-idle waiting.
- Raw benchmark prompts must measure Con continuity, not third-party CLI wandering; narrow, evidence-seeking prompts are much more useful than open-ended “review the repo” asks.

Next focus:
- Add and stabilize session-resume benchmarks that return to the same interactive coding pane after intervening shell work.
- Remove stale pending-request state when tabs are closed or reset so fresh benchmark tabs cannot inherit control-plane busy leaks.

Notes:
- Scorecard: `.context/benchmarks/scored/20260411T111845Z-operator-local-codex-git-workflow.json`
- Judge artifact: `.context/benchmarks/judged/20260411T111844Z-operator-local-codex-git-workflow.json`

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

## 2026-04-11 03:20 UTC · local-coding-follow-up · unscored

Benchmark inspection exposed two local-coding continuity bugs that were not visible in the earlier numeric score alone.

Product changes:
- Added `ensure_local_coding_workspace` so the agent can prepare or reuse the full local coding pair in one step instead of stitching `ensure_local_agent_target` and `ensure_local_shell_target` together ad hoc.
- Prevented panes with recent local agent-cli continuity from being silently reused as generic local shell targets on follow-up turns.
- Normalized `~` project paths before startup shell quoting so new local coding panes no longer fail with literal `cd '~/project'` commands.
- Local coding workspace bootstrap can now create the requested local project directory before launching the agent-cli or shell pane into it.

Lessons:
- The local coding path needs its own first-class control concept. Treating CLI panes and shell panes as interchangeable creates avoidable ambiguity under pressure.
- Startup command correctness matters as much as target selection. A wrongly quoted cwd can make a benchmark look like an agent-reasoning failure when the real issue is bootstrap plumbing.

Next focus:
- Rerun the local Codex operator benchmark on a freshly launched Con build and verify that the paired-workspace path becomes the default, not the lucky case.
## 2026-04-11 04:27 UTC · operator-local-codex-devloop · 11/15 · release_floor

Paired local shell and Codex reuse were correct, but workspace bootstrap still burned one turn recovering from a missing directory and the repair loop did not close.

Score breakdown:
- Target Preparation: 2/3
- Target Reuse: 3/3
- Workspace Correctness: 2/3
- Execution Loop: 3/3
- Follow-up Repair: 1/3

Product changes:
- Ran a fresh live local Codex operator benchmark against the current /tmp/con.sock app on tab 4.
- Confirmed the paired shell-plus-Codex structure works in practice: pane 1 for file/test work, pane 2 for Codex interaction.
- Observed a remaining bootstrap gap: when the requested local project directory does not exist, the first Codex-launch turn burns time recovering from that missing path.

Lessons:
- A first-class paired local coding workspace is the right shape, but it still needs stronger bootstrap semantics for missing project directories.
- Repair continuity is now good enough to reuse the same Codex target, but the benchmark still needs green completion, not just reuse.

Next focus:
- Make paired local coding workspace preparation create the requested project directory before launching the agent-cli or shell pane.
- Keep the same shell-plus-CLI pairing, but close the repair loop to green tests more reliably.

Notes:
- Record: .context/benchmarks/terminal-agent-20260411T042439Z.json
- Scored card: .context/benchmarks/scored/20260411T042722Z-operator-local-codex-devloop.json

## 2026-04-11 04:30 UTC · operator-ssh-dual-host-maintenance · 14/15 · world_class

The live dual-host maintenance run stayed on the correct hosts across all four turns, reused the same SSH workspaces, and handled apt boundaries safely. The only missing piece is a harder recovery exercise such as a disconnected-pane turn.

Score breakdown:
- Host Routing: 3/3
- Workspace Reuse: 3/3
- Privilege Handling: 3/3
- Recovery: 2/3
- Result Clarity: 3/3

Product changes:
- Ran a fresh live dual-host operator benchmark against the current /tmp/con.sock app on tab 5.
- Confirmed that the same SSH workspaces were reused across healthcheck, package-state, warning-log, and continuity-proof turns.
- Raised the dual-host profile to a world-class score for the first time in the tracked loop.

Lessons:
- The remote workspace inventory is now strong enough to sustain a real four-step maintenance chain without falling back to local macOS or recreating panes.
- The remaining gap in this profile is not first-turn routing anymore; it is explicit recovery behavior when a host pane disconnects or becomes stale.

Next focus:
- Add a scored dual-host recovery case that disconnects one host pane mid-scenario and verifies the agent reuses or recreates only the affected workspace.
- Keep the current host-routing path stable while shifting benchmark pressure toward stale/disconnected SSH recovery.

Notes:
- Record: .context/benchmarks/terminal-agent-20260411T043036Z.json
- Scored card: .context/benchmarks/scored/20260411T043059Z-operator-ssh-dual-host-maintenance.json

## 2026-04-11 04:40 UTC · operator-ssh-tmux-devloop · 13/15 · target_met

The live tmux dev loop completed all five turns correctly, but it still relied on raw tmux keystrokes and visible-screen reasoning instead of promoting Con-caused tmux setup into a native tmux anchor soon enough.

Score breakdown:
- tmux Targeting: 2/3
- Target Stability: 3/3
- Execution Correctness: 3/3
- Separation of Work: 2/3
- Truthfulness: 3/3

Product changes:
- Ran a fresh live ssh→tmux operator benchmark against the current /tmp/con.sock app on tab 6.
- Verified that the five-turn tmux dev loop can complete end-to-end with honest reasoning and correct file/run behavior.
- Observed the remaining architecture gap clearly: tmux succeeded through raw prefix/key navigation because Con did not expose tmux-native control soon enough after its own tmux setup actions.

Lessons:
- The functional tmux workflow is already strong, but the control plane still under-promotes causal tmux setup into native tmux control.
- The biggest remaining tmux gap is not file-edit correctness; it is getting from fresh shell prompt to native tmux control before the agent falls back to raw prefix navigation.

Next focus:
- Promote recent Con-caused tmux session creation or targeting into a native tmux shell anchor while the shell prompt is still fresh.
- Re-run the tmux operator profile after that change and verify the agent prefers tmux-native tools over raw keystrokes in the setup turn.

Notes:
- Record: .context/benchmarks/terminal-agent-20260411T043451Z.json
- Scored card: .context/benchmarks/scored/20260411T044040Z-operator-ssh-tmux-devloop.json

## 2026-04-11 05:14 UTC · operator-local-codex-devloop · 7/15 · below_floor

Paired local workspace bootstrap is correct now, but the first real create-and-test turn still times out before the coding loop closes.

Score breakdown:
- Target Preparation: 3/3
- Target Reuse: 1/3
- Workspace Correctness: 3/3
- Execution Loop: 0/3
- Follow-up Repair: 0/3

Product changes:
- Fresh live rerun after typed agent_cli_turn: local coding bootstrap is now clean, but the first create-and-test turn still times out, so the next iteration shifts to reducing shell-plus-agent orchestration burden in that turn.

Lessons:
- The local coding workspace now comes up cleanly without burning a turn on missing-directory recovery.
- The remaining failure has moved from target preparation to the first substantive agent-cli-plus-shell work turn.

Next focus:
- Inspect the timed-out create-and-test turn and reduce the amount of shell-plus-Codex orchestration the model must compose in one turn.
- Add a higher-level local coding step or stronger target guidance so the first coding turn closes reliably after workspace prep.

## 2026-04-11 05:14 UTC · operator-ssh-tmux-devloop · 13/15 · target_met

The tmux dev loop is functionally strong, but setup still leans on raw tmux transcript handling instead of fully clean native control promotion.

Score breakdown:
- tmux Targeting: 2/3
- Target Stability: 3/3
- Execution Correctness: 3/3
- Separation of Work: 2/3
- Truthfulness: 3/3

Product changes:
- Fresh tmux rerun confirms the stack is strong once the target is established; remaining tmux work is cleaner native bootstrap and stronger proof on agent-cli orientation, not basic execution.

Lessons:
- Once the tmux file-work target is established, Con keeps it stable through create, edit, rerun, and separate long-running work.
- The remaining tmux gap is cleaner native orientation and verification, not basic execution.

Next focus:
- Tighten tmux bootstrap so the initial setup turn promotes native control earlier and avoids shell-compat fallback notes.
- Improve installation-check verification so agent-cli orientation ends with stronger proof, not cautious follow-up wording.

## 2026-04-11 05:14 UTC · operator-ssh-dual-host-recovery · 15/15 · world_class

Selective SSH recovery is now operator-grade: one host disconnected, only that host was recovered, and the final mapping stayed explicit.

Score breakdown:
- Host Routing: 3/3
- Selective Recovery: 3/3
- Workspace Reuse: 3/3
- Recovery Honesty: 3/3
- Result Clarity: 3/3

Product changes:
- First live run of the new dual-host recovery profile reached world-class behavior; selective SSH recovery is now a strong typed control path and should stay stable while pressure moves elsewhere.

Lessons:
- Typed remote-workspace recovery now preserves host identity strongly enough for selective recovery instead of full workspace recreation.
- The happy path is no longer the real benchmark pressure on dual-host SSH; recovery is now strong too.

Next focus:
- Keep this path stable while shifting SSH benchmark pressure toward more hostile stale-pane and mixed-layout scenarios.
- Do not regress selective recovery while improving other host-workspace behaviors.

## 2026-04-11 05:36 UTC · operator-local-codex-devloop · 13/15 · target_met

Paired local coding is strong again: clean preparation, same-target reuse, correct shell lane, and bounded Codex follow-up. The remaining gap is that Codex did not finish the repair within the turn budget.

Score breakdown:
- Target Preparation: 3/3
- Target Reuse: 3/3
- Workspace Correctness: 3/3
- Execution Loop: 3/3
- Follow-up Repair: 1/3

Product changes:
- Reuse local agent targets via workspace hints
- Separate shell and agent-cli lanes in local coding

Lessons:
- The shell-vs-agent-cli lane rule fixed the duplicate-work regression; the shell lane now carries deterministic file/test work cleanly.
- The remaining local Codex gap is not workspace reuse anymore; it is bounded interactive repair completion inside the existing Codex pane.

Next focus:
- Improve same-target Codex repair completion without regressing the shell-lane separation.
- Investigate whether a stronger local Codex attachment or a smarter post-turn wait/check path can close the repair loop.

## 2026-04-11 05:55 UTC · operator-ssh-tmux-devloop · 13/15 · target_met

tmux setup is cleaner now: the shell-compat wrapper failure is gone and the workflow completes end to end, but native tmux control still drops out after initial setup so later turns fall back to observed tmux interaction.

Score breakdown:
- tmux Targeting: 2/3
- Target Stability: 3/3
- Execution Correctness: 3/3
- Separation of Work: 2/3
- Truthfulness: 3/3

Product changes:
- Use non-login shells for tmux wrappers
- Add remote tmux workspace preparation tool

Lessons:
- Switching tmux and shell-probe wrappers to non-login shells removed the remote shell-compat leak without changing the control contract.
- The next tmux bottleneck is explicit: native control is still not being retained or promoted through the attached-session workflow, so later turns fall back to raw tmux UI interaction.

Next focus:
- Promote remote tmux session preparation into a durable native control anchor that survives beyond the first setup turn.
- Drive the new ensure_remote_tmux_workspace tool harder in the prompt and benchmark path so ssh->tmux bootstrap stops being reconstructed ad hoc.

## 2026-04-11 06:38 UTC · operator-local-opencode-devloop · 15/15 · world_class

Local OpenCode workspace pair completed the full deterministic create-break-repair loop cleanly.

Score breakdown:
- Target Preparation: 3/3
- Target Reuse: 3/3
- Workspace Correctness: 3/3
- Execution Loop: 3/3
- Follow-up Repair: 3/3

Product changes:
- Replaced visible agent-cli shell-idle waiting with output-settle semantics and refocused the local coding benchmark on Con-owned paired workspaces instead of third-party CLI approval UX.

Lessons:
- The paired shell lane keeps OpenCode available without forcing Con to depend on OpenCode's own interactive approval flow.

Next focus:
- Carry the same target clarity into multi-pane and restored-session summaries.

## 2026-04-11 06:38 UTC · operator-local-claude-devloop · 14/15 · world_class

Local Claude Code workspace pair completed the loop, but the preparation summary still blurred which pane was the interactive target versus the shell companion.

Score breakdown:
- Target Preparation: 2/3
- Target Reuse: 3/3
- Workspace Correctness: 3/3
- Execution Loop: 3/3
- Follow-up Repair: 3/3

Product changes:
- Replaced visible agent-cli shell-idle waiting with output-settle semantics and refocused the local coding benchmark on Con-owned paired workspaces instead of third-party CLI approval UX.

Lessons:
- The paired workspace can execute cleanly even when the assistant's target summary is still sloppy.

Next focus:
- Tighten local target naming and summary generation so Claude-like runs report the pair crisply.

## 2026-04-11 06:38 UTC · operator-local-codex-devloop · 15/15 · world_class

Local Codex workspace pair completed the full deterministic create-break-repair loop cleanly.

Score breakdown:
- Target Preparation: 3/3
- Target Reuse: 3/3
- Workspace Correctness: 3/3
- Execution Loop: 3/3
- Follow-up Repair: 3/3

Product changes:
- Replaced visible agent-cli shell-idle waiting with output-settle semantics and refocused the local coding benchmark on Con-owned paired workspaces instead of third-party CLI approval UX.

Lessons:
- When the benchmark keeps deterministic work in the shell lane, Con can keep the interactive Codex target reusable without stalling the turn.

Next focus:
- Retain the same paired-workspace discipline in tmux and remote coding flows.

## 2026-04-11 07:25 UTC · operator-ssh-tmux-devloop · 14/15 · world_class

The tmux workflow now stays on a clean remote shell target and separates long-running work correctly, but the control plane still falls back to shell-driven tmux commands because native tmux attachment is not retained strongly enough.

Score breakdown:
- tmux Targeting: 2/3
- Target Stability: 3/3
- Execution Correctness: 3/3
- Separation of Work: 3/3
- Truthfulness: 3/3

Product changes:
- Added ensure_remote_tmux_shell_target so ssh->tmux->shell preparation is one typed control-plane step instead of ad hoc tool composition.
- Retained durable tmux shell anchors across prompt-like tmux screens after recent Con-caused tmux setup, and exposed tmux-native capabilities through that retained anchor.

Lessons:
- A typed remote tmux shell-target tool plus a no-attach policy materially improves behavior even before native tmux attachment is fully retained.

Next focus:
- Retain or promote native tmux attachment after remote tmux bootstrap so later turns prefer tmux-native query/send/run over shell-driven tmux commands.

Notes:
- The isolated 20260411T072159Z tmux run no longer attached the outer pane to tmux and cleanly separated the file-work target from the long-running sleep target.

## 2026-04-11 07:35 UTC · operator-ssh-tmux-devloop · 14/15 · world_class

The tmux workflow now completes end to end at a world-class level, but it is still relying on shell-driven tmux fallback because native tmux control is timing out during the shell-anchor query step.

Score breakdown:
- tmux Targeting: 2/3
- Target Stability: 3/3
- Execution Correctness: 3/3
- Separation of Work: 3/3
- Truthfulness: 3/3

Product changes:
- Added ensure_remote_tmux_shell_target as the high-level ssh->tmux->shell bootstrap path so the model no longer has to reconstruct the remote tmux workflow ad hoc.
- Added durable prompt-like tmux shell anchors so native tmux capability can survive beyond the first strict shell-probe frame.

Lessons:
- The benchmark now proves that Con can complete a meaningful ssh->tmux dev loop even when native tmux retention fails, so the remaining gap is control-path robustness rather than high-level workflow design.
- The next fix is in shell-anchor completion detection: the tmux query markers are already visible on screen while the pane still reports busy, which causes a false timeout.

Next focus:
- Make shell-anchor tmux commands succeed as soon as their begin/end markers parse, instead of waiting for busy-state recovery first.
- Re-run the tmux operator profile after the shell-anchor fix and verify that the agent keeps the same high score with fewer shell-driven fallbacks.

Notes:
- Run record: .context/benchmarks/20260411T072736Z-01-operator-ssh-tmux-devloop.json

## 2026-04-11 07:35 UTC · operator-ssh-tmux-devloop · 14/15 · world_class

The tmux workflow now completes end to end at a world-class level, but it is still relying on shell-driven tmux fallback because native tmux control is timing out during the shell-anchor query step.

Score breakdown:
- tmux Targeting: 2/3
- Target Stability: 3/3
- Execution Correctness: 3/3
- Separation of Work: 3/3
- Truthfulness: 3/3

Product changes:
- Recovered prompt-visible shell anchors for tmux-native control instead of treating stale busy flags as hard blockers.
- Made tmux capture/exec marker parsing tolerant of wrapped marker lines in the embedded terminal surface.

Lessons:
- The benchmark now proves that Con can complete a meaningful ssh->tmux dev loop even when native tmux retention fails, so the remaining gap is control-path robustness rather than high-level workflow design.
- The next fix is in shell-anchor completion detection: the tmux query markers are already visible on screen while the pane still reports busy, which causes a false timeout.

Next focus:
- Make shell-anchor tmux commands succeed as soon as their begin/end markers parse, instead of waiting for busy-state recovery first.
- Re-run the tmux operator profile after the shell-anchor fix and verify that the agent keeps the same high score with fewer shell-driven fallbacks.

Notes:
- Fresh isolated run 20260411T080737Z completed all five tmux operator steps through native control and preserved the original file-work target across the long-running separation step.
- Remaining tmux gap is target discipline in the final install-check turn, not basic control-path reliability.

## 2026-04-11 08:40 UTC · operator-ssh-tmux-devloop · 15/15 · world_class

The ssh->tmux operator loop now completes on a clean native tmux path, including deterministic shell-lane install checks inside the correct tmux target.

Score breakdown:
- tmux Targeting: 3/3
- Target Stability: 3/3
- Execution Correctness: 3/3
- Separation of Work: 3/3
- Truthfulness: 3/3

Product changes:
- Fixed tmux helper quoting and added tmux_shell_turn as a typed shell-lane primitive for deterministic work inside existing tmux shell targets.

Lessons:
- tmux control helpers must shell-quote the entire helper script once; nested single-quoted fragments leak tmux commands into the target shell and create false quote-state failures.
- A typed tmux shell-lane turn closes the last gap between tmux target preparation and deterministic shell verification, so install checks and test runs no longer degrade into raw send-keys choreography.

Next focus:
- Broaden the operator benchmark ladder with tmux agent-cli interstitial cases and session-resume cases now that the core tmux shell lane is stable.

Notes:
- Fresh live rerun on /tmp/con.sock reached 15/15. The final install/orientation step now proves command availability inside con-bench:1.1 instead of inferring from pane inventory alone.

## 2026-04-11 04:30 UTC · operator-ssh-dual-host-maintenance · 14/15 · world_class

The live dual-host maintenance run stayed on the correct hosts across all four turns, reused the same SSH workspaces, and handled apt boundaries safely. The only missing piece is a harder recovery exercise such as a disconnected-pane turn.

Score breakdown:
- Host Routing: 3/3
- Workspace Reuse: 3/3
- Privilege Handling: 3/3
- Recovery: 2/3
- Result Clarity: 3/3

Product changes:
- Benchmark live-socket operator runs now create and clean up a fresh tab automatically, and tabs.new bootstraps a live first pane instead of leaving automation on a dead surface.

Lessons:
- The remote workspace inventory is now strong enough to sustain a real four-step maintenance chain without falling back to local macOS or recreating panes.
- The remaining gap in this profile is not first-turn routing anymore; it is explicit recovery behavior when a host pane disconnects or becomes stale.

Next focus:
- Add a scored dual-host recovery case that disconnects one host pane mid-scenario and verifies the agent reuses or recreates only the affected workspace.
- Keep the current host-routing path stable while shifting benchmark pressure toward stale/disconnected SSH recovery.

Notes:
- Fresh isolated dual-host maintenance run on /tmp/con.sock completed on tab 4 and closed cleanly. This validates the new-tab benchmark path rather than only the product workflow itself.

## 2026-04-11 09:56 UTC · operator-ssh-tmux-devloop · 14/15 · world_class · llm_judge

The new LLM-assisted judge path worked on a real world-class tmux run and still found a concrete truthfulness defect that the manual score had previously missed.

Score breakdown:
- tmux Targeting: 3/3
- Target Stability: 3/3
- Execution Correctness: 3/3
- Separation of Work: 3/3
- Truthfulness: 2/3

Product changes:
- Added an LLM-judge workflow that consumes the rubric, raw benchmark record, and saved conversation transcript, then emits structured judgment JSON that `score.py` can turn into a scored card.

Lessons:
- A softened benchmark report is not enough evidence for judging. The judge needs the raw transcript and tool trace to catch subtle truthfulness gaps.
- Even strong tmux runs can overstate what was proven. In this case, the assistant claimed a shell probe result when the evidence only showed a `tmux_shell_turn` install check.

Next focus:
- Use the new judge path on every world-class candidate run before treating it as a stable benchmark milestone.
- Tighten assistant summary language so final answers describe only the checks directly evidenced by the tool trace.

Notes:
- Judge artifact: `.context/benchmarks/judged/20260411T095643Z-operator-ssh-tmux-devloop.json`
- Scored card: `.context/benchmarks/scored/20260411T095650Z-operator-ssh-tmux-devloop.json`
