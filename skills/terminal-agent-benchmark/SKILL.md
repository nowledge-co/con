---
name: terminal-agent-benchmark
description: Run and maintain Con's terminal-agent benchmark against a live app session. Use when validating con-cli, SSH workspace reuse, tmux awareness, agent-target preparation, or when collecting benchmark evidence for regressions and release notes.
---

# Terminal Agent Benchmark

Use this skill when you need to evaluate Con as a terminal-native agent, not just compile it.

Primary references:

- [`benchmarks/terminal-agent/README.md`](../../benchmarks/terminal-agent/README.md)
- [`docs/impl/terminal-agent-benchmark.md`](../../docs/impl/terminal-agent-benchmark.md)
- [`docs/impl/con-cli-e2e.md`](../../docs/impl/con-cli-e2e.md)

## Default workflow

1. Confirm a live app session and socket exist.
2. Run the strict benchmark:
   - `python3 benchmarks/terminal-agent/run.py --suite strict`
3. If provider setup is present, run:
   - `CON_BENCH_ENABLE_AGENT=1 python3 benchmarks/terminal-agent/run.py --suite all`
4. Prefer a built-in profile when one matches the workflow:
   - `python3 benchmarks/terminal-agent/run.py --list-profiles`
   - `python3 benchmarks/terminal-agent/run.py --profile basic-local-shell`
   - `CON_BENCH_ENABLE_AGENT=1 python3 benchmarks/terminal-agent/run.py --profile basic-local-codex --suite all`
5. Use starter profiles for quick regression checks and operator profiles for richer coding, SSH maintenance, or tmux dev-loop evaluation.
   - `python3 benchmarks/terminal-agent/run.py --profile operator-local-codex-devloop --suite operator`
   - `python3 benchmarks/terminal-agent/run.py --profile operator-local-claude-devloop --suite operator`
   - `python3 benchmarks/terminal-agent/run.py --profile operator-local-opencode-devloop --suite operator`
   - `python3 benchmarks/terminal-agent/run.py --profile operator-ssh-dual-host-maintenance --suite operator`
   - `python3 benchmarks/terminal-agent/run.py --profile operator-ssh-tmux-devloop --suite operator`
6. For SSH/tmux changes, run the relevant playbook under `benchmarks/terminal-agent/playbooks/`.
7. Save the JSON record under `.context/benchmarks/` and cite it in your report.
8. If the run is an operator benchmark, score it with:
   - `python3 benchmarks/terminal-agent/score.py --profile ... --record ... --score ...`
9. Generate a trend report when comparing many runs:
   - `python3 benchmarks/terminal-agent/report.py`

## Rules

- Prefer `pane_id` over `pane_index` when following up on benchmark findings.
- Keep benchmark control on the existing `panes.*` API unless the benchmark
  explicitly targets pane-local surfaces. Surface support is additive and should
  not change the built-in agent's pane/tool contract.
- Do not treat playbook observations as strict pass/fail evidence unless the behavior is actually deterministic.
- If a scenario depends on host setup, say so explicitly.
- Keep operator playbooks safe-by-default. Prefer read-only checks first, and treat destructive or privileged steps as explicit branches.
- If a benchmark reveals a product limit, document the limit instead of hiding it behind a softer assertion.
- Operator suites intentionally serialize `agent ask` turns. If a tab already has a pending agent request, let the runner wait and reuse the same tab instead of opening a parallel benchmark against it.
