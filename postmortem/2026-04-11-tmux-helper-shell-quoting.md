# tmux helper shell quoting leaked control commands into target panes

## What happened

The ssh to tmux operator benchmark was completing most of the workflow, but tmux-native list and capture calls intermittently left the target shell in a `quote>` continuation state.

Visible symptoms:

- tmux control commands such as `tmux capture-pane -p -J -t '%77' ...` appeared inside the tmux shell target itself
- later shell-lane checks degraded into cautious inventory-only answers because the shell target was no longer clean
- the benchmark plateaued at `14/15` even after target stability and long-running separation were otherwise fixed

## Root cause

The tmux helper builders in `crates/con-agent/src/tmux.rs` generated commands like:

- `sh -c ' ... tmux capture-pane -t '%77' ... '`
- `sh -c ' ... tmux list-panes -F '__CON_TMUX__...' ... '`

That mixed a single-quoted outer `sh -c` script with inner single-quoted tmux arguments produced by `shell_quote()`.

When the helper needed a quoted tmux target or quoted format string, the shell saw mismatched quote boundaries. In practice this caused the helper text to leak across the shell boundary and show up as literal input in the tmux target, producing the `quote>` continuation prompt.

This was not a model problem. It was a control-plane quoting bug.

## Fix applied

- Added `shell_wrap_script()` so tmux helpers build the full helper script as plain text first, then shell-quote the entire script exactly once.
- Migrated tmux list, capture, and exec command builders to that pattern.
- Added `tmux_shell_turn`, a typed tool for one deterministic shell command inside an existing tmux shell target, so install checks and test runs no longer require the model to manually compose `tmux_send_keys + tmux_capture_pane + settle`.
- Re-ran the live ssh to tmux operator benchmark after the fix.

## What we learned

- tmux-native control needs the same shell-boundary rigor as any other protocol attachment. If the helper command is not quoted as a single unit, the control plane stops being trustworthy.
- Once tmux shell work is a first-class typed lane, benchmark quality improves quickly because the model no longer has to improvise low-level tmux choreography for ordinary shell tasks.
- The right abstraction was not “teach the model more tmux.” It was “close the missing shell-lane primitive.”
