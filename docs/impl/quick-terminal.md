# Quick Terminal

## Overview

Con has a macOS-only **Quick Terminal**: a special Con window that can be shown or hidden with a global hotkey.

It is **not** a separate terminal subsystem. It is a normal `ConWorkspace` with a small amount of quick-terminal-specific lifecycle and AppKit behavior around it.

Current product behavior:

- global toggle via `Cmd+\\`
- lazily created on first toggle
- hidden by default because it does not exist until first toggle
- slides down from the top of the current screen
- uses full visible screen width
- defaults to half the visible screen height
- can be resized vertically from the bottom edge
- restores the previously frontmost app when hidden via the toggle path
- auto-hides when it loses key focus
- keeps its tabs / panes / shell state while hidden
- is destroyed when its last tab closes or its last shell exits
- is recreated fresh on the next toggle after destruction

## Product rules

1. Quick Terminal is a singleton **controller concept** within one running Con process.
2. The Quick Terminal window is **lazy**, not pre-created.
3. If the Quick Terminal window exists and is hidden, toggling shows the same live workspace again.
4. If the Quick Terminal window has been destroyed, the next toggle creates a **fresh** Quick Terminal.
5. Quick Terminal has no titlebar and no traffic-light window controls.
6. It is pinned to the top of the active screen and always uses the full visible width.
7. Width is not user-adjustable in practice; height is adjusted from the bottom edge.
8. The live AppKit frame is the source of truth for height while the window exists.
9. No extra persisted Quick Terminal geometry/config state is stored.
10. Layout-only tab UI changes must not trigger LLM tab-summary requests.

## Architecture

### Workspace model

Quick Terminal is a normal `ConWorkspace` marked with `is_quick_terminal`.

It reuses the standard workspace model for:

- tabs
- panes
- shell sessions
- cwd
- scrollback
- focus
- sidebar / top chrome behavior

There is no parallel session model.

### Rust controller

`crates/con-app/src/quick_terminal.rs` is intentionally small.

It owns only minimal runtime state:

- `raw_ptr`: native window pointer, if the Quick Terminal window currently exists
- `visible`: whether the existing Quick Terminal window is currently shown
- `return_pid`: previously frontmost app pid captured for restore-on-hide

Responsibilities:

- lazy creation on first toggle
- show/hide dispatch for an existing Quick Terminal window
- destruction-state reset when the Quick Terminal window is removed

It does **not** mirror tabs, panes, session contents, or geometry history.

### Native/AppKit layer

`crates/con-app/src/objc/quick_terminal_trampoline.m` owns macOS-specific window behavior:

- borderless + resizable configuration
- top-pinned full-width geometry normalization
- minimum-height clamp
- slide-in / slide-out animation
- Con activation on show
- restore of the previous app on toggle-hide
- auto-hide on `NSWindowDidResignKeyNotification`

A key constraint is that Quick Terminal must **not** replace GPUI's own native delegate chain. The final implementation keeps GPUI's window lifecycle intact and layers Quick Terminal behavior around it.

## Lifecycle

### Creation

Quick Terminal is created lazily.

When the global Quick Terminal hotkey fires and no Quick Terminal window exists:

1. Con loads the current config.
2. Con creates a fresh session with history and `HOME` as the default cwd on macOS.
3. Con opens a new special Con window via `open_quick_terminal(...)`.
4. The new window is marked as a Quick Terminal workspace.
5. The native window pointer is captured.
6. The window is configured and immediately shown with slide-in animation.

There is no eager startup creation anymore.

### Show

If the Quick Terminal window already exists and is hidden:

1. Capture the current frontmost app pid unless it is already Con.
2. Recompute the frame against the current visible screen.
3. Keep full visible width.
4. Keep the current live height, clamped to bounds.
5. Activate Con.
6. Make the Quick Terminal key/front.
7. Animate it downward from the top edge.

### Hide

If the Quick Terminal window exists and is visible, there are two hide paths.

#### Toggle hide

On global hotkey press:

1. Animate upward off-screen.
2. Keep the window alive.
3. Restore the previously frontmost app if one was captured.

#### Focus-loss auto-hide

When the user clicks elsewhere and the Quick Terminal resigns key:

1. Animate upward off-screen.
2. Keep the window alive.
3. Do **not** force app restore; macOS naturally activates what the user clicked.

### Destruction

Quick Terminal is destroyed when its live workspace is exhausted.

Destroy paths:

- closing the last tab
- last shell / pane exiting

In those paths Con:

1. hides the Quick Terminal window
2. clears the Quick Terminal controller state
3. closes the Quick Terminal workspace window

This means the next toggle creates a new Quick Terminal from scratch.

## Geometry

### Initial geometry

On creation:

- x = visible left edge
- width = visible screen width
- height = visible screen height / 2
- y = off-screen top position before slide-in

### Show-time normalization

Before each show, AppKit recomputes the frame from the current screen:

