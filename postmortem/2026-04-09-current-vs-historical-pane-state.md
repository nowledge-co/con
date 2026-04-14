# Current vs Historical Pane State

## What happened

con's pane runtime tracker had become safer than the early title-pattern phase, but it still had a design flaw.

Recent shell actions and `last_command` history could still shape the pane's current visible target. In practice, that meant con could carry forward tmux or agent-CLI history as if it were still the live foreground runtime.

That is the wrong product contract for an open-source terminal that claims to understand the terminal honestly.

## Root cause

The runtime model did not separate two different questions:

1. What is the current verified foreground target?
2. What shell frame did con last verify in this pane?

Those two concepts were both being projected into `scope_stack` and `active_scope`.

This let historical shell context leak into current control state.

## Fix applied

We split the model.

- `scope_stack` is now current-only.
- `last_verified_scope_stack` stores the last shell frame con verified with a typed shell probe.
- `tmux_session` is now current-only.
- `last_verified_tmux_session` keeps the historical tmux shell frame separately.
- Control state, prompt context, and `list_panes` now expose both layers explicitly.
- If the current foreground target is not proven, con now keeps it `unknown` even when it still has useful historical shell context.

## What we learned

- Causal history is valuable, but it must never become present-tense truth by accident.
- The strongest runtime model is not the one that guesses the most. It is the one that keeps current truth and historical orientation separate.
- For terminals, "I know what I last verified" is a real product feature, as long as it is labeled honestly.
