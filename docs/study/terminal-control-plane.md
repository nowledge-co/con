# Study: Terminal Control Planes

## Executive Summary

There is no universal terminal equivalent of Chrome DevTools Protocol.

The terminal world is split across several narrower control surfaces:

- emulator embedding APIs
- VT state APIs
- shell integration
- multiplexer control protocols
- app-native RPC protocols
- OS and PTY process inspection

If con wants to become the terminal whose agent actually understands the terminal, it needs a layered control plane, not one guessed runtime snapshot.

## The Closest Thing To "CDP" In Terminals

### Emulator APIs

These are terminal-specific APIs exposed by the emulator itself.

Examples:

- Ghostty embedded C API
- libghostty-vt C API
- Kitty remote control
- iTerm2 proprietary escape codes

These are not universal. Each emulator exposes a different surface.

### Shell Integration

This is cooperation between the shell and the terminal.

Examples:

- OSC 133 semantic prompt markers
- OSC 7 working directory reporting
- shell startup wrappers and SSH compatibility env vars

This is useful, but it only describes shell state. It does not automatically describe the full-screen app now in front.

### Multiplexer Control

This is the strongest control plane for tmux-like layers.

Examples:

- tmux control mode
- zellij plugin and control surfaces

This is the closest terminal-world analogue to CDP for multiplexers: structured notifications, stable target ids, capture, and control commands.

### App-Native RPC

Interactive apps often have their own real control planes.

Examples:

- Neovim Msgpack-RPC
- future agent CLI sockets or explicit machine-readable markers

When available, this is much stronger than screen parsing.

### OS And PTY Inspection

This is the operating-system truth layer.

Examples:

- PTY child pid
- foreground process group
- process command line
- tty path
- SSH process ancestry

This is the best source for foreground runtime identity, but it is not a VT feature.

## What Ghostty Gives con Today

### Embedded C API

con currently embeds full Ghostty through the application and surface C API.

That gives us:

- app and surface lifecycle
- keyboard and mouse input forwarding
- clipboard integration
- live config updates
- visible and scrollback text reads
- child-exited state
- title updates
- PWD updates
- command-finished notifications from shell integration
- an inspector surface for humans

This is a strong embedding API. It is not a general runtime-inspection API.

### libghostty-vt C API

Ghostty also ships a separate VT library.

That gives access to terminal-emulator state such as:

- active primary or alternate screen
- title and PWD
- cursor position and terminal modes
- grid references
- cell and row semantic prompt metadata
- formatting and render-state APIs

This is closer to terminal state introspection.

But it still does not answer:

- what OS process currently owns the PTY foreground
- whether the user is on a remote host
- which tmux session/window/pane is active
- whether the visible app is Codex CLI, Claude Code, OpenCode, Neovim, or something else

### Internal Ghostty Components

Ghostty internally has richer building blocks than the current embedder API exposes.

Examples from the source:

- termio exec and PTY ownership logic
- shell integration management
- tmux control mode parser and viewer
- semantic prompt state

Those are valuable facts for con's direction, but today they are not a stable embedded control plane we can consume directly.

## What Ghostty Does Not Currently Give Us At The Embedded Boundary

For con's target product, the important missing facts are:

- authoritative PTY foreground process identity
- authoritative PTY foreground process group
- authoritative remote host identity
- authoritative active-screen and semantic-prompt export on the embedded surface path
- authoritative tmux session, window, and pane identity
- authoritative app identity for Neovim, Codex CLI, Claude Code, or OpenCode

That means a Ghostty surface is not enough, by itself, to behave like a browser target with CDP attached.

## Strong Control Surfaces Elsewhere In The Terminal Ecosystem

### tmux Control Mode

tmux control mode is the strongest protocol match to CDP in the terminal world.

It provides:

- structured notifications
- stable pane and window ids
- structured capture and layout queries
- structured command execution through tmux itself

This is exactly why Ghostty has a dedicated tmux parser and viewer internally.

For con, tmux control mode should be treated as a first-class adapter, not a special case of screen text.

### Shell Integration

Shell integration is not a full control plane, but it is still essential.

It provides:

- prompt boundaries
- command-finished events
- working directory updates
- shell-scope environment probes

It is the correct place to ask:

- are we at a shell prompt
- what does this shell think `$TMUX` is
- what does this shell think `$SSH_CONNECTION` is

It is not the correct place to claim foreground app truth after the shell has given control to tmux, Neovim, or another TUI.

### Emulator-Specific Protocols

Some terminals expose extra control planes of their own.

Examples:

- Kitty remote control
- iTerm2 proprietary escape codes and shell-integration metadata
- Ghostty application automation such as AppleScript

These are useful reference points, but they are emulator-specific rather than universal.

### App-Native RPC

If a foreground app has a real API, use it.

Examples:

- Neovim Msgpack-RPC
- future agent CLI control sockets

This is how con should think about editors and advanced TUIs: not as things to pattern match, but as attachable runtimes when the app cooperates.

## What This Means For con

con should not chase a fake universal protocol.

It should build its own control plane as a graph of typed adapters.

## Recommended Architecture

### 1. Runtime Graph

This answers:

`what nested runtimes are present?`

Examples:

- con pane
- local shell
- SSH connection
- remote shell
- tmux session
- tmux window
- tmux pane
- Neovim instance
- agent CLI instance

Each node should carry:

- target id
- kind
- address space
- parent relationship
- metadata
- freshness
- evidence
- capabilities

### 2. Control Attachments

This answers:

`what protocol or transport is attached to which target?`

This is the concept closest to CDP sessions.

