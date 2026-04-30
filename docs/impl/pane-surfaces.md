# Pane Surfaces

## Summary

Con's public pane model now has two layers:

- **Pane**: a visible split rectangle in a tab.
- **Surface**: a live terminal session hosted by a pane.

Every pane always has one active surface. A pane may also host additional
inactive surfaces, which behave like pane-local terminal tabs. The existing
pane APIs, built-in agent tools, and terminal-agent benchmarks continue to see
only the active surface of each pane.

This is an additive control-plane feature for external orchestrators that need
interactive subagent patterns without creating unbounded visible splits.

## Why This Exists

Interactive subagent tools often want this lifecycle:

1. First worker gets a visible split, usually to the right of the caller.
2. Later workers join that same worker pane as tabs.
3. The orchestrator can read, send keys, focus, rename, and close those tabs.
4. When the last owned worker tab exits, the worker pane can be closed.

cmux calls the inner unit a `surface`, so Con uses the same word for the
control-plane concept. In product terms, a surface is simply a terminal session
inside a pane.

This is distinct from the lower-level Ghostty/native rendering surface used by
the terminal backend. In this document, "surface" always means the
control-plane terminal session inside a pane.

## Compatibility Contract

The compatibility rule is strict:

- `panes.*` targets panes and operates on the active surface only.
- The built-in agent prompt/tool set keeps the existing pane model.
- Existing benchmark fixtures should keep using `panes.*` unless a test is
  explicitly about pane-local surfaces.
- Surface APIs are opt-in under `surfaces.*`.

This prevents a new abstraction from changing the behavior of Con's benchmarked
agent harness or the user-facing pane picker.

## Control API

The JSON-RPC methods are:

- `tree.get`: returns tabs, panes, and pane-local surfaces.
- `surfaces.list`: lists surfaces in a tab, optionally scoped to one pane.
- `surfaces.create`: creates a new surface inside an existing pane.
- `surfaces.split`: creates a split pane with an initial surface.
- `surfaces.focus`: activates a surface in its pane.
- `surfaces.rename`: updates the surface title shown in the pane-local strip.
- `surfaces.close`: closes a surface; it refuses to close the last surface in a
  pane unless the caller opts into closing an owned ephemeral pane.
- `surfaces.read`: reads recent terminal content from a surface.
- `surfaces.send_text`: writes literal text bytes to a surface.
- `surfaces.send_key`: sends a small named key set (`escape`, `enter`, `tab`,
  `backspace`, `ctrl-c`, `ctrl-d`) to a surface.
- `surfaces.wait_ready`: waits until the surface has a live terminal session
  or the timeout expires, then returns readiness and shell-integration metadata.

Surface targeting accepts:

- `surface_id`: stable within the tab for the lifetime of the surface.
- `pane_id`: stable within the tab for the lifetime of the pane.
- `pane_index`: current visible pane position, useful for humans but not ideal
  for follow-up automation.

Prefer `surface_id` for follow-up automation.

## CLI Examples

First create a visible worker pane:

```bash
con-cli --json surfaces split \
  --tab 1 \
  --pane-id 0 \
  --location right \
  --title worker-1 \
  --owner pi-interactive-subagents \
  --command "codex"
```

Then create more worker sessions inside the same pane:

```bash
con-cli --json surfaces create \
  --tab 1 \
  --pane-id <worker_pane_id> \
  --title worker-2 \
  --owner pi-interactive-subagents \
  --command "codex"
```

Wait for the worker before driving it:

```bash
con-cli --json surfaces wait-ready --surface-id <surface_id> --timeout 10
```

Drive a worker:

```bash
con-cli --json surfaces send-text --surface-id <surface_id> "explain this repo"
con-cli --json surfaces send-key --surface-id <surface_id> enter
con-cli --json surfaces read --surface-id <surface_id> --lines 120
```

Close a worker surface:

```bash
con-cli --json surfaces close --surface-id <surface_id>
```

Close the last surface and its owned worker pane:

```bash
con-cli --json surfaces close \
  --surface-id <surface_id> \
  --close-empty-owned-pane
```

`surfaces split` marks its initial pane as closeable by default. Use
`--keep-pane-when-last` when the pane should survive after its last surface
closes.

## Lifecycle Rules

- Only active surfaces are visible.
- Hidden surfaces keep running and continue receiving terminal output.
- Tab, pane, window, and app close paths tear down every surface, not just the
  active surface.
- If a non-last surface exits, Con removes only that surface.
- If the last surface in a pane exits, Con follows the existing pane close
  escalation: close pane, then close tab, then close the workspace window.

## Current Limit

Session restore persists the split layout and active surface cwd, matching the
existing terminal-session restore behavior. It does not yet restore multiple
live pane-local surfaces because terminal processes themselves are not restored.
