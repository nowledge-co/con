# Terminal Agent Benchmark

This benchmark is how Con measures progress toward a real terminal-native agent.

It is intentionally split into three layers:

- **Strict suites**: machine-checkable control-plane verification against a live running Con session
- **Operator suites**: replayable multi-turn prompt sequences that execute real SSH, tmux, and coding workflows and record the transcript
- **Playbooks**: scenario guides and scoring rubrics for judging the operator runs honestly

That split is deliberate. Con should have hard invariants where the product is deterministic, and honest rubrics where the product is agentic.

## What this benchmark is for

Use it to:

- catch regressions in `con-cli` and the local socket bridge
- validate that visible shell execution remains safe and reusable
- track session understanding across SSH, tmux, and agent workflows
- maintain release-quality evidence as Con moves toward open source

## Run the strict suite

With a live app already running:

```bash
python3 benchmarks/terminal-agent/run.py --suite strict
```

Optional live in-tab agent verification:

```bash
CON_BENCH_ENABLE_AGENT=1 python3 benchmarks/terminal-agent/run.py --suite all
```

Run benchmark suites sequentially against a given live app session. For live-socket operator runs without `--tab`, the runner now creates a fresh temporary tab automatically so one operator run does not collide with the active conversation or another benchmark tab.

Defaults:

- socket path: `/tmp/con.sock`
- tab target: active tab
- record path: `.context/benchmarks/terminal-agent-<timestamp>.json`

You can override them:

```bash
python3 benchmarks/terminal-agent/run.py \
  --socket /tmp/con.sock \
  --tab 2 \
  --record .context/benchmarks/latest.json
```

List the built-in profiles:

```bash
python3 benchmarks/terminal-agent/run.py --list-profiles
```

Run a concrete profile:

```bash
python3 benchmarks/terminal-agent/run.py --profile basic-local-shell
CON_BENCH_ENABLE_AGENT=1 python3 benchmarks/terminal-agent/run.py --profile basic-local-codex --suite all
```

Run a complex operator benchmark:

```bash
python3 benchmarks/terminal-agent/run.py --profile operator-local-codex-devloop --suite operator
python3 benchmarks/terminal-agent/run.py --profile operator-local-claude-devloop --suite operator
python3 benchmarks/terminal-agent/run.py --profile operator-local-opencode-devloop --suite operator
python3 benchmarks/terminal-agent/run.py --profile operator-local-codex-git-workflow --suite operator
python3 benchmarks/terminal-agent/run.py --profile operator-local-codex-session-resume --suite operator
python3 benchmarks/terminal-agent/run.py --profile operator-ssh-dual-host-maintenance --suite operator
python3 benchmarks/terminal-agent/run.py --profile operator-ssh-dual-host-recovery --suite operator
python3 benchmarks/terminal-agent/run.py --profile operator-ssh-tmux-devloop --suite operator
```

Run an isolated multi-iteration batch:

```bash
python3 benchmarks/terminal-agent/iterate.py \
  --suite operator \
  --profile operator-local-codex-devloop \
  --profile operator-local-claude-devloop \
  --profile operator-local-opencode-devloop \
  --profile operator-ssh-dual-host-maintenance \
  --profile operator-ssh-dual-host-recovery \
  --profile operator-ssh-tmux-devloop
```

`iterate.py` launches a fresh Con app instance for each iteration with its own socket, XDG data home, and XDG config home. Use it when you want a real trend line instead of one hand-run benchmark.

On macOS, `iterate.py` also forces isolated session and conversation storage with `CON_SESSION_PATH` and `CON_CONVERSATIONS_DIR`, so fresh benchmark apps do not inherit your real restored tabs or saved conversations.

If the batch runner reports `blocked` with `ghostty_surface_bootstrap_unavailable`, that is not a scored product regression. It means the benchmark environment could not produce a live Ghostty surface for the launched app process. In that case, prefer running operator suites against an already-live Con session with `--socket`.

