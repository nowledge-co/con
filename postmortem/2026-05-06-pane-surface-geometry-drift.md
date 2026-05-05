# Pane Surface Geometry Drift

## What Happened

Issue #142 reported TUI text being clipped after working with panes and
pane-local surfaces. The same TUI looked correct after fully closing and
reopening Con, which pointed to state drift in the live layout path rather than
an application-level wrapping problem.

## Root Cause

Con keeps two pieces of geometry in sync on macOS:

- GPUI pane bounds, which decide where a pane is visible.
- Ghostty surface size, which decides the PTY rows and columns a TUI sees.

The hidden-surface fix from May 2 kept inactive surfaces resized while they
were not visible, but the focus path still treated surface activation mostly as
a visibility/focus change. If a surface's cached bounds matched the pane while
Ghostty's actual pixel size had drifted, `update_frame` could return early and
skip the resize that sends the corrected rows/columns to the TUI.

That explains the symptom: a TUI can keep rendering for a wider grid than the
visible pane, so text appears cut off until a restart creates every surface
from fresh layout.

## Fix Applied

- Surface focus now marks the active tab's native terminal layout as pending
  and notifies terminal views before changing native visibility.
- Re-focusing the already-active surface also runs this geometry checkpoint, so
  a user or orchestrator can repair a drifted pane by selecting the surface it
  is already using.
- Surface focus preserves Con's zoom/layout visibility sequencing. If focusing
  a surface clears a pane zoom, native visibility waits for the layout handoff
  instead of exposing a stale AppKit frame.
- `GhosttyView::update_frame` no longer trusts cached bounds alone. It also
  checks that Ghostty's embedded surface reports the backing pixel size implied
  by the current GPUI bounds before taking the fast early-return.

## What We Learned

Terminal size is part of the TUI protocol, not presentation. Any place that
activates a hidden live terminal must be treated as a geometry checkpoint, even
if the pane rectangle itself did not visibly change.

## Follow-Ups

- Add a control-plane E2E for `surfaces.create`, `surfaces.focus`, pane split
  resize, then `surfaces.list`, asserting all surfaces in the pane report the
  same grid size.
- Add a compact debug command or documented recipe for issue reports:
  `CON_GHOSTTY_PROFILE=1 RUST_LOG=con::perf=info,con=debug` plus
  `con-cli --json surfaces list`.
