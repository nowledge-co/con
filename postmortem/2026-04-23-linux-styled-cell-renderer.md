# 2026-04-23 — Linux: styled-cell renderer over libghostty-vt

## What happened

The Linux terminal pane in `con-app/src/linux_view.rs` had been
rendering its PTY/VT state by reading
`LinuxGhosttyTerminal::read_screen_text(...)` and painting the result
as plain monochrome text. The libghostty-vt parser was already
producing styled cells with `fg`, `bg`, and `attrs` (bold, italic,
underline, strike, inverse), but those fields were thrown away by the
view layer.

Effects on a real Linux desktop:

- All shell prompts, `ls --color=always` output, vim/less/bat themes,
  spinners and progress bars rendered as monochrome white-on-black.
- Cursor position was invisible.
- Theme changes from the settings panel had no effect on the Linux
  pane because `LinuxGhosttyApp::update_appearance` and
  `update_colors` were no-ops on the VT side.

The Linux build also could not be produced from the cloud agent VM as
shipped: the workspace requires Rust edition 2024 (Cargo 1.85+), but
the base image only had Rust 1.83. Attempting `cargo build` failed with
"feature `edition2024` is required".

## Root cause

Two concrete gaps:

1. The Linux paint path was a placeholder text view that bypassed the
   parser's styled output. `read_screen_text` deliberately strips
   styling because it's the path the agent uses to produce model
   prompts; it was never the right input for the GPUI renderer.
2. The Linux backend never forwarded `TerminalColors` into
   `VtScreen::set_theme`, so unstyled cells fell back to the parser's
   internal default of `(0xCC,0xCC,0xCC)` on `(0,0,0)` regardless of
   the user-selected theme.

For the build environment, the cloud agent VM only carried Rust 1.83,
the GPUI Linux apt deps, and no Zig — none of which are sufficient for
con's Linux build today.

## Fix applied

Code changes (branch `cursor/linux-styled-cell-renderer-a6bd`):

- `con-ghostty/src/linux/pty.rs`:
  - `LinuxPtyOptions` carries an optional `TerminalColors` theme.
  - `LinuxPtySession::spawn` translates that into a `vt::ThemeColors`
    via `ThemeColors::from_ansi16` before constructing `VtScreen`.
  - New `snapshot()` and `set_theme()` methods so the view and the
    backend can read fresh `ScreenSnapshot`s and live-update the
    palette without restarting the PTY.
- `con-ghostty/src/linux/backend.rs`:
  - `LinuxBackendConfig` stores the current `TerminalColors`.
  - `LinuxGhosttyApp::new`, `update_colors`, `update_appearance`, and
    `default_pty_options` plumb the theme through into new sessions.
  - `LinuxGhosttyTerminal::update_appearance` now hands the colors to
    the live `VtScreen`, and a new `snapshot()` accessor exposes the
    parser snapshot to the GPUI view.
- `con-ghostty/src/lib.rs`:
  - Re-export `ScreenSnapshot`, `Cell as VtCell`, `Cursor as VtCursor`,
    and the `ATTR_*` constants so the Linux view in `con-app` doesn't
    need to peek into private modules.
  - `vt::Cell` derives `PartialEq` / `Eq` so the view can
    short-circuit re-renders when the snapshot is byte-identical.
- `con-app/src/linux_view.rs`:
  - Replaces the `Vec<String>` cache with a cached
    `Option<ScreenSnapshot>`.
  - Renders one `StyledText` per VT row, collapsing runs of cells with
    identical `(fg, bg, attrs, is_cursor)` into a single `TextRun`.
  - Decodes packed `0xRRGGBBAA` cell colors from the parser into GPUI
    `Hsla`. Alpha 0 is treated as "use the default" (matching the
    Windows pixel-shader contract documented in `vt.rs::read_cell`).
  - Honors bold (`FontWeight::BOLD`), italic (`FontStyle::Italic`),
    underline, strikethrough, inverse, and a cursor block by swapping
    fg/bg under the cursor cell.
- `docs/impl/linux-port.md`: phase 4 now reflects "styled cell paint
  landed; glyph-atlas grid renderer still pending", and the immediate-
  next-work list is updated.

Environment changes captured for future runs:

