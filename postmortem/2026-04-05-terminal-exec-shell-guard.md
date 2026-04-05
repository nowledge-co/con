# What happened

The built-in agent could issue `terminal_exec` against a pane whose visible target was not a shell.

In practice this meant con could type shell commands into a tmux pane running `nvim`, which is unacceptable for a terminal product that claims to understand pane runtime safely.

# Root cause

The system prompt warned the model about tmux and TUIs, but the execution layer still allowed `terminal_exec` and `batch_exec` on any live pane.

That was the wrong boundary. Prompt guidance is advisory. Tool execution rules must enforce the product's safety contract.

# Fix applied

We made visible command execution shell-only by default:

- added a shared `direct_terminal_exec_is_safe` policy based on pane runtime state
- `terminal_exec` and `batch_exec` now refuse to write commands into panes that are not proven plain-shell targets
- tool descriptions and system prompt text now state explicitly that:
  - `pane_index` refers to a con pane, not a tmux pane
  - `shell_exec` is always local
  - tmux, nvim, and other TUIs must be inspected or interacted with through pane tools, not shell-command injection

# What we learned

- Safety-critical execution rules cannot live only in prompts.
- The product needs a strict distinction between:
  - local hidden execution
  - visible shell execution
  - interactive TUI input
- Until con has a true tmux-aware execution path, refusing shell injection into tmux/TUI panes is the correct product behavior.