- force x to visible left
- force width to visible width
- reuse the current live height from the window frame
- if the current height is invalid, fall back to half-screen height
- clamp height to `[min_height, visible_screen_height]`
- pin the top edge to the visible top edge

### Resize behavior

User-facing behavior is intentionally simple:

- width remains full visible width
- top edge stays pinned
- bottom edge is the effective resize edge
- live window height is reused across hide/show while the window exists

Quick Terminal height is **not** persisted across destruction or across app relaunches.

## Session behavior

Quick Terminal preserves live state only while its window still exists.

### While hidden

If the user simply toggles it off, Quick Terminal keeps:

- cwd
- shell state
- tabs
- panes
- scrollback
- focus inside the workspace

### After destruction

If the last tab closes or the last shell exits:

- the Quick Terminal window is destroyed
- controller state is reset
- the next toggle creates a fresh Quick Terminal
- old cwd / shell state is not reused by Quick Terminal-specific logic

## Settings

User-configurable settings are:

- Quick Terminal enabled toggle
- Quick Terminal keybinding

These are reapplied at runtime when settings are saved.

During key recording in settings, both global hotkeys are suspended so Quick Terminal / Global Summon do not fire while recording a shortcut.

What is intentionally **not** stored:

- saved Quick Terminal height
- extra Quick Terminal geometry persistence fields
- mirrored Rust-side remembered height state
- cross-destruction Quick Terminal session memory

## Global Summon vs Quick Terminal

These are separate features.

### Quick Terminal

- global hidden terminal window
- toggle-oriented
- special borderless top-pinned window
- can preserve live terminal state while hidden
- destroyed when its last tab / shell exits

### Global Summon

- operates on the main Con window behavior
- toggles the main Con window in/out rather than using the special Quick Terminal window

The two hotkeys are registered independently.

## Tabs and summarizer behavior

Quick Terminal uses the same tab model as normal Con windows.

Important rule:

- content-driven changes may trigger tab summarization
- layout-only changes must not trigger tab summarization

In particular, switching horizontal/vertical tab presentation must not send LLM requests.

Related cleanup from this work:

- new-tab activation inside Quick Terminal was aligned with shared workspace activation flow
- top chrome refresh behavior was fixed so tab UI does not require a manual resize to appear

## Important implementation lessons

### 1. Do not replace GPUI's native delegate ownership

Earlier iterations interfered with GPUI's native lifecycle. The visible result was a Quick Terminal that appeared on screen but behaved as if input, top chrome, and focus were stalled until the user manually resized the window.

The final implementation keeps GPUI's delegate ownership intact.

### 2. Avoid duplicated workspace/session state machines

Earlier designs added extra concepts such as:

- eager creation
- reinitialize-on-last-tab-close
- mirrored remembered height
- extra creation state flags

These increased complexity without improving the product behavior.

The final model is simpler:

- if the Quick Terminal window exists, reuse it
- if it is destroyed, recreate it fresh

### 3. Reuse shared workspace behavior whenever possible

Diverging into Quick-Terminal-specific tab activation or layout behavior created rendering and focus regressions.

The final implementation works best when Quick Terminal reuses normal `ConWorkspace` flows and only adds a narrow lifecycle shell around them.

## File map

Primary files:

- `crates/con-app/src/main.rs`
  - Quick Terminal window creation entrypoint
  - Quick Terminal window options / initial bounds
- `crates/con-app/src/quick_terminal.rs`
  - singleton runtime controller
  - lazy create / show / hide / destroy-state reset
- `crates/con-app/src/global_hotkey.rs`
  - global hotkey registration and callback dispatch
- `crates/con-app/src/objc/quick_terminal_trampoline.m`
  - AppKit configure / animation / auto-hide behavior
- `crates/con-app/src/workspace.rs`
  - shared workspace behavior
  - destroy-on-last-tab / destroy-on-last-shell-exit logic

## Verification checklist

Manual verification should cover:

- app launch does **not** pre-create Quick Terminal
- first `Cmd+\\` lazily creates and shows Quick Terminal
- second `Cmd+\\` hides it
- showing again reuses the same live workspace if it was only hidden
- Quick Terminal focuses correctly on show
- toggle-hide restores the previously frontmost app
- clicking elsewhere auto-hides Quick Terminal
- default cwd is `HOME`
- full-width top-pinned appearance
- default half-screen height
- bottom-edge height resize works
- Quick Terminal keeps its session state across hide/show while alive
- add tab / `Cmd+T` works normally inside Quick Terminal
- tab UI renders without requiring a manual resize
- changing tab layout does not trigger LLM summary requests
- closing the last tab destroys Quick Terminal
- shell exit on the last pane destroys Quick Terminal
- after destruction, next `Cmd+\\` creates a fresh Quick Terminal

## Scope

This implementation is macOS-only.

Non-goals for now:

- persistent Quick Terminal geometry across app relaunches
- preserving Quick Terminal state after destruction
- a background daemon or resident no-window app mode
- non-macOS Quick Terminal behavior
