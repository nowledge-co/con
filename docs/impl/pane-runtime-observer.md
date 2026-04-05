# Pane Runtime Observer

## Why this exists

A terminal pane is not a single process.

For serious workflows, the visible runtime is usually a stack:

1. local login shell
2. ssh client
3. remote shell
4. tmux or zellij
5. another shell
6. Codex CLI, Claude Code, OpenCode, vim, htop, less, or a long-running program

The current agent context model is still mostly a snapshot:

- title
- cwd
- recent output
- last command
- remote host hint
- a small amount of pane-mode inference

That is enough to avoid some bad mistakes, but it is not enough to answer the real question:

`what is this pane actually running right now, and how certain are we?`

This document defines the long-term architecture for that answer.

## Design goals

- Model the visible runtime of a pane, not just shell metadata.
- Separate backend facts from product inferences.
- Represent nested scopes explicitly.
- Preserve confidence and freshness with every claim.
- Work from Ghostty facts without rebuilding another terminal engine.
- Support external agent CLIs without hijacking them.
- Degrade honestly when the platform cannot provide strong evidence.

## Non-goals

- Perfect remote introspection without cooperation from the remote machine.
- Regex-based certainty about every TUI.
- Ghostty-specific hacks that pretend the C API exposes more than it really does.
- Hiding uncertainty from the user or the agent.

## Core rule

The system must never present shell-derived metadata as if it were the visible foreground runtime unless the evidence says that is still true.

## Architecture

The design is a three-layer model.

### Layer 1: Backend facts

These are raw observations with minimal interpretation.

Examples:

- terminal title
- OSC 7 pwd
- shell integration presence
- alternate-screen state
- command-finished events
- visible screen text
- future libghostty foreground-process exports

This layer answers:

`what did the backend actually observe?`

It does not answer:

`what app is definitely running?`

### Layer 2: Pane runtime observer

This is a stateful observer that consumes facts over time and produces a runtime model.

It is responsible for:

- evidence aggregation
- freshness tracking
- scope detection
- conflict resolution
- confidence scoring
- invalidation when the foreground runtime changes

This layer answers:

`given all recent facts, what runtime stack is most defensible?`

### Layer 3: Consumers

Consumers include:

- the built-in agent prompt
- `list_panes`
- tab and sidebar labels
- notifications
- approval UI
- future session restore and resume surfaces

Consumers must receive structured runtime state, not re-run their own heuristics on raw text.

## Data model

### `PaneObservationFrame`

An immutable observation snapshot emitted by a backend adapter.

Current implementation in con:

- `title`
- `cwd`
- `recent_output`
- `last_command`
- `last_exit_code`
- `last_command_duration_secs`
- `detected_remote_host`
- `has_shell_integration`
- `is_alt_screen`
- `is_busy`

Suggested fields:

- `pane_id`
- `observed_at`
- `backend`
- `pty_child_pid`
- `pty_foreground_pgid`
- `title`
- `pwd`
- `shell_integration`
- `command_finished`
- `alt_screen`
- `screen_excerpt`
- `screen_hash`
- `size`

### `Evidence`

Every non-trivial claim must carry evidence.

Suggested fields:

- `source`
- `observed_at`
- `strength`
- `freshness`
- `value`
- `note`

Suggested sources:

- `pty_foreground`
- `pty_child`
- `shell_integration`
- `ghostty_action`
- `screen_structure`
- `screen_text`
- `title`
- `cwd_artifact`
- `user_label`
- `manual_override`

### `PaneRuntimeState`

The durable observer output for one pane.

Current implementation in con:

- `mode`
- `shell_metadata_fresh`
- `tmux_session`
- `scope_stack`
- `warnings`

### `ScopeStack`

A pane should expose nested scopes instead of a single label.

Suggested scope kinds:

- `LocalShell`
- `SshConnection`
- `RemoteShell`
- `Multiplexer`
- `Shell`
- `InteractiveApp`
- `AgentCli`

Suggested app kinds:

- `Tmux`
- `Zellij`
- `Vim`
- `Neovim`
- `Less`
- `Htop`
- `Top`
- `Unknown`

Suggested agent CLI kinds:

- `Codex`
- `ClaudeCode`
- `OpenCode`
- `Unknown`

Example:

```text
[
  LocalShell(zsh),
  SshConnection(host=prod-2),
  RemoteShell(zsh),
  Multiplexer(kind=tmux, session=deploy),
  Shell(bash),
  AgentCli(kind=Codex)
]
```

That is the abstraction the product actually needs.

## Strong signals vs advisory signals

### Strong signals

These can justify high-confidence claims.

#### Ghostty action and screen facts

The public libghostty C API already gives us strong pane facts:

- title
- working directory
- command-finished events
- process-exited state
- visible text
- scrollback text

These are strong facts about terminal state.
They are not direct foreground-process identity.

#### Upstreamable runtime facts

Ghostty clearly owns the PTY and process group internally.
If con needs high-confidence local foreground-app identity, the durable path is to expose that through libghostty instead of rebuilding a parallel PTY stack in this repo.

That should be treated as an upstream API contract project, not a local shortcut.

#### Shell integration events

OSC 133 provides prompt and command lifecycle semantics.

Useful facts:

- whether the shell is active
- when a command started
- when it finished
- whether command metadata belongs to the visible runtime

This is strong for shell state, but it is not sufficient to identify the full nested scope stack by itself.

#### Alternate screen

Alternate-screen entry is a strong signal that the visible runtime is no longer an ordinary shell prompt.

It should invalidate shell-metadata freshness for visible-app claims.

### Advisory signals

These help with classification but must not create false certainty.

#### Screen structure

Useful examples:

- tmux status bar patterns
- boxed layouts
- agent CLI banners
- vim-like rulers

This is valuable, but it is inherently advisory.

#### Title

Useful for:

- `user@host`
- `tmux`
- session names
- explicit program titles

Titles are helpful but not authoritative.

#### Filesystem artifacts

Examples:

- `.claude/`
- `.opencode/`
- `AGENTS.md`

These can explain why a tool might be in use, but they do not prove the pane is running it.

## Backend adapters

## Ghostty backend

Ghostty currently gives us:

- title via action callback
- pwd via action callback
- command-finished via action callback
- visible and scrollback text via `ghostty_surface_read_text`
- selection access
- inspector handle

Important limit:

the embedded C API does not currently expose the same rich semantic prompt and runtime internals that Ghostty uses internally.

Important consequence:

we should not design the pane-runtime system around assumptions that Ghostty will tell us the exact foreground app or nested scope stack today.

Ghostty should feed Layer 1 facts. Layer 2 should remain a con-owned observer that merges those facts into a defensible runtime model.

## Ghostty-specific observations

Upstream Ghostty clearly maintains richer prompt semantics internally:

- semantic prompt state in `Screen.zig`
- prompt/output selection boundaries
- prompt-click movement
- command lifecycle handling in `stream_handler.zig`

But those semantics are not fully exported through the embedded C API today.

Also, Ghostty's OSC 7 handling validates the reported hostname against the local system before surfacing it as `PWD` state. That means `PWD` is not a durable source of remote host identity for embedded consumers.

This matters because a product design that depends on remote hostname coming from Ghostty `PWD` is structurally unsound.

Current con behavior reflects that limit:

- remote host identity is merged from pane-local evidence, not OSC 7 alone
- tmux status lines and pane titles can contribute advisory host hints
- when no evidence survives that merge, the runtime model keeps host as `unknown` instead of collapsing to `local`

## Probe design

The observer should run probes independently and merge their evidence.

### `GhosttyObservationProbe`

Purpose:

- build `PaneObservationFrame` from the embedded Ghostty surface
- keep title, cwd, command-finished, and screen excerpts synchronized
- expose only facts that libghostty actually exports today

### `ShellIntegrationProbe`

Purpose:

- track prompt-oriented metadata that Ghostty exposes indirectly today
- mark shell metadata freshness
- detect transitions back to the shell when strong shell evidence returns

### `TerminalSemanticProbe`

Purpose:

- consume backend-native signals such as command-finished, title updates, and future Ghostty semantic exports

### `ScreenStructureProbe`

Purpose:

- recognize tmux-like layouts, agent CLI chrome, and dense TUIs when stronger probes are unavailable

Constraint:

this probe is advisory only.

### `RemoteContextProbe`

Purpose:

- keep remote scope identity stable without overstating certainty
- distinguish remote-shell hints from truly proven foreground identity

Likely evidence:

- persistent SSH target
- title hints
- user-confirmed labels
- future explicit integration markers

### `ManualLabelProbe`

Purpose:

- allow the user to name or confirm a scope when the platform cannot prove it

Examples:

- "prod deploy tmux"
- "Codex on staging"
- "logs tail pane"

Manual labels should never overwrite facts. They should layer on top of them.

### `GhosttyObservabilityContract`

Purpose:

- define the next upstream libghostty exports con actually needs
- avoid rebuilding a parallel PTY/process introspection stack beside Ghostty

High-value future exports:

- foreground process identity
- alternate-screen state
- richer semantic prompt lifecycle
- explicit remote/runtime markers for embedded hosts

## Freshness and invalidation

This is the part cheap designs usually miss.

The observer must invalidate stale metadata aggressively.

Examples:

- When a pane enters alternate screen, shell cwd and last command become advisory for visible-app claims.
- When the foreground process group changes from `zsh` to `tmux`, shell prompt assumptions must be downgraded immediately.
- When the foreground process group returns to a shell and OSC 133 prompt markers resume, shell metadata can become fresh again.
- When the pane is inside `ssh`, local foreground identity is still useful, but remote runtime identity must be represented as lower-confidence unless supported by stronger evidence.

## Agent-facing contract

The built-in agent should not reason directly from raw title, cwd, and output.

It should receive a structured summary such as:

- `scope_stack`
- `active_scope_kind`
- `remote_host`
- `multiplexer_session`
- `agent_cli_kind`
- `shell_metadata_fresh`
- `screen_mode`
- `confidence`
- `warnings`

Example warning:

`Visible pane appears to be inside tmux. cwd and last_command may describe the underlying shell, not the visible program. Inspect the pane before making claims.`

## Product implications

This architecture is not only for prompts.

It also enables better product surfaces:

- clearer pane badges
- better sidebar names
- safer approval copy for remote operations
- accurate notifications from external agent CLIs
- reliable resume state when returning to a workspace

## Implementation plan

### Phase 1

- add `PaneObservationFrame`, `Evidence`, `PaneRuntimeState`, and `ScopeStack` types
- keep current pane-mode snapshot path as a temporary adapter into the new model

### Phase 2

- upstream or expose stronger libghostty runtime facts when needed
- record executable identity only from explicit backend contracts
- avoid shelling out in the hot path for pane identity

### Phase 3

- integrate backend adapters:
  - Ghostty surface adapter
- unify freshness rules across all Ghostty fact streams

### Phase 4

- add external-agent CLI classifiers based on strong evidence first, advisory screen evidence second
- expose runtime summaries in `list_panes` and agent context

### Phase 5

- add user-visible scope badges and manual labels
- use runtime scopes in approvals and notifications

## Testing strategy

- unit tests for evidence merge rules
- unit tests for freshness invalidation
- fixture-based tests for common scope stacks
- integration tests for:
  - local shell -> tmux
  - local shell -> ssh
  - local shell -> ssh -> tmux
  - local shell -> Codex CLI
  - local shell -> ssh -> tmux -> Claude Code

## What this avoids

This design avoids three long-term failures:

1. believing process-wide environment variables describe the focused pane
2. over-trusting shell metadata when a TUI has taken over the screen
3. scattering app-specific heuristics across prompts, tools, and UI labels

That is the standard required if con wants real credibility in SSH, tmux, and external-agent workflows.
