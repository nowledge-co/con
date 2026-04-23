Pushed `cursor/linux-styled-cell-renderer-a6bd` (PR #58 / 4cea592).

What changed:

- Linux pane now reads `LinuxGhosttyTerminal::snapshot()` (a fresh
  `libghostty-vt` `ScreenSnapshot`) instead of the lossy
  `read_screen_text` path. Each VT row renders as one GPUI
  `StyledText` with one `TextRun` per styled span, collapsing runs of
  cells that share `(fg, bg, attrs, is_cursor)`.
- SGR colors, bold (`FontWeight::BOLD`), italic (`FontStyle::Italic`),
  underline, strikethrough, and inverse all survive into the visible
  Linux pane. Cell colors decode from the parser's `0xRRGGBBAA`
  packing into `Hsla` (alpha=0 means "use default", matching the
  Windows pixel-shader contract documented in `vt.rs::read_cell`).
- Block cursor: when libghostty-vt reports the cursor as visible the
  cell under it gets fg/bg swapped, and the trailing-blank trim is
  suppressed when the cursor sits in trailing space so it stays
  painted at the end of the line.
- `LinuxGhosttyApp` / `LinuxGhosttyTerminal` now plumb
  `TerminalColors` (foreground / background / 16-color ANSI palette)
  into `VtScreen::set_theme`, both at session spawn (via
  `LinuxPtyOptions.theme`) and live via `update_appearance` /
  `update_colors`. Theme switches in the settings panel now actually
  take effect on the Linux pane.
- Added `linux_view::tests` covering the row renderer (with and
  without cursor) and the color-alpha decoder. All 12 `con` tests +
  5 `con-ghostty` tests pass under `RUSTFLAGS="-D warnings"`.

Cross-platform safety:

- Only two shared files touched: `con-ghostty/src/lib.rs` (six new
  lines, all `#[cfg(target_os = "linux")]`-gated re-exports) and
  `con-ghostty/src/vt.rs` (a single `PartialEq, Eq` derive added on
  `vt::Cell`, a `#[repr(C)]` POD struct that's only compiled on
  Windows + Linux).
- macOS modules (`terminal.rs`, `ffi.rs`, `objc/`, `ghostty_view.rs`)
  are byte-for-byte unchanged. `pub mod vt` itself is
  `#[cfg(any(target_os = "windows", target_os = "linux"))]` — macOS
  never sees the `Cell` struct or the new derive.
- `con-ghostty` cross-checks clean for `--target x86_64-pc-windows-msvc`
  with `CON_SKIP_GHOSTTY_VT=1`.

Verification on a native-ish Linux desktop:

- Bootstrap on a fresh Ubuntu 24.04 cloud VM with Rust 1.95 (workspace
  needs edition 2024 / Cargo 1.85+), Zig 0.15.2 to `~/.local/`, the
  GPUI Linux apt deps the CI job already installs, plus
  `mesa-vulkan-drivers` for a software ICD.
- `cargo build -p con` produces a 314 MB debug binary in
  `target/debug/con`. Launches against the headless X11 display,
  brings up its WGPU/Vulkan surface on llvmpipe, and binds the
  control socket at `/tmp/con.sock`.
- End-to-end check via `con-cli`: `panes list` reports the bash pane,
  `panes send-keys` + `panes read` confirm `pwd`, `ls --color=always`,
  `printf "\033[31m..."` etc. all execute and the styled output is
  consumed by the parser; the GPUI view repaints the styled cells
  between each `cx.notify()`.

Plan doc + postmortem updates:

- `docs/impl/linux-port.md` — phase 4 status updated to "styled
  `StyledText` paint landed; glyph-atlas grid renderer pending"; the
  immediate-next-work list now points at "build the real glyph-atlas
  renderer that paints per-cell backgrounds + pre-rasterized glyphs"
  (matching the D3D11/DirectWrite path on Windows).
- `postmortem/2026-04-23-linux-styled-cell-renderer.md` — full writeup
  of the change, the env bootstrap, and the lessons learned.

What's still not complete after this PR (carry-over for phase 5/6):

- glyph-atlas grid renderer with per-cell metrics and GPU instancing
  (today's view still shapes text at layout time)
- mouse reporting + selection on Linux
- desktop-environment validation on Wayland and X11 native sessions
  (not just llvmpipe-on-Xvfb)
- packaging (`.deb` / AppImage / Flatpak)

## Visual verification on a Linux desktop session

Verified on the cloud-agent VM's actual XFCE desktop session (not
just the headless `con-cli` round-trip). The XFCE compositor,
`xfwm4` window manager, and real `xfdesktop` panel are running on
`:1`; con joins as a regular client window with **client-side
decorations** (the GPUI top bar replaces xfwm's frame). Captures
taken via `xwd` → `convert`:

`screenshots/2026-04-23-fresh-launch.png` — fresh launch, empty
shell. The Flexoki Dark terminal pane shows the live `~ $` bash
prompt and a solid dark **block cursor** sitting after the `$`,
rendered in **IoskeleyMono** (proper monospace cell grid). The con
top bar paints across the full window width, with the right-side
caption cluster (sidebar / AI / settings + minimize / maximize /
close) matching the Windows beta layout. No xfwm4 titlebar above
the GPUI window.

`screenshots/2026-04-23-styled-output.png` — after pushing styled
output via the control socket. Confirms in one frame:

- `printf "...\033[31mred \033[32mgreen \033[34mblue
  \033[1;33mbold-yellow \033[4mund\033[0m\n"` paints "red" in red,
  "green" in green, "blue" in blue, "bold-yellow" in bolded yellow
  (visibly heavier weight than the surrounding text), "und" in
  yellow with an underline beneath it.
- `ls --color=always /etc | head -6` paints `alternatives`,
  `apparmor.d`, `apport`, `apt` in the dircolors directory blue
  and `bash.bashrc` in the default foreground (a plain file).
- Block cursor is parked on the next prompt line.
- Every monospace cell aligns vertically, confirming the
  IoskeleyMono lookup actually resolves on Linux.

So the styled-cell paint path, the IoskeleyMono shaping, and the
client-side titlebar are all proven on a real Linux desktop session.
The remaining honest caveat for native verification: GPU is
`llvmpipe` (software Vulkan) and the desktop session is XFCE on a
cloud VM. Validation on a hardware-accelerated Wayland or X11
session, and on multiple desktop environments (GNOME, KDE), is
still useful before the Linux build comes off "in progress" on the
tracker.

## Follow-up fixes after first review

Two regressions surfaced when the styled-cell PR was reviewed
against a Linux desktop screenshot (not the cloud-VM capture):

1. **Terminal pane was rendering in the system fallback font, not
   IoskeleyMono.** The workspace ships with
   `default_font_family = "Ioskeley Mono"` (with a space). macOS
   Core Text and Windows DirectWrite happily resolve that to the
   embedded TTF (whose `name` table reports `family =
   "IoskeleyMono"`, no space). GPUI Linux's CosmicText backend
   does an exact `face.families.iter().any(|family| *name ==
   family.0)` match — `"Ioskeley Mono"` misses, the lookup falls
   through to a proportional sans, and the terminal cells stop
   aligning. Fix lives in `crates/con-app/src/theme.rs` as
   `canonical_terminal_font_family()`: a Linux-only normalization
   that maps `"Ioskeley Mono"` → `"IoskeleyMono"` before storing
   into `Theme::mono_font_family`. macOS / Windows behavior is
   byte-identical to before.
2. **Linux window had the native xfwm4 titlebar stacked on top of
   the in-app top bar.** Linux was opting into
   `WindowDecorations::Server`, so xfwm drew a real titlebar
   above the GPUI window. Switched to
   `WindowDecorations::Client` in `default_window_decorations`,
   added a `TitlebarOptions` (matching what Windows already
   passes), and extended the existing `caption_buttons` cluster
   in `workspace.rs` to also build on Linux. The X11 backend's
   `on_hit_test_window_control` is a no-op, so each Linux button
   gets an explicit `on_mouse_down` handler that calls
   `window.minimize_window()` / `zoom_window()` / `remove_window()`
   directly. The top-bar drag area already calls
   `start_window_move` on Linux via `_NET_WM_MOVERESIZE`, which
   xfwm honors.

Both fixes are visible in the new screenshot pair.
