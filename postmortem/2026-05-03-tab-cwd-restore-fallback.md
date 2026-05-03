# Tab cwd lost on restore when OSC 7 never fires

## What happened

After PR #113 shipped layout-only workspace + session restore (tab/pane/surface
text), a user reported that tab names came back across restarts but the
"previously opened path" did not — every restored tab landed in whatever
directory `con` was relaunched from rather than the directory the tab was
originally opened in.

## Root cause

`snapshot_session` reads each surface's cwd from
`GhosttyView::current_dir()`, which in turn read **only** the live PWD
reported by libghostty's `OSC 7` action callback (`crates/con-ghostty/src/terminal.rs:955`).
Two side-effects:

1. **macOS without shell integration.** The default macOS zsh sets the
   terminal title via OSC 0/2 (the user sees the title save fine) but
   never emits OSC 7. `current_dir()` returns `None`, the snapshot
   stores `cwd = None`, and restore launches the new pane with no
   explicit cwd.
2. **Windows.** `windows_view::GhosttyView::current_dir()` was
   hard-coded to `None`. Linux already had a fallback to `initial_cwd`
   in `linux_view.rs:211` — macOS and Windows did not.

Even when shell integration was active, the *first* tab's cwd was
sampled before the shell had a chance to print its first prompt, so
the very first session save still recorded `cwd = None`. Restoring
into a different cwd (e.g. relaunching `con` from `~`) then opened
the tab there instead of in the project directory the user had
originally launched `con` from.

## Fix

Two small, defensive changes in `crates/con-app/`:

* `ghostty_view::current_dir` (macOS), `windows_view::current_dir`,
  and `stub_view::current_dir` now fall back to the view's
  `initial_cwd` whenever the live OSC 7 PWD is missing — matching
  the Linux backend's existing behaviour.
* `make_ghostty_terminal` (the single shared factory used by every
  new-pane / restore / duplicate-tab call site) now defaults
  `cwd = None` to `std::env::current_dir()` before handing it to
  `GhosttyView::new`. The shell would have inherited that cwd anyway;
  recording it explicitly is what makes restore deterministic.

Net effect: tab cwd is now always *some* concrete path — the live
shell PWD when shell integration is wired up, otherwise the
launch-time cwd of the tab. Restores are stable across relaunches
from a different parent directory.

## What we learned

- Save-time snapshots must not rely on best-effort terminal protocol
  signals (OSC 7) for fields whose absence breaks user-visible
  restore. Fall back to the deterministic launch parameter.
- Per-platform view backends need parity audits when we extend
  persistence — the Linux view already had the right fallback;
  macOS and Windows had silently diverged.
