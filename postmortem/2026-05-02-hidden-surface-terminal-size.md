# Hidden Surface Terminal Size

## What Happened

Pane-local surfaces allowed orchestrators to keep several live terminal sessions
inside one visible pane. When a TUI coding agent was launched in an inactive
surface, it could observe a bootstrap or stale terminal size because only the
active surface was rendered and resized by the pane layout.

## Root Cause

The public pane/surface model was correct, but the rendering path only
materialized layout for the visible active surface. Hidden surfaces kept
running, yet their terminal backend did not receive the host pane's latest
pixel bounds and grid size until the user focused that surface.

## Fix Applied

The pane renderer now captures the active terminal host bounds and synchronizes
every inactive surface in the same pane to those bounds. Each platform backend
keeps the sync cheap:

- macOS updates the embedded Ghostty frame without changing visibility.
- Windows updates pane bounds and only refreshes render state when bounds
  changed or the session was not initialized yet.
- Linux syncs the PTY-backed surface size and notifies only on change.

Human rename was also added through the normal action system, so users can name
surfaces from Command Palette, app menu, terminal context menu, or by
double-clicking a surface tab.

## What We Learned

If a hidden terminal session is still live, its layout contract must remain live
too. "Not rendered" cannot mean "not resized" for TUI applications, because
rows/columns are part of the application protocol, not just presentation.

## Follow-Ups

- Add a control-plane E2E that creates multiple surfaces in one pane, resizes
  the pane, and asserts the inactive surface reports the same grid size after
  focus.
- Keep the built-in agent harness and benchmarks on the pane/active-surface
  contract unless a test explicitly targets `surfaces.*`.
