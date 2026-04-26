# Linux render-latency follow-up

This note mirrors the tracker updates from PR #65 and PR #68 after the
Linux preview shipped.

## What landed

- PR #65 cached per-row `StyledText` text/runs in the preview renderer.
  Steady-state updates now rebuild VT-dirty rows plus cursor-affected
  rows instead of reconstructing the whole visible grid every tick.
- PR #68 changed Linux PTY output to wake the terminal view directly.
  The workspace idle poll is no longer the intended wake path for normal
  shell output.
- Shared `CON_GHOSTTY_PROFILE` instrumentation now logs `vt_snapshot`
  timing for Linux and Windows through the same path.
- The final Linux stale-screen regression after exiting `htop` / `vim`
  was fixed by treating each new VT generation as a visible-row cache
  refresh boundary in the Linux preview renderer, while row containers
  still clear across the full pane width.

## Tracker status

The current Linux preview renderer is now suitable for near-term runtime
testing, but it is still the temporary GPUI `StyledText` row renderer.
The long-term renderer remains a GPUI-owned glyph-atlas grid renderer so
Linux can avoid layout-time shaping as the hot path and match the fixed
cell-grid architecture used by the Windows D3D11/DirectWrite renderer.

Native Linux performance smoke should continue to cover:

- `ls` / large command-output bursts
- `htop` and `nvim` alternate-screen enter/exit
- row clearing after TUI exit
- Wayland and X11 desktop sessions, especially hardware-accelerated KDE
  and GNOME

## Remaining work

- Linux glyph-atlas grid renderer.
- Mouse reporting, selection, scrollback gestures, and clipboard polish.
- Native desktop validation across major Wayland/X11 environments.
- Native package format decision (`.deb`, AppImage, Flatpak, etc.) on
  top of the existing tarball + one-line installer path.
