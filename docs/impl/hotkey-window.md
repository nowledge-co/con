# Hotkey Window

## Overview

Con now has a macOS-only hotkey window that behaves like a global hidden terminal for the current app run.

It is intentionally implemented as a **single special Con window**, not a parallel terminal subsystem.

Core behavior:

- global toggle via `Cmd+\\`
- created together with the first normal Con window
- starts hidden
- slides down from the top of the current screen
- full visible width
- default height is half the visible screen height
- can be resized vertically from the bottom edge
- keeps its terminal/session state while hidden
- can be configured as always-on-top
- restores the previously frontmost app when hidden
- disappears when the Con process exits

## Product rules

1. The hotkey window is a singleton within one running Con app instance.
2. It is pre-created with the first normal Con window, not lazily created on first toggle.
3. It starts hidden and is later controlled only by toggle show/hide.
4. It owns its own long-lived terminal session for the lifetime of the app run.
5. It has no titlebar and no traffic-light window controls.
6. It is pinned to the top of the current screen and always uses full visible width.
7. Width is not user-adjustable in practice; height is adjustable from the bottom edge.
8. The live AppKit window frame is the source of truth for current height during the app run.
9. No extra persisted hotkey-window geometry/config state is required.
10. Layout-only tab UI changes must not trigger LLM tab-summary requests.

## Final architecture

### Workspace model

The hotkey window is a normal `ConWorkspace` with hotkey-specific creation and window behavior.

There is no second terminal model and no duplicate session model.

This means the hotkey window naturally keeps:

- cwd
- shell state
- scrollback
- panes
- tabs
- focus state inside the workspace

for as long as the app process remains alive.

### Rust controller

`crates/con-app/src/hotkey_window.rs` is intentionally thin.

It owns only runtime singleton control state:

- async app handle
- native window pointer
- visible/hidden flag
- previously frontmost app pid for restore-on-hide
- runtime always-on-top application

It does **not** own mirrored terminal/session/geometry state.

### Native/AppKit layer

`crates/con-app/src/objc/hotkey_window_trampoline.m` owns the macOS-specific window behavior:

- borderless configuration
- slide-in / slide-out animation
- app activation on show
- app restore on hide
- top-pinned geometry normalization
- full-width enforcement
- minimum-height clamp

A key implementation detail is that the hotkey layer must **not replace GPUI's own `NSWindowDelegate`**. Earlier versions did this and broke GPUI's activation / focus / layout lifecycle, which caused the hotkey window to appear visually present but behave as if input and top chrome were stalled until a manual resize happened.

The final implementation keeps GPUI's delegate chain intact.

## Lifecycle

### Creation

When the first normal Con window opens:

1. Con opens the requested normal window.
2. Con also opens the singleton hotkey window.
3. The hotkey window uses `HOME` as its default cwd.
4. The hotkey window is immediately configured as hidden.

There is no `pending_show`, `creating`, or first-toggle creation flow in the final design.

### Show

On global hotkey press, if the hotkey window is hidden:

1. Capture the current frontmost app pid unless it is already Con.
2. Recompute the window frame against the current screen.
3. Force full visible width.
4. Keep the current window height, clamped to screen bounds.
5. Activate Con.
6. Make the hotkey window key/front.
7. Animate it downward from the top edge.

### Hide

On global hotkey press, if the hotkey window is visible:

1. Animate it upward off-screen.
2. Keep the window alive.
3. Restore the previously frontmost app if one was captured.

### Destruction

The hotkey window lives only as long as the Con app process lives.

When the app exits, the hotkey window exits too.

## Geometry

### Initial geometry

On creation:

- x = visible left edge
- y = top-pinned shown position
- width = visible screen width
- height = visible screen height / 2

### Show-time geometry normalization

Before each show:

- recompute current screen visible bounds
- force x to visible left
- force width to visible width
- reuse the current live height
- clamp height into valid screen bounds
- place y so the top edge stays pinned

This makes the hotkey window adapt automatically to display changes without extra persistence machinery.

### Resize behavior

User-facing resize behavior is intentionally simple:

- width stays aligned to full visible width
- top edge remains pinned
- effective resize happens on the bottom edge
- height change stays in the live window frame

We deliberately do **not** persist hotkey height across launches.

## Settings

User-configurable settings remain:

- hotkey-window enabled toggle
- hotkey-window keybinding
- hotkey-window always-on-top toggle

These settings are reapplied at runtime when settings are saved.

What is intentionally **not** stored anymore:

- saved hotkey window height
- extra hotkey geometry persistence fields
- mirrored Rust-side remembered height state

## Tab and summarizer behavior

The hotkey window uses the same tab model as normal windows.

Important rule:

- content-driven changes may trigger tab summarization
- layout-only changes must not trigger tab summarization

In particular, switching horizontal/vertical tab presentation must not send LLM requests.

During this work, `new_tab` behavior was also tightened so new-tab activation reuses the shared tab activation flow instead of maintaining a separate hotkey-only tab-switch path.

## Important implementation lessons

### 1. Do not replace GPUI's window delegate

This was the most important bug discovered during implementation.

Replacing the native window delegate caused GPUI to miss key activation / resize / frame callbacks. The visible symptom was that the hotkey window appeared, but buttons, shortcuts, focus, and top chrome behaved as if they were stalled until a manual resize occurred.

The correct fix was to keep GPUI's delegate ownership intact.

### 2. Avoid duplicated state machines

Intermediate versions introduced extra state such as:

- lazy creation
- pending show
- creating flags
- mirrored height memory

These made the system harder to reason about and were ultimately unnecessary.

The final design is simpler because the live window and workspace are the primary sources of truth.

### 3. Reuse shared workspace behavior

When hotkey behavior diverged into custom tab activation / synchronization code, regressions appeared in focus and tab rendering.

The final design prefers reusing existing `ConWorkspace` flows and only adding a narrow hotkey-specific shell around them.

## File map

Primary files:

- `crates/con-app/src/main.rs`
  - eager hotkey-window creation
  - hotkey-window options / initial bounds
- `crates/con-app/src/hotkey_window.rs`
  - singleton runtime controller
- `crates/con-app/src/global_hotkey.rs`
  - hotkey registration and callback dispatch
- `crates/con-app/src/objc/hotkey_window_trampoline.m`
  - AppKit show/hide/configure behavior
- `crates/con-app/src/workspace.rs`
  - shared workspace/tab behavior used by the hotkey window

## Verification checklist

Manual verification should cover:

- open normal Con window → hotkey window is pre-created hidden
- `Cmd+\\` toggles show/hide
- hotkey window focuses correctly on show
- hide restores the previously frontmost app
- default cwd is `HOME`
- full-width top-pinned appearance
- default half-screen height
- bottom-edge height resize works
- hotkey window keeps its session state across hide/show
- always-on-top setting applies correctly
- add tab / `Cmd+T` works normally inside the hotkey window
- tab UI renders without requiring a manual resize
- changing tab layout does not trigger LLM summary requests

## Current scope

This implementation is macOS-only.

Non-goals for now:

- cross-launch hotkey session persistence beyond existing Con session behavior
- persistent geometry restoration across app relaunches
- a background daemon or app-without-window resident mode
- non-macOS hotkey window behavior
