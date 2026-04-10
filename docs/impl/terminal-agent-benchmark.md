# Implementation: Terminal Agent Benchmark

## Why Con needs its own benchmark

Con is not trying to win a generic coding benchmark.

The product claim is narrower and harder:

- understand the current terminal situation
- preserve pane and host continuity across turns
- operate safely around SSH, tmux, and nested targets
- use stronger control channels when they exist
- stay honest when the runtime cannot actually prove something

That requires a benchmark that mixes deterministic control-plane checks with workflow-level evaluation.

## Benchmark layers

### 1. Strict control-plane suite

This layer is machine-checkable and should run often.

Current runner:

- [`benchmarks/terminal-agent/run.py`](../../benchmarks/terminal-agent/run.py)

Current focus:

- socket identity
- tab discovery
- pane discovery, including live surface readiness
- visible shell execution
- optional live in-tab `agent ask`

These are hard product invariants. If they fail, higher-level SSH/tmux evaluation is not meaningful.

### 2. Operator suites

This layer executes real multi-turn benchmark prompts through `con-cli agent ask`.

It is not pretending to be a fully machine-scored intelligence benchmark yet. The current purpose is:

- exercise complex terminal-agent workflows end to end
- keep the prompt sequence stable and replayable
- record the full transcript as benchmark evidence

Current operator flows live in the operator profiles under `benchmarks/terminal-agent/profiles/`.

Operational rule:

- run operator suites on an idle tab
- do not point the runner at a tab that already has an in-progress `agent ask`

### 3. Playbook workflows

This layer evaluates real user tasks that are not yet stable enough for brittle exact-output assertions.

Current playbooks:

- [`local-codex-workspace.md`](../../benchmarks/terminal-agent/playbooks/local-codex-workspace.md)
- [`local-codex-two-sum-devloop.md`](../../benchmarks/terminal-agent/playbooks/local-codex-two-sum-devloop.md)
- [`remote-host-reuse.md`](../../benchmarks/terminal-agent/playbooks/remote-host-reuse.md)
- [`remote-dual-host-maintenance.md`](../../benchmarks/terminal-agent/playbooks/remote-dual-host-maintenance.md)
- [`tmux-session-awareness.md`](../../benchmarks/terminal-agent/playbooks/tmux-session-awareness.md)
- [`tmux-agent-target-preparation.md`](../../benchmarks/terminal-agent/playbooks/tmux-agent-target-preparation.md)
- [`tmux-remote-devloop.md`](../../benchmarks/terminal-agent/playbooks/tmux-remote-devloop.md)

These are scored against product criteria such as:

- targeting correctness
- continuity across turns
- recovery behavior
- safety
- honesty of uncertainty reporting

### 4. Scored iteration loop

Operator runs become comparable when they are judged against a stable rubric.

Con now commits those rubrics under:

- `benchmarks/terminal-agent/rubrics/`

And ships two support tools:

- `benchmarks/terminal-agent/score.py`
- `benchmarks/terminal-agent/log_iteration.py`
- `benchmarks/terminal-agent/report.py`

This gives the project a repeatable loop:

1. run a profile
2. score the resulting record
3. append a tracked improvement-log entry
4. capture lessons and next focus
5. generate trend reports across many runs

That is how the benchmark becomes a product-improvement system instead of only a demo script.

## Built-in profiles

The benchmark now ships with two profile bands.

Starter profiles:

- `basic-local-shell`
- `basic-local-codex`
- `basic-ssh-dual-host`
- `basic-ssh-tmux`

Operator profiles:

- `operator-local-codex-devloop`
- `operator-ssh-dual-host-maintenance`
- `operator-ssh-tmux-devloop`

Profiles combine:

- environment checks
- the strict suite
- optional operator scenarios
- recommended playbooks for deeper scenario work

This keeps the benchmark maintainable. Everyday development can run a small profile quickly, while deeper release work can expand into the operator playbooks.

## Safety discipline

The operator profiles are deliberately not “do anything destructive and hope”.

Rules:

- prefer read-only checks before mutating admin commands
- treat privilege escalation as an explicit branch in the scenario
- keep the benchmark honest when hosts, package managers, or remote tooling differ

That makes the benchmark usable across real environments instead of only on one curated machine.

## Why this split is intentional

If Con tries to make every tmux or SSH behavior a strict exact-output benchmark too early, the benchmark will become fragile and dishonest.

If Con only does human demos, regressions will slip constantly.

So the benchmark stays hybrid:

- strict where the product is deterministic
- operator-replayed where the workflow should be exercised end to end
- rubric-based where the product is still agentic

## Result discipline

Strict runs write JSON records under `.context/benchmarks/`.

Scored operator runs write comparable cards under `.context/benchmarks/scored/`.

That gives the project:

- reproducible evidence for regressions
- a history of improvement over time
- material that can later be turned into an open-source benchmark guide

## Maintenance rules

When adding a new benchmark:

1. Put it in the strict suite only if it is truly machine-checkable and stable.
2. Use a playbook if success depends on higher-level agent judgment.
3. Prefer product semantics over benchmark cosmetics. A benchmark that flatters the current implementation is not useful.
4. Record limits explicitly. Unknown should stay unknown.

## Current environment policy

The strict suite works against:

- local macOS shell
- a real running Con app session
- the live Unix socket surface

The playbooks currently assume operator-provided remote hosts such as `haswell`.

The final public benchmark can add a reproducible environment guide later. Until then, the benchmark should stay honest about which scenarios are environment-dependent.
