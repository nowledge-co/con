# Implementation: Terminal Agent Improvement Loop

Con now has a benchmark-driven improvement loop for the terminal agent.

The loop is intentionally layered:

1. `strict` benchmark runs protect the deterministic control-plane floor.
2. `operator` benchmark runs exercise realistic multi-turn workflows.
3. scored operator runs make progress comparable across iterations.
4. a tracked iteration log keeps the learning attached to the repo.
5. a report step turns many judged runs into a trend view.

## Why this exists

Con should not depend on ad hoc memory and scattered screenshots to improve.

We need a repeatable loop that answers:

- what scenario was exercised
- how well it performed
- what changed
- what was learned
- whether we are actually climbing toward the target

## Rubrics

Operator profiles now have committed scoring rubrics in:

- `benchmarks/terminal-agent/rubrics/`

Each rubric sets:

- release floor
- target score
- world-class score
- dimension-level criteria

These are intentionally stable enough to compare across runs, but still high-level enough to avoid brittle phrase-matching.

## Scoring

Use:

```bash
python3 benchmarks/terminal-agent/score.py \
  --profile operator-local-codex-devloop \
  --record .context/benchmarks/<run>.json \
  --score target_preparation=2 \
  --score target_reuse=3 \
  --score workspace_correctness=3 \
  --score execution_loop=3 \
  --score follow_up_repair=2 \
  --summary "Strong same-target continuity, still too much Codex trust-prompt friction." \
  --lesson "The operator benchmark must encode trust/interstitial continuation explicitly." \
  --next-focus "Reduce special handling around coding-cli trust prompts."
```

This writes a scored record under `.context/benchmarks/scored/`.

## Reporting

Generate a trend report with:

```bash
python3 benchmarks/terminal-agent/report.py
```

Optional:

```bash
python3 benchmarks/terminal-agent/report.py \
  --profile operator-ssh-tmux-devloop \
  --output .context/benchmarks/reports/tmux.md
```

## Anti-overfitting rules

Do not “improve the score” by narrowing the benchmark until it flatters the current implementation.

Guardrails:

- rotate between local shell, coding-cli, SSH, and tmux operator profiles
- keep negative cases in scope, especially disconnects, stale panes, and missing tmux/native control
- prefer typed control improvements over prompt-only patching
- treat unknown as unknown when the backend cannot prove more
- when the benchmark changes, explain whether the product got stronger or the evaluation only got clearer

## Iteration discipline

Each product iteration should produce:

1. a benchmark record
2. a scored record
3. a tracked improvement-log entry
4. one concise summary
5. a few lessons
6. the next focused improvement area

That gives us a trail we can turn into a 50-iteration report later without reconstructing the history by hand.

## Improvement log

Append a tracked note with:

```bash
python3 benchmarks/terminal-agent/log_iteration.py \
  --scorecard .context/benchmarks/scored/<run>.json \
  --change "what changed in the product" \
  --note "anything unusual about the benchmark run"
```

This updates `docs/impl/terminal-agent-improvement-log.md`, which is the durable human-readable trail that complements the scored JSON files and trend report.
