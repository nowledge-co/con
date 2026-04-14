# Pane Runtime Heuristics Removal

## What happened

con's pane runtime observer and control plane were still promoting pane titles, status-line shapes, and screen-text patterns into typed runtime facts.

That created product-level claims such as:

- "this pane is tmux"
- "this pane is on host X"
- "this pane is Claude Code / Codex / OpenCode"

Those claims were sometimes right, but they were not backed by authoritative backend signals.

## Why this was wrong

The embedded Ghostty C API currently gives con a smaller truth surface than the product was pretending to have:

- command-finished events
- shell integration presence
- visible text
- title
- cwd / OSC 7 state
- process-exited state

It does not currently export explicit foreground-process identity or the richer semantic prompt state that Ghostty keeps internally.

It also does not currently export authoritative foreground command text, alternate-screen state, or remote-host identity on the embedded surface path.

That means:

- title is a raw observation, not runtime truth
- screen structure is a raw observation, not runtime truth
- OSC 7 is not a reliable remote-host source for embedded SSH panes

The old design blurred those boundaries.

## Fix applied

We removed title- and screen-pattern heuristics from typed pane runtime state.

The shipped runtime model now:

- uses Ghostty command boundaries plus PTY input generations to decide whether shell metadata is fresh
- only promotes tmux and agent-CLI identity from authoritative command-line evidence
- treats alternate-screen state as a strong generic interactive-app signal when the backend can export it
- leaves remote host as `unknown` unless stronger backend evidence exists
- uses `unknown` for unproven visible targets instead of `unknown_tui`

We also updated the prompt, pane metadata, and control notes so the model sees both:

- pane title and visible screen text are observations only
- the current Ghostty backend cannot yet prove foreground command text, alternate-screen state, or remote-host identity

## What we learned

- A conservative unknown is better than a plausible lie.
- Pane safety rules must be tied to backend contracts, not prompt wording.
- The missing long-term seam is still upstream Ghostty observability for foreground runtime identity.
- If con wants to be trusted in tmux, SSH, and nested agent workflows, every typed runtime fact must be explainable from first principles.
