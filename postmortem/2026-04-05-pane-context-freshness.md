## What happened

The built-in agent could misread panes after the user manually changed session state inside the terminal, especially with `ssh` and `tmux`.

In the reported case, the pane was attached to a remote `tmux` session showing a full-screen TUI, but the agent still described the pane using stale shell metadata such as a local cwd and missing remote host.

## Root cause

Two design flaws compounded:

1. `TerminalContext` mixed pane-local state with process-global environment reads (`SSH_CONNECTION`, `TMUX`). Those env vars describe the app process, not the active pane.
2. The prompt did not distinguish between a shell pane and a full-screen `tmux`/TUI pane, so the model over-trusted cwd and last-command fields even when the visible app had taken over the screen.

## Fix applied

- Switched `ssh_host` and `tmux_session` derivation to pane-local snapshots.
- Added focused-pane mode detection (`shell`, `multiplexer`, `tui`, `unknown`) plus a shell-metadata freshness flag.
- Propagated the same metadata through `list_panes`, so tool results surface when pane metadata is stale.
- Updated the system prompt to treat stale shell metadata as advisory and prefer `list_panes`, `read_pane`, and `send_keys` for tmux/TUI inspection.

## What we learned

- Terminal agents need an explicit notion of metadata freshness. Without it, even accurate fields become dangerous once a pane stops behaving like a shell.
- `ssh` and `tmux` awareness has to be modeled per pane, never per app process.
- For interactive terminal software, "what is visible now" must outrank historical shell state.
