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
4. For SSH/tmux changes, run the relevant playbook under `benchmarks/terminal-agent/playbooks/`.
5. Save the JSON record under `.context/benchmarks/` and cite it in your report.

## Rules

- Prefer `pane_id` over `pane_index` when following up on benchmark findings.
- Do not treat playbook observations as strict pass/fail evidence unless the behavior is actually deterministic.
- If a scenario depends on host setup, say so explicitly.
- If a benchmark reveals a product limit, document the limit instead of hiding it behind a softer assertion.
