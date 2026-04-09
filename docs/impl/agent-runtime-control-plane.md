# Agent Runtime Control Plane

## Why this exists

con now has a usable pane runtime tracker.

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

See also:

- `docs/study/terminal-control-plane.md`

## Current foundation

con now ships the first typed control-plane layer:

- every observed pane can be reduced to a `PaneControlState`
- every pane now keeps a reducer-backed runtime tracker instead of recomputing state from one frame
- `list_panes` exposes address space, visible target, nested target stack, explicit control attachments, control channels, capabilities, and notes
- the system prompt embeds the same control state for the focused pane and peer panes
- visible execution is gated by `exec_visible_shell`, not by ad hoc prompt wording
- panes with a proven fresh shell prompt now expose a read-only `probe_shell_context` capability for typed shell-scoped facts
- typed shell probes and con-originated actions are preserved as causal history on the pane, with freshness and invalidation rules
- current verified foreground state is now modeled separately from the last verified shell frame, so stale shell history cannot masquerade as the live visible target

This is still phase one.

What it solves:

- the model can see that a con pane showing tmux is still only addressable as a con pane
- the model can represent nested situations such as `remote shell -> tmux -> agent CLI` instead of flattening them into one label
- a fresh shell prompt inside tmux can now be modeled as `remote shell -> tmux -> shell`, which keeps visible shell execution safe without pretending con has tmux-native control
- shell execution safety is computed from typed capability data
- shell-scoped probing is explicit instead of being hidden behind generic terminal execution
- prompt, tools, and runtime guards share one vocabulary
- tmux now has an explicit inspectable adapter slot, rather than being implied only through generic pane metadata
- recent con actions stay available as causal evidence so the agent can understand how a pane was reached without treating history as present-tense truth
- when the current foreground target is unproven, control falls back to `unknown` while `last_verified_shell_stack` remains available as historical orientation

con now also ships the first true protocol attachment beyond raw pane observation:

- if a pane has a fresh shell prompt
- and a typed shell probe confirms same-session tmux

then con can expose a native tmux control attachment for that pane.

That attachment currently supports:

- tmux target discovery
- tmux pane capture
- tmux-native send-keys to a chosen tmux target

This is the right abstraction because it scales across:

- local tmux
- ssh -> tmux
- tmux panes running Codex CLI / Claude Code / OpenCode

without needing app-specific screen scraping.

What it does not solve yet:

- tmux-native pane and window addressing
- tmux-native command execution
- editor-native control
- foreground-process truth from Ghostty for nested remote runtimes
- manual tmux/editor detection on the current embedded Ghostty backend when command text and alternate-screen state are not exported

## First principles

### 1. Observation is not control

Seeing `ssh -> tmux -> nvim` is not the same as being able to operate on it.

The runtime tracker says what con currently believes is visible.
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

### 6. Attachments are first-class

The terminal world does not have one universal protocol like CDP.

So con needs explicit protocol attachments.

Examples:

- Ghostty surface attachment
- shell prompt attachment
- tmux control attachment
- Neovim RPC attachment

An attachment is the real unit of authority.

Observation says what may be visible.
An attachment says what con can actually talk to.

### 7. Causality matters, but it is not truth

con often knows how it entered a pane:

- it created the pane with `ssh haswell`
- it executed `tmux attach -t work`
- it sent raw input to a TUI
- it ran a typed shell probe

That causality is extremely useful. But it must stay source-tagged and freshness-tagged.

The correct product rule is:

- current backend facts and fresh typed probes define active runtime truth
- con action history explains how the pane got here
- historical action history must never unlock control by itself

### 8. Current state and historical shell state are different products

The current foreground target answers:

`what can con safely act on right now?`

The last verified shell frame answers:

`what shell context did con last verify in this pane?`

Those are both valuable, but they are not interchangeable.

Examples:

- If con verified `remote_shell -> tmux -> shell` and the user then opened `nvim`, the current target must become `unknown` until the backend or a fresh probe proves more.
- If con still has the old shell frame, it should keep it as historical orientation for the model and the user, not as the live control target.

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

### Attachment graph

This graph answers:

`what protocol session is attached to what target?`

Suggested attachment kinds:

- `GhosttySurfaceAttachment`
- `GhosttyVtAttachment`
- `ShellPromptAttachment`
- `TmuxControlAttachment`
- `NeovimRpcAttachment`
- `OsPtyAttachment`

Every attachment should carry:

- `attachment_id`
- `target_id`
- `transport`
- `authority_level`
- `visibility_policy`
- `capabilities`
- `last_verified_at`
- `invalidates_on`

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
- `Unknown`

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

### 0. `ShellProbeAdapter`

Purpose:

- collect read-only shell-scope truth when a fresh prompt is proven

Examples:

- `$TMUX`
- `$SSH_CONNECTION`
- hostname
- tmux socket discovery
- `NVIM_LISTEN_ADDRESS`

Constraint:

- only valid at a proven shell prompt
- must not be promoted into visible-target truth once the pane leaves shell ownership

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
  -> PaneRuntimeTracker
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
- keep current pane tracker as the source for top-level runtime facts and causal history

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