Use a tab that is not already serving another in-progress agent request. Operator suites serialize turns on one tab by design, and the runner will fail fast if the tab stays busy too long.

## Strict suite coverage

Today the strict runner verifies:

- socket identity and method inventory
- tab discovery
- pane discovery on the active tab, including surface-ready live panes
- visible shell execution and reuse of a proven shell pane
- optional live built-in agent response

This is the hard floor. If these break, Con is not ready for higher-level SSH/tmux evaluation.

## Operator suites

Operator suites are real multi-turn `agent ask` flows driven by the benchmark runner.

They do not pretend to be fully machine-scored intelligence benchmarks. Instead, they:

- execute the full prompt sequence automatically
- start from a fresh in-tab conversation when the profile asks for it
- optionally run visible-shell setup commands before the first operator turn
- optionally run local shell setup commands before the first operator turn when benchmark hygiene needs work outside the live Con tab
- record the assistant transcript for each turn
- give you one JSON record you can review after the run

Each operator step can also carry its own timeout budget, so a stuck agent turn fails the benchmark honestly instead of hanging the whole run forever.

This is the practical bridge between the strict floor and the final public benchmark story.

## Scoring and reports

Operator runs become comparable when they are scored against a stable rubric.

Rubrics live in [`rubrics/`](./rubrics/). Score a run with:

```bash
python3 benchmarks/terminal-agent/score.py \
  --profile operator-local-codex-devloop \
  --record .context/benchmarks/<run>.json \
  --score target_preparation=2 \
  --score target_reuse=3 \
  --score workspace_correctness=3 \
  --score execution_loop=3 \
  --score follow_up_repair=2 \
  --summary "Strong same-target continuity, still too much trust-prompt friction." \
  --lesson "Trust prompts still interrupt the flow." \
  --next-focus "Reduce special handling around coding-cli interstitial prompts."
```

This writes a scored record under `.context/benchmarks/scored/`.

For an LLM-assisted judgment pass, ask Con's built-in agent to judge the rubric against the raw benchmark record and saved conversation transcript:

```bash
python3 benchmarks/terminal-agent/judge_llm.py \
  --profile operator-ssh-tmux-devloop \
  --record .context/benchmarks/terminal-agent-<run>.json \
  --socket /tmp/con.sock
```

That writes a judge artifact under `.context/benchmarks/judged/`.

Then convert the judge output into a normal rubric scorecard:

```bash
python3 benchmarks/terminal-agent/score.py \
  --profile operator-ssh-tmux-devloop \
  --record .context/benchmarks/terminal-agent-<run>.json \
  --judge-file .context/benchmarks/judged/<judge>.json
```

This is the intended shape:

- hard invariants stay machine-checkable
- the LLM judge reads the raw transcript, not just the summary
- final scores still stay rubric-constrained and auditable

Append that scored run to the tracked improvement log with:

```bash
python3 benchmarks/terminal-agent/log_iteration.py \
  --scorecard .context/benchmarks/scored/<run>.json \
  --change "one focused product change"
```

Generate a trend report with:

```bash
python3 benchmarks/terminal-agent/report.py
python3 benchmarks/terminal-agent/report.py --profile operator-ssh-tmux-devloop
```

For repeated improvement work, use:

- [`skills/terminal-agent-improvement-loop/SKILL.md`](../../skills/terminal-agent-improvement-loop/SKILL.md)

## Playbooks

The playbooks in [`playbooks/`](./playbooks/) cover the product behaviors that still need structured scenario evaluation:

