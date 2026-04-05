# What happened

con improved pane observability, but the built-in agent could still fail badly in nested SSH/tmux/editor workflows.

The failure pattern was consistent:

- the agent could observe a remote tmux session
- but it still lacked a correct control model
- so it could confuse local execution, con panes, tmux panes, and raw TUI input

# Root cause

The architecture had a runtime observer, but not a control plane.

That left four different concepts too loosely coupled:

1. runtime identity
2. target addressing
3. action type
4. control channel

Prompt guidance tried to cover that gap, but prompt guidance cannot safely replace a typed action model.

# Fix applied

We documented the architecture explicitly in `docs/impl/agent-runtime-control-plane.md` and shipped the first typed control-plane layer in code.

The shipped foundation includes:

- `PaneControlState` as a shared, typed contract above `PaneRuntimeState`
- explicit address-space, visible-target, nested-target-stack, control-channel, and capability fields
- `list_panes`, the system prompt, and visible-exec guards all consuming that same control state
- visible shell execution now gated by capability (`exec_visible_shell`) instead of loosely repeated safety prose

The longer-term design still introduces:

- separate runtime and control-channel graphs
- explicit target kinds for con panes, SSH scopes, tmux sessions/windows/panes, and editors
- capability-based tool families instead of one flat "run command in pane" model
- strict separation between local hidden exec, visible shell exec, tmux-native control, and raw TUI input

# What we learned

- Observability is necessary but insufficient.
- A world-class terminal agent needs both:
  - a runtime model
  - a control model
- The correct long-term fix is not "teach the model to be careful." It is to give the model a tool surface whose types make the dangerous confusion impossible.
