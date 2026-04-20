# Implementation: Ghostty Rendering Pipeline

## Overview

con now runs a single terminal runtime: embedded Ghostty on macOS.

con does not maintain its own VT parser, PTY loop, scrollback grid, or canvas renderer anymore.
Ghostty owns terminal execution end to end. con owns:

- window, tab, and split layout
- AI harness integration
- pane metadata, context extraction, and product UI
- theme translation into Ghostty config

Important clarification:

- one `GhosttyApp` is shared across the whole con window
- each split is still its own Ghostty surface

That is not a temporary compromise. It matches Ghostty's own host-side architecture on macOS: native splits are modeled as a surface tree, not as one giant surface that renders every pane internally.

That split is intentional. Terminal correctness stays in Ghostty. Product intelligence stays in con.

## Runtime stack

```text
GPUI window
  └── GhosttyView
        ├── native NSView child
        ├── GhosttyApp          one per con window
        └── GhosttyTerminal     one per split surface
              ├── PTY + child process
              ├── VT parser + screen + scrollback
              ├── Metal renderer
              └── action callbacks
```

## Surface creation

`GhosttyView` creates a native `NSView` and passes it to `GhosttyApp::new_surface`.

We currently configure each surface with:

- `scale_factor`
- `font_size`
- `working_directory`
- `context = TAB`

Those values are part of `ghostty_surface_config_s` in libghostty's public C API.

## Split model

The correct long-term model is a host-managed Ghostty surface tree.

That means:

- Con should keep one shared `GhosttyApp` per window
- split creation should flow through Ghostty split actions
- the host still creates and owns each new surface view for each split

This is why "make the whole window one Ghostty instance" is the wrong mental model.
At the app level, con already does that. The remaining migration work is about replacing con-specific split semantics with Ghostty-driven surface-tree semantics.

## Data flow

1. User input enters GPUI.
2. `GhosttyView` forwards keyboard and mouse events into `ghostty_surface_*`.
3. Ghostty uses its embedded runtime wakeup callback to schedule `ghostty_app_tick()` on the macOS main queue. Con does not drive the core renderer from a fixed workspace polling loop anymore.
4. Ghostty writes input to the pane PTY, parses output, updates screen state, and renders into its Metal-backed view.
5. The host `NSView` and child surface `NSView` both use native autoresizing plus `NSViewLayerContentsRedrawDuringViewResize` so AppKit keeps the embedded layer responsive during live window resize.
6. Ghostty emits action callbacks such as title updates, PWD changes, and command-finished events.
7. `con-ghostty` stores those facts in `TerminalState` and bumps a wake generation after each wakeup-driven app tick.
8. Con's workspace-level housekeeping loop notices that generation change, drains surface-local state once for the window, and also handles deferred resize commits and one-shot init retries.
9. con reads `TerminalState` and visible text to build pane metadata for the agent, sidebar, and tab UI.

This distinction matters. Ghostty's embedded API is designed around wakeup-driven ticking, not "tick it every N milliseconds and hope that is close enough."

## What con reads from Ghostty

Today con relies on these Ghostty-facing facts:

- title
- current working directory
- recent command completion signal, exit code, and duration
- process alive / exited state
- visible screen text
- recent scrollback text
- selection text
- grid size

These are enough to support:

- honest pane summaries
- visible command execution for the built-in agent
- better tmux and TUI awareness
- theme and font propagation into new panes

## What con does not own anymore

The following responsibilities were removed from con:

- PTY creation and resize plumbing
- VTE parsing
- alternate-screen bookkeeping in a custom grid
- custom scrollback storage
- shell input suggestion overlays inside the terminal renderer
- GPUI text painting of terminal cells

This is a product win. Those paths were expensive to maintain and always weaker than the native Ghostty implementation.

## TerminalPane

`TerminalPane` is now a thin Ghostty-backed wrapper, not a multi-backend enum.

It exposes the capabilities that the rest of the app needs:

- lifecycle: focus, visibility, liveness
- visible text reads
- search
- command completion polling
- remote-host and pane-mode hints
- theme application

The point of `TerminalPane` is no longer backend abstraction.
It is product abstraction: one stable pane API for the workspace and agent layers.

One important integration detail: `GhosttyView` should not own its own perpetual 16ms GPUI polling loop. That duplicates host work once per pane and becomes visible during live resize. Con now keeps exactly one lightweight workspace pump for Ghostty-related housekeeping while leaving Ghostty's renderer itself wakeup-driven.

## Agent execution

The `terminal_exec` tool writes the command into the visible Ghostty pane.

Completion strategy:

- preferred: Ghostty `COMMAND_FINISHED`
- fallback: bounded recent-output capture after timeout

We no longer depend on a grid callback path.

## Design consequence

Ghostty is now the terminal runtime.

If con needs better pane intelligence, the right long-term move is:

1. consume stronger Ghostty facts when the C API exposes them
2. upstream missing observability hooks when needed
3. keep product inference in con's pane runtime observer

The wrong move is rebuilding another terminal engine beside Ghostty.