- local Codex workspace preparation and reuse
- local Codex file-edit-test-repair loops
- local Codex git-backed coding and diff-review loops
- local Codex session-resume after intervening shell work
- local Claude Code workspace preparation and reuse
- local Claude Code git-backed coding and diff-review loops
- local Claude Code session-resume after intervening shell work
- local OpenCode workspace preparation and reuse
- local OpenCode git-backed coding and diff-review loops
- local OpenCode session-resume after intervening shell work
- remote host reuse across follow-up turns
- remote dual-host maintenance flows
- remote dual-host recovery after one host disconnects
- tmux session understanding
- tmux agent-target preparation
- remote tmux file-edit-run-reuse loops

Each playbook includes:

- setup
- prompts to run
- what success looks like
- what failure looks like
- scoring dimensions

They are designed to become announcement-grade benchmark material later, once the environment setup is stabilized.

## Built-in profiles

The profile set is now split into two bands.

### Starter profiles

These are the fast everyday regressions:

- `basic-local-shell`
  - baseline local tab and visible shell control
- `basic-local-codex`
  - local Codex CLI workflow on `~/dev/temp`
- `basic-ssh-dual-host`
  - remote host reuse using `haswell` and `cinnamon`
- `basic-ssh-tmux`
  - mixed plain SSH and `ssh -> tmux` orientation using `haswell` and `cinnamon`

### Operator profiles

These are the richer human-scored scenario tracks:

- `operator-local-codex-devloop`
  - local Codex workspace setup, file creation, test execution, and repair loop
- `operator-local-claude-devloop`
  - local Claude Code workspace setup, file creation, test execution, and repair loop
- `operator-local-opencode-devloop`
  - local OpenCode workspace setup, file creation, test execution, and repair loop
- `operator-local-codex-git-workflow`
  - local Codex paired workspace with git init, diff evidence, and interactive review turn
- `operator-local-claude-git-workflow`
  - local Claude Code paired workspace with git init, diff evidence, and interactive review turn
- `operator-local-opencode-git-workflow`
  - local OpenCode paired workspace with git init, diff evidence, and interactive review turn
- `operator-local-codex-session-resume`
  - local Codex continuity after returning from shell-lane work back into the same interactive pane
- `operator-local-claude-session-resume`
  - local Claude Code continuity after returning from shell-lane work back into the same interactive pane
- `operator-local-opencode-session-resume`
  - local OpenCode continuity after returning from shell-lane work back into the same interactive pane
- `operator-ssh-dual-host-maintenance`
  - multi-host SSH continuity for health, package-manager, and follow-up maintenance work
- `operator-ssh-dual-host-recovery`
  - selective recovery when one host workspace disconnects but the other should stay intact
- `operator-ssh-tmux-devloop`
  - remote `ssh -> tmux` file work, long-running target separation, and agent-CLI orientation

Profiles add environment checks, operator scenarios, and recommended playbooks on top of the strict suite. Starter profiles are for day-to-day regression work. Operator profiles are the current bridge toward the final public benchmark story.

## Safety model

The richer SSH and tmux playbooks are intentionally safe by default.

- They prefer read-only checks first
- They treat package-manager mutation as conditional and explicit
- They require the agent to explain privilege boundaries instead of bluffing past them

That matters for credibility. A benchmark that only passes on hand-held, over-permissioned hosts is not useful.

## Benchmark philosophy

Con is not trying to benchmark generic coding-agent intelligence.

It is benchmarking whether an agent:

- targets the right pane or tmux object
- preserves session continuity across turns
- avoids unsafe execution in TUIs
- explains uncertainty honestly
- recovers cleanly when panes disappear, SSH closes, or tmux control is missing

## Result records

Each strict run writes a JSON record under `.context/benchmarks/`.

That makes it easy to:

- attach evidence to bug reports
- compare pre/post-change runs
- accumulate benchmark history while the product is still pre-release

## How to extend it

1. Add a new strict case to [`run.py`](./run.py) when the behavior is deterministic and machine-checkable.
2. Add or update a playbook when the scenario needs human or rubric-based evaluation.
3. Keep the benchmark honest. Do not convert uncertain agent behavior into fake exact assertions just to increase a pass rate.
