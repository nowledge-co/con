# Read-Only Fact Preflight

## What happened

con gained a tmux preflight step so the agent could answer session-state questions with stronger tmux facts.

The first implementation only ran that preflight for certain message shapes, such as questions that looked like:

- "where am I?"
- "what host is this?"
- "am I in tmux?"

That improved some responses, but it was the wrong design.

## Root cause

The runtime pipeline mixed two different concerns:

- what safe fact sources are available on the focused pane
- what the user happened to ask in this turn

That made fact gathering depend on prompt wording instead of control-plane truth.

This was a classic product mistake:

- cheap in the short term
- harder to reason about later
- easy to overfit to smoke-test prompts

## Fix applied

The harness now runs a deterministic read-only fact preflight before each agent turn:

1. If the focused pane exposes `probe_shell_context`, run it.
2. If that refreshed pane exposes tmux native query, fetch tmux target inventory too.

This preflight is driven only by typed control capabilities on the pane.

It is no longer keyed off user-message heuristics.

## What we learned

- Fact gathering should be a runtime policy, not a language policy.
- If a stronger safe fact source exists, con should harvest it before the model answers.
- Message-shape heuristics are acceptable only for weak observations. They are not acceptable for deciding whether to collect authoritative runtime facts.
- The clean design is: backend facts first, protocol facts next, weak screen observations last.
