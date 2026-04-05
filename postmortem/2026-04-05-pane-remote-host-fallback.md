# What happened

In an SSH session attached to tmux, the built-in agent could say the focused pane was `local` even when the visible pane clearly showed a remote machine such as `haswell`.

The visible failure was not just weak tmux awareness. The outer remote scope was being lost, so the agent started its reasoning from the wrong machine.

# Root cause

Two design problems combined:

1. con relied too heavily on Ghostty's OSC 7 `PWD` host signal for remote identity even though that signal is not durable for embedded remote detection.
2. When no remote host was detected, the system prompt rendered the pane host as `local` instead of `unknown`.

That meant a missing host signal became a false local claim.

# Fix applied

We changed pane runtime host handling to be evidence-based and non-destructive:

- added an effective `remote_host` to `PaneRuntimeState`
- carry host confidence and evidence source with that value
- infer advisory remote host hints from pane title and tmux-like screen structure when OSC 7 is missing
- compare host hints against the real local hostname before treating them as remote
- update agent prompts and pane metadata to emit `unknown` instead of `local` when host identity is not proven

# What we learned

- `unknown` is a product feature, not a failure. It is strictly better than confidently naming the wrong machine.
- Remote identity has to be merged from multiple pane-local signals. No single Ghostty-exported field is enough today.
- Scope-stack work only helps if every consumer uses the same merged host result. A careful runtime model can still fail if the prompt layer reintroduces false defaults.