- Rust toolchain bumped to stable 1.95 via `rustup update stable` +
  `rustup default stable`.
- Installed apt deps required by `gpui_linux` and the libghostty-vt
  build:
  `libxcb-composite0-dev libxcb-dri2-0-dev libxcb-glx0-dev
  libxcb-present-dev libxcb-xfixes0-dev libxkbcommon-x11-dev
  libwayland-dev libvulkan-dev libfreetype-dev libfontconfig1-dev
  mesa-vulkan-drivers`.
- Installed Zig 0.15.2 to `~/.local/zig-x86_64-linux-0.15.2/` and
  added it to `PATH`. `cargo build -p con` then succeeds and produces
  a 314 MB debug binary in `target/debug/con` that launches against
  the cloud-agent X11 display, brings up a live `bash` PTY, exposes
  the control socket at `/tmp/con.sock`, and survives end-to-end
  `con-cli panes list / read / send-keys` traffic.

## Follow-on fixes after visual review

A Linux-desktop screenshot from the user surfaced two more
regressions that the cloud-VM smoke didn't catch (because both my
captures used the buggy build):

3. The terminal pane was rendering in the system fallback font, not
   IoskeleyMono. Root cause: `default_font_family` returns `"Ioskeley
   Mono"` (with space) but the embedded TTFs report
   `family = "IoskeleyMono"` (no space). macOS Core Text / Windows
   DirectWrite resolve both forms to the same font (forgiving family
   matching), but GPUI Linux's CosmicText backend does an exact
   `face.families.iter().any(|family| *name == family.0)` match and
   falls through to a proportional sans on miss. Fix: a Linux-only
   `canonical_terminal_font_family()` in `crates/con-app/src/theme.rs`
   that normalizes the display name to the registered family name
   before storing into `Theme::mono_font_family`. macOS / Windows
   stay byte-identical (the helper returns the input unchanged).
4. The Linux window stacked the native xfwm4 titlebar on top of the
   in-app top bar. Root cause: `default_window_decorations()` was
   pinned to `WindowDecorations::Server` on Linux. Fix: switched
   Linux to `WindowDecorations::Client`, gave it the same
   `TitlebarOptions` Windows uses, and extended `caption_buttons` in
   `workspace.rs` to also build on Linux. The X11 backend's
   `on_hit_test_window_control` is a no-op, so each Linux caption
   button gets an explicit `on_mouse_down` handler that calls
   `window.minimize_window()` / `zoom_window()` / `remove_window()`.
   The top-bar drag area already calls `start_window_move()` on
   Linux via `_NET_WM_MOVERESIZE`, which xfwm honors. The X11
   backend gracefully falls back to server decorations when no
   compositor is present, so the change is safe on minimal sessions.

## What we learned

- Hooking `read_screen_text` into a renderer is always wrong. The
  parser already maintains the only correct grid; the renderer's job
  is to display *its* cells, not to re-parse a sanitized text dump.
- The Linux backend should treat `TerminalColors` as part of the PTY
  contract, not a post-hoc visual tweak. Spawning the VT without a
  theme leaves an unbranded fallback palette baked into the first
  frame and the user can't tell whether the misrender is in the
  parser or the view.
- The cloud-agent VM is a perfectly serviceable Linux smoke
  environment once Rust 1.85+, the GPUI apt deps, Zig 0.15.2, and a
  software Vulkan ICD (`mesa-vulkan-drivers`) are present. We should
  recommend an env-setup agent that pre-installs these so future
  Linux work doesn't pay the ~3 min reinstall cost on every fresh VM.
- Cross-platform font name resolution is not as forgiving as it
  looks. Core Text / DirectWrite hide a real bug — `"Foo Bar"`
  silently resolves to a `"FooBar"`-named TTF — that the Linux text
  system doesn't paper over. When in doubt, the canonical name is
  whatever `fc-scan --format='%{family}\n'` reports for the file.
- Per-platform window-decoration choices need explicit visual proof.
  The Linux pane *worked* (control socket round-tripped, snapshot
  diffed, prompt moved) but also painted underneath a server-drawn
  titlebar that hid the real product chrome. Always screenshot the
  actual desktop session, not just the headless harness, before
  declaring a paint path "verified."
