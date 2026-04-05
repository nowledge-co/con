# Agent Runtime Control Plane

## Why this exists

con now has a usable pane runtime observer.

That solves only half of the problem.

The harder half is control.

The built-in agent must answer three different questions correctly before it acts:

1. what runtime is visible right now
2. what target the user actually means
3. what control channel can operate on that target safely

Current con tools do not separate those questions strongly enough.

That is why the agent can still drift into failures such as:

- running `shell_exec` locally while the user means a remote tmux workflow
- confusing a con pane index with a tmux pane/window index
- typing a shell command into `nvim`
- treating "I can see tmux" as "I can safely execute inside tmux"

The fix is not more prompt text.

The fix is a first-class control plane.

## Current foundation

con now ships the first typed control-plane layer:

- every observed pane can be reduced to a `PaneControlState`
- `list_panes` exposes address space, visible target, control channels, capabilities, and notes
- the system prompt embeds the same control state for the focused pane and peer panes
- visible execution is gated by `exec_visible_shell`, not by ad hoc prompt wording

This is still phase one.

What it solves:

- the model can see that a con pane showing tmux is still only addressable as a con pane
- shell execution safety is computed from typed capability data
- prompt, tools, and runtime guards share one vocabulary

What it does not solve yet:

- tmux-native pane and window addressing
- tmux-native command execution
- editor-native control
- foreground-process truth from Ghostty for nested remote runtimes

## First principles

### 1. Observation is not control

Seeing `ssh -> tmux -> nvim` is not the same as being able to operate on it.

The runtime observer says what con believes is visible.
The control plane says what con can safely do.

### 2. Addressing is not identity

`pane_index=1` is a con pane address.

It is not:

- a tmux pane id
- a tmux window index
- a remote host identity
- an editor buffer

Every target layer needs its own address space.

### 3. Execution must match the runtime layer

There are multiple kinds of action:

- local hidden execution
- visible shell execution
- remote shell execution
- tmux-native execution
- raw TUI input
- editor-native mutation

Those are different operations and must not share one generic "run command" path.

### 4. Capability discovery must precede action

Before the agent acts, it must know whether the target supports:

- shell execution
- tmux control
- raw key input
- editor-native control
- safe file access

If the capability is absent, the tool must refuse.

### 5. Unknown is a valid state

If con cannot prove a target or control channel, it must say `unknown` and stop.

False confidence is the worst product failure here.

## Core model

The correct abstraction is a runtime graph plus a control-channel graph.

### Runtime graph

This graph answers:

`what nested runtimes are visible?`

Example:

```text
ConPane(tab=2,pane=1)
  -> SshConnection(host=haswell)
  -> RemoteShell(kind=zsh)
  -> TmuxSession(name=model-serving)
  -> TmuxWindow(index=4,name=nvim)
  -> TmuxPane(id=%17)
  -> Editor(kind=neovim,file=test.sh)
```

Every node should carry:

- `target_id`
- `parent_target_id`
- `kind`
- `confidence`
- `freshness`
- `metadata`
- `evidence`
- `capabilities`

### Control-channel graph

This graph answers:

`how can con safely act on this runtime?`

Example channels:

- `LocalHiddenExec`
- `VisibleConShell`
- `RemoteShellAnchor`
- `TmuxCommandChannel`
- `TmuxPaneInput`
- `EditorRpc`
- `RawKeyInput`

Every channel should carry:

- `channel_id`
- `target_scope`
- `transport`
- `safety_level`
- `latency_expectation`
- `requires_confirmation`
- `capabilities`
- `last_verified_at`

## Target kinds

These should be explicit product-level types, not just strings in prompt XML.

### Container targets

- `ConPane`
- `SshConnection`
- `RemoteShell`
- `TmuxSession`
- `TmuxWindow`
- `TmuxPane`

### Interactive app targets

- `ShellPrompt`
- `Editor`
- `AgentCli`
- `Dashboard`
- `Pager`
- `UnknownTui`

### Execution targets

- `LocalWorkspace`
- `ShellTarget`
- `TmuxTarget`
- `EditorTarget`

## Capability model

Every runtime target should expose a capability set.

Suggested capabilities:

- `read_screen`
- `search_scrollback`
- `send_raw_input`
- `exec_visible_shell`
- `exec_hidden_local`
- `exec_remote_shell`
- `tmux_query`
- `tmux_capture`
- `tmux_send_keys`
- `tmux_exec`
- `editor_query`
- `editor_apply_edit`

Rules:

- capabilities come from adapters, not prompt inference
- capabilities may be absent even when the runtime is visible
- tools must check capabilities before running

## Control adapters

The control plane needs explicit adapters.

### 1. `PaneShellAdapter`

Purpose:

- execute in a visible shell when the active target is a fresh shell prompt

Maps to:

- current `terminal_exec`

Constraint:

- invalid for tmux/TUI/editor targets

### 2. `LocalExecAdapter`

Purpose:

- run hidden local subprocesses inside the con workspace machine

Maps to:

- current `shell_exec`

Constraint:

- never substitutes for remote intent

### 3. `TmuxControlAdapter`

Purpose:

- query sessions, windows, panes
- capture pane output
- send keys to a specific tmux pane
- execute a shell command in a specific tmux pane that is known to be a shell

Important:

this is not the same as raw input into the currently visible pane.

It needs a control channel anchored in the same tmux scope.

Possible channel sources:

- an existing shell prompt inside the same remote scope
- a dedicated con-managed tmux control anchor
- a future remote helper

Without one of those, tmux should be inspect-only.

### 4. `EditorAdapter`

Purpose:

- reason about `vim` / `nvim` / future editors without shell injection

Near-term reality:

- likely inspect-only unless a stronger channel exists

Long-term:

- Neovim RPC or editor-native integration

### 5. `RawInputAdapter`

Purpose:

- fallback for user-approved, intentional TUI interaction

Examples:

- `<Esc>`
- `:w`
- `Ctrl-C`
- arrow keys

Constraint:

- never used as a hidden substitute for shell execution

## Tool families

The current tool set is too flat.

The future tool surface should be grouped by control layer.

### Discovery tools

- `list_runtime_targets`
- `inspect_target`
- `list_control_channels`
- `resolve_target`

These answer:

- what exists
- what it is
- how certain con is
- what con can safely do with it

### Shell tools

- `exec_local`
- `exec_in_shell_target`

Rules:

- `exec_local` is always local
- `exec_in_shell_target` requires a shell capability
- neither tool may target tmux/editor/TUI targets implicitly

### Tmux tools

- `tmux_list_sessions`
- `tmux_list_windows`
- `tmux_list_panes`
- `tmux_capture_pane`
- `tmux_send_keys`
- `tmux_exec_in_pane`
- `tmux_new_window`
- `tmux_split_pane`

Rules:

- all tmux tools address tmux targets, not con panes
- `tmux_exec_in_pane` requires a tmux control channel plus a shell-capable tmux pane target
- `tmux_send_keys` is explicit TUI input, not a generic exec fallback

### Editor tools

- `inspect_editor`
- `list_buffers`
- `apply_editor_edit`

Rules:

- only available when an editor-native channel exists
- otherwise the agent must stay inspect-only or ask the user to leave the editor

### Generic TUI tools

- `send_input`
- `send_control_key`
- `wait_for_target_state`

Rules:

- these are interaction tools, not shell tools
- they should be clearly marked as destructive/high-risk when they change editor/TUI state

## Prompt contract

The prompt should stop framing all mutation as "run a command in a pane."

The built-in agent should follow this sequence:

1. identify the intended runtime target
2. inspect the target capabilities
3. choose the least-destructive control channel
4. refuse if no safe channel exists

Mandatory prompt rules:

- `pane_index` always means a con pane
- tmux panes/windows use tmux-specific target ids
- `shell_exec` is always local
- do not substitute local execution for remote intent
- do not substitute raw key input for shell execution
- if the visible target is an editor or unknown TUI, con is inspect-only unless an explicit editor/tmux channel exists

## Approval model

Approvals should reflect the control plane, not just the tool name.

Suggested approval classes:

- `LocalHiddenExec`
- `VisibleShellExec`
- `RemoteShellExec`
- `TmuxExec`
- `TmuxSendKeys`
- `EditorMutation`
- `RawTuiInput`

Approval copy must state:

- machine scope
- runtime scope
- target id
- control channel
- whether the action is visible or hidden

## State wiring

The app should wire this in one direction:

```text
Ghostty facts
  -> PaneRuntimeObserver
  -> RuntimeGraphBuilder
  -> CapabilityResolver
  -> Tool/Prompt/UI consumers
```

Consumers must not skip layers.

That means:

- sidebar naming reads resolved targets
- `list_panes` becomes a con-pane summary only
- `list_runtime_targets` becomes the real nested runtime view
- execution tools resolve through capabilities, not raw pane state

## Implementation phases

### Phase 1: Target graph

- add `RuntimeTarget`, `ControlChannel`, `CapabilitySet`, and `TargetId`
- keep current pane observer as the source for top-level runtime facts

### Phase 2: Tool split

- replace the flat "pane + command" model with shell/tmux/tui tool families
- keep `terminal_exec` only as a shell-target adapter

### Phase 3: Tmux adapter

- add inspect-only tmux tools first
- add tmux execution only when a real tmux control channel exists

### Phase 4: Prompt and approvals

- rewrite prompt around target resolution and channel choice
- rewrite approval UI around control-channel classes

### Phase 5: Editor-native control

- add Neovim-native integration if and when a credible control path exists

## Success criteria

con is credible here when all of the following are true:

- it never confuses a con pane with a tmux pane
- it never runs local hidden execution for a remote task unless the user explicitly asks
- it never types shell commands into `vim` / `nvim`
- it can inspect nested `ssh -> tmux -> editor/agent-cli` state honestly
- it can execute inside tmux only through an explicit tmux control path
- approvals make the machine, runtime, and control channel obvious

That is the minimum bar for a world-class terminal-native agent.
