# tmux Anchor Underpromotion

## What happened

The live `operator-ssh-tmux-devloop` benchmark could complete its five-turn workflow, but it still succeeded by driving tmux through raw prefix/key sequences.

Con already knew more than that:

- it had just created or targeted the tmux session itself
- the shell prompt was still fresh at that point
- the tmux command was causal history from Con, not a guess from screen text

But the control plane still withheld tmux-native capability until a later shell probe inside tmux populated `$TMUX`.

## Root cause

tmux-native capability was gated too narrowly:

- fresh shell prompt
- plus fresh shell probe
- plus `$TMUX`

That excluded a valid same-shell control case:

- Con just ran a tmux command from the current fresh shell prompt
- so that same shell can already execute `tmux list-panes`, `tmux capture-pane`, and `tmux new-window`

The tools did not need `$TMUX` for those commands. The gating logic did.

## Fix applied

- Extended tmux session detection to capture both `-t` and `-s` session arguments.
- Promoted recent Con-caused tmux setup actions into a typed tmux-session hint in pane runtime state.
- Allowed a fresh shell prompt plus recent tmux command continuity to expose tmux-native control immediately.
- Marked that attachment explicitly as action-history-backed rather than pretending it came from a shell probe.

## What we learned

- The right long-term approach is not “more tmux heuristics.” It is promoting truthful causal actions into typed control facts.
- Capability gating should reflect what the transport actually needs. If tmux commands work from the current shell, the control plane should say so.
