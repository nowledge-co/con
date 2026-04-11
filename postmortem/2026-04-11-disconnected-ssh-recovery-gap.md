# Disconnected SSH Recovery Gap

## What happened

Con learned to avoid reusing disconnected SSH panes, but typed target resolution went too far in the other direction: once a remote pane showed an SSH-closed screen, `resolve_work_target(intent="remote_shell")` returned no usable candidate at all.

## Root cause

The remote-shell resolver treated disconnected workspaces only as "not reusable". It did not preserve their host identity as a recovery hint, so the model lost the typed path for selective host recovery.

## Fix applied

- Added `remote_shell_target` as a preparation-style control path in work-target resolution.
- Disconnected SSH panes now surface as recovery candidates when their host identity is still known.
- Recovery candidates explicitly point at `ensure_remote_shell_target` and explain that only the affected host should be recovered.
- Prompt/runtime docs now call out selective SSH recovery as a first-class path.

## What we learned

- "Not safely executable" is not the same thing as "no longer useful routing information."
- Recovery is its own control-plane state and should stay typed, not fall back to raw reasoning over pane lists.
