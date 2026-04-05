# Pane Runtime Observability

## What happened

We found a structural flaw in how con described pane state to the built-in agent.

Two failure cases exposed it:

1. a pane started in one state, then the user manually attached to `tmux` or another TUI in the middle of the session
2. the user asked about a pane running nested scopes such as `ssh -> tmux -> shell -> external agent CLI`

The agent often still had shell-oriented metadata, but that metadata no longer described the visible runtime.

The result was not just a bad heuristic.
It was the wrong abstraction.

## Root cause

We were mixing together three different kinds of information:

- backend facts
- shell metadata
- product-level runtime interpretation

That led to two concrete problems.

### 1. Pane context was not modeled as a runtime stack

The system mostly treated a pane as a flat snapshot:

- cwd
- title
- recent output
- last command

Real terminal work is nested. A single pane can move through local shell, SSH, tmux, remote shell, and a foreground CLI agent without ever changing tabs.

Our model had no first-class way to represent that.

### 2. Shell-derived metadata was over-trusted

Even after we removed process-wide `SSH_CONNECTION` and `TMUX` from agent context, the remaining model still relied too heavily on shell metadata and a few screen-derived hints.

That is not durable once a multiplexer or TUI owns the screen.

## Fix applied

We shipped two steps on 2026-04-05.

### 1. Stabilization

- pane context is derived from the pane itself, not process-global environment variables
- pane mode is classified as `shell`, `multiplexer`, `tui`, or `unknown`
- shell metadata carries freshness, so the prompt can treat it as stale when a TUI is visible
- tmux session hints come from pane-local evidence rather than inherited `TMUX`

### 2. Shared runtime model

- con now emits a shared `PaneObservationFrame` for each pane
- runtime interpretation now lives in `PaneRuntimeState`
- the agent prompt and `list_panes` consume the same scope stack and warning model
- advisory detections such as remote shell, tmux, and branded agent CLIs carry explicit confidence instead of pretending to be backend facts

This turns pane awareness into a reusable product surface rather than a prompt-only fix.

## Long-term fix

The durable fix remains a `Pane Runtime Observer`.

What changed is the seam:

- `PaneObservationFrame` is now the backend-fact boundary
- `PaneRuntimeState` is now the shared consumer-facing boundary
- the next missing capability is not another local heuristic
- it is a stronger embedded libghostty observability contract

That contract should eventually expose:

- foreground process identity
- alternate-screen state
- richer semantic prompt lifecycle
- better embedded remote/runtime markers

The design is documented in:

- `docs/impl/pane-runtime-observer.md`

## What we learned

- Pane awareness is a runtime-observability problem, not a prompt-formatting problem.
- Shell metadata and visible-app identity must be separate concepts.
- A shared runtime model is worth shipping before perfect observability exists, as long as confidence is explicit and facts stay separate from inferences.
- Ghostty gives us important terminal facts, but its current embedded API does not expose all of the richer runtime semantics it keeps internally.
- If con wants credibility with SSH, tmux, and external agent CLIs, the next sharp investment is upstream Ghostty observability, not another parallel local process probe.
