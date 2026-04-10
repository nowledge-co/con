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

Run benchmark suites sequentially against a given live app session. Do not point two benchmark runners at the same tab in parallel.

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
python3 benchmarks/terminal-agent/run.py --profile operator-ssh-dual-host-maintenance --suite operator
python3 benchmarks/terminal-agent/run.py --profile operator-ssh-tmux-devloop --suite operator
```

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
- record the assistant transcript for each turn
- give you one JSON record you can review after the run

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
- remote host reuse across follow-up turns
- remote dual-host maintenance flows
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
- `operator-ssh-dual-host-maintenance`
  - multi-host SSH continuity for health, package-manager, and follow-up maintenance work
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
