---
name: terminal-agent-improvement-loop
description: Run a benchmark-driven improvement loop for Con's terminal agent. Use when iterating on pane awareness, SSH/tmux behavior, coding-cli flows, benchmark scoring, or progress tracking across many runs.
---

# Terminal Agent Improvement Loop

Use this skill when improving Con as a terminal-native agent, not just fixing a one-off bug.

Primary references:

- [`benchmarks/terminal-agent/README.md`](../../benchmarks/terminal-agent/README.md)
- [`docs/impl/terminal-agent-benchmark.md`](../../docs/impl/terminal-agent-benchmark.md)
- [`docs/impl/terminal-agent-improvement-loop.md`](../../docs/impl/terminal-agent-improvement-loop.md)

## Workflow

1. Choose the smallest operator profile that matches the problem.
2. Run the benchmark on an idle tab:
   - `python3 benchmarks/terminal-agent/run.py --profile operator-local-codex-devloop --suite operator`
   - for repeated clean iterations, prefer `python3 benchmarks/terminal-agent/iterate.py ...`
3. Score the resulting run with the matching rubric:
   - `python3 benchmarks/terminal-agent/score.py --profile ... --record ... --score ...`
   - or ask the built-in agent to judge the raw record and transcript first:
     `python3 benchmarks/terminal-agent/judge_llm.py --profile ... --record ... --socket /tmp/con.sock`
   - then turn that judge artifact into a normal scorecard:
     `python3 benchmarks/terminal-agent/score.py --profile ... --record ... --judge-file ...`
4. Record one short summary, a few lessons, and a few next-focus bullets in the score record.
5. Append the scorecard to the tracked improvement log:
   - `python3 benchmarks/terminal-agent/log_iteration.py --scorecard ... --change "..."`
6. Make one focused product change.
7. Re-run the same operator profile.
8. Generate a report when you need to inspect trend:
   - `python3 benchmarks/terminal-agent/report.py`

## Rules

- Use `strict` suites to protect the floor and `operator` suites to judge real workflows.
- Prefer operator profiles that start a fresh conversation and have bounded step timeouts.
- Prefer typed control-plane improvements over prompt-only fixes.
- Do not overfit to a single benchmark phrase or one host layout.
- Keep unknowns honest when the backend cannot prove more.
- When benchmark infra changes, say whether the product improved or the measurement improved.
- Keep iteration notes concise and comparable across runs.
- Keep `docs/impl/terminal-agent-improvement-log.md` useful to a human reader; it should explain what changed, not just repeat the numeric score.
- If you use the LLM judge, feed it the raw record and transcript, not only the generated report. The report is a summary, not primary evidence.