Suggested attachment kinds:

- `GhosttySurfaceAttachment`
- `GhosttyVtAttachment`
- `ShellPromptAttachment`
- `TmuxControlAttachment`
- `NeovimRpcAttachment`
- `AgentCliAttachment`
- `OsPtyAttachment`

Each attachment should carry:

- attachment id
- target id
- transport
- authority level
- visibility policy
- capabilities
- last verified time
- invalidation rules

### 3. Evidence Tiers

con should treat every runtime claim according to its authority.

Suggested tiers:

- `BackendFact`
- `ProtocolFact`
- `ShellProbe`
- `ObservationHint`

Rules:

- only `BackendFact` and `ProtocolFact` can gate tools
- `ShellProbe` can orient and refine targeting
- `ObservationHint` can only influence cautious UX and agent instructions

### 4. Tool Families By Control Layer

con tools should be designed around control layers, not just `pane + command`.

Suggested families:

- terminal observation tools
- shell tools
- tmux tools
- editor tools
- local workspace tools

Examples:

- `read_pane_screen`
- `probe_shell_context`
- `tmux_list_targets`
- `tmux_capture`
- `tmux_send_keys`
- `tmux_exec`
- `nvim_eval`
- `nvim_apply_edit`

### 5. Explicit Attachment Lifecycle

For strong protocol adapters, con should model:

- discover
- attach
- verify
- use
- invalidate
- detach

That is a better long-term fit than ad hoc runtime inference.

### 6. Stateful Pane Tracker

Even with good adapters, con should not rebuild pane identity from a single screen snapshot.

Each pane needs a reducer-backed tracker that merges:

- the latest Ghostty observation frame
- the latest typed shell or protocol probe
- con-originated actions such as pane creation, visible shell exec, raw input, and process exit

This gives con an important advantage:

- current facts stay explicit
- recent causality stays available
- stale history can be invalidated cleanly

The product rule is:

- current runtime truth comes from fresh backend facts and fresh typed probes
- action history explains how the pane got here
- action history alone must never unlock shell exec, tmux control, or editor mutation

## Recommended Implementation Order

### Phase 1: Honest Shell-Scoped Probing

Build a shell probe adapter that only runs when a fresh shell prompt is proven.

Use it for:

- `$TMUX`
- `$SSH_CONNECTION`
- hostname
- pwd
- tmux socket and session discovery
- app-specific env vars such as `NVIM_LISTEN_ADDRESS`

This should be read-only and explicitly tagged as shell-scope evidence.

### Phase 1.5: Reducer-Backed Pane State

Before tmux-native control, con should persist shell probes and con-originated actions on each pane.

This phase should add:

- recent action history
- typed shell-context snapshots with freshness
- invalidation when a fresh shell prompt returns without a matching fresh probe
- prompt and `list_panes` exposure for both current truth and recent causal history

### Phase 2: tmux Query Adapter

When shell probes prove tmux is available, add a tmux query adapter.

Initial goal:

- query tmux session, window, and pane ids
- query `pane_current_command`
- query `pane_current_path`
- capture pane contents safely

This is the highest-value adapter after shell probing.

### Phase 3: tmux Control Attachment

Add an explicit tmux control attachment when con can prove that it is talking to the same tmux server.

This should enable:

- stable tmux target ids
- structured capture
- safe send-keys
- safe tmux-native command execution

### Phase 4: App-Native Attachments

Add adapters only where the app actually cooperates.

Start with:

- Neovim Msgpack-RPC

Treat Codex CLI, Claude Code, and OpenCode as app-attachment candidates only if:

- foreground process identity is proven, or
- the app intentionally advertises a machine-readable control endpoint

### Phase 5: Upstream Ghostty Surface Observability

The cleanest long-term path is upstream work in Ghostty.

High-value exports would be:

- surface access to active screen state
- surface access to semantic prompt state
- surface access to PTY child and foreground process identity
- surface access to tty and process metadata needed for attachable runtime inspection
- a stable bridge from embedded surfaces to libghostty-vt query APIs

## Product Rule

con should aim to feel like it has a CDP for terminals.

It should not pretend that the terminal ecosystem already has one.

The world-class solution is to build a layered control plane that:

- uses Ghostty for emulator truth
- uses tmux for tmux truth
- uses the shell for shell truth
- uses app RPC when the app offers it
- uses OS and PTY facts for process truth
- treats screen patterns as hints, never as authority

## Sources

- Ghostty embedded C API: `3pp/ghostty/include/ghostty.h`
- Ghostty embedded runtime: `3pp/ghostty/src/apprt/embedded.zig`
- Ghostty VT API: `3pp/ghostty/include/ghostty/vt.h`
- Ghostty VT terminal state: `3pp/ghostty/include/ghostty/vt/terminal.h`
- Ghostty VT screen semantics: `3pp/ghostty/include/ghostty/vt/screen.h`
- Ghostty tmux control implementation: `3pp/ghostty/src/terminal/tmux/control.zig`
- Ghostty tmux viewer: `3pp/ghostty/src/terminal/tmux/viewer.zig`
- Ghostty shell integration: `3pp/ghostty/src/shell-integration/README.md`
- Ghostty shell integration docs: https://ghostty.org/docs/features/shell-integration
- tmux control mode: https://github.com/tmux/tmux/wiki/Control-Mode
- kitty remote control: https://sw.kovidgoyal.net/kitty/remote-control/
- iTerm2 shell integration: https://iterm2.com/documentation-shell-integration.html
- Neovim API and RPC: https://neovim.io/doc/user/api.html
