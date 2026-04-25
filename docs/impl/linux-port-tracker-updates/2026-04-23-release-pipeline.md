Linux preview shipped — milestone landing summary
=================================================

The Linux preview milestone landed in two PRs:

- **#58** (`cursor/linux-styled-cell-renderer-a6bd`, merged) —
  the runtime backend.
- **#59** (`cursor/linux-release-pipeline-a6bd`, in review) —
  one-liner installer + tarball release pipeline + notify-only
  auto-update.

What ships now
--------------

The same `con` binary that runs on macOS and Windows now opens on
Linux with a real terminal pane:

- **Real PTY pane** via Unix PTY + `libghostty-vt` (the shared
  parser the Windows backend already uses).
- **Per-row styled-cell paint** in the GPUI window. Each VT row
  renders as one `StyledText` with one `TextRun` per styled span,
  collapsing runs of cells that share `(fg, bg, attrs, is_cursor)`.
  SGR colors, bold (`FontWeight::BOLD`), italic
  (`FontStyle::Italic`), underline, strikethrough, inverse, and a
  block cursor (fg/bg swap on the cursor cell, post-inverse-aware so
  htop's selected row + vim status lines stay legible) all survive
  into the visible pane. Cell colors decode from the parser's
  `0xRRGGBBAA` packing into GPUI `Hsla` (alpha=0 means "use the
  default", matching the Windows pixel-shader contract documented
  in `vt.rs::read_cell`).
- **Theme palette synced from settings**:
  `LinuxGhosttyApp::update_appearance` / `update_colors` now plumb
  `TerminalColors` (foreground / background / 16-color ANSI palette)
  into `VtScreen::set_theme` both at session spawn (via
  `LinuxPtyOptions.theme`) and live from settings.
- **IoskeleyMono shaping**: a Linux-only
  `canonical_terminal_font_family()` normalizes the user-facing
  display name `"Ioskeley Mono"` to the registered TTF family
  `"IoskeleyMono"` (also case- and whitespace-insensitive after
  the #59 review-feedback round) so GPUI's `CosmicTextSystem`
  resolves the embedded font instead of falling back to a
  proportional sans. macOS / Windows stay byte-identical.
- **Client-side decorations**: `WindowDecorations::Client` plus the
  shared `TitlebarOptions` and the `caption_buttons` cluster
  (minimize / maximize / close) extended to also build on Linux.
  The X11 backend's `on_hit_test_window_control` is a no-op, so
  each Linux caption button gets an explicit `on_mouse_down`. The
  Close button routes through `prepare_window_close` (cancel agent
  sessions, flush state, drop pending control-request responses)
  before `remove_window` — same shutdown semantics the macOS /
  Windows close button uses via `on_window_should_close`.
- **Transparent ARGB window with rounded corners**:
  `supports_transparent_main_window()` returns `true` on Linux,
  GPUI's X11 backend opens against an ARGB depth-32 visual, the
  workspace root clips to a 14 px corner radius (matching Win11
  Mica's perceived radius), and the existing per-pane terminal
  opacity + per-surface UI opacity sliders composite through to
  the desktop now that the Root background is transparent.
- **Backdrop blur where the compositor exposes it**: KDE Plasma
  Wayland gets real `org_kde_kwin_blur` Gaussian blur via
  `WindowBackgroundAppearance::Blurred`. X11 / mutter / sway have
  no app-driven backdrop-blur protocol — the toggle still ships
  there but the visible result is transparency only (documented
  in `set_linux_window_blur`'s rustdoc).
- **Snappy paint pipeline**: removed a redundant per-tick
  snapshot refresh and a 50–200 KB `Vec<Cell>` deep-equality
  compare from the snapshot path (replaced with a generation-counter
  compare), and tightened the workspace idle poll loop from 16 ms
  to 8 ms. Measured: keystroke-echo round-trip via the control
  socket dropped from 32.6 ms → 16.6 ms mean across 5 runs.
- **No more "Waiting for shell prompt…" flash on alt-screen TUIs**.
  A `seen_any_output` latch on the view fixes the placeholder
  showing every time `htop` / `vim` / `less` / `fzf` cleared the
  screen via the alternate-screen switch and hadn't drawn their
  UI yet.
- **One-line installer** (PR #59):
    `curl -fsSL https://con-releases.nowledge.co/install.sh | sh`
  Same one-liner macOS uses; `install.sh` is now a Unix dispatcher
  that detects the host OS and routes Darwin to the existing DMG
  flow or Linux to a new tarball flow that drops `con` into
  `~/.local/bin`, registers a `.desktop` launcher entry, and
  installs the 256×256 hicolor icon. No sudo.
- **Notify-only in-app updater** (PR #59): polls the same
  Sparkle-shaped appcast XML as the Windows backend at
  `https://con-releases.nowledge.co/appcast/{channel}-linux-x86_64.xml`,
  surfaces "Update now" in Settings → Updates, and re-runs
  `install.sh` on apply with the appcast-pinned version
  (`CON_INSTALL_VERSION`) so beta-channel users never get silently
  downgraded to stable when GitHub's `/releases/latest` skips
  prereleases.

Verified end-to-end
-------------------

Cloud-VM XFCE :1 session (xfwm4 / xfdesktop / xfce4-panel,
software Vulkan via llvmpipe):

- Full launch flow including `xprop` confirming
  `_MOTIF_WM_HINTS = 0x2,0,0,0,0` (CSD active) and
  `_NET_FRAME_EXTENTS = 0,0,0,0` (no server frame).
- Styled output (red/green/blue/bold-yellow/underlined-und/inverse
  via printf, dircolors blue ls, full-row inverse highlights).
- htop in the alternate screen (selected-row inverse cursor
  visible, F-key footer, real /proc table).
- Real Linux release pipeline against a local fake-release HTTP
  server: `release.sh` produced a 39 MB tarball; install.sh
  installed it; the running binary's notify-only updater fetched
  the local appcast, parsed the version, transitioned LATEST to
  `UpdateAvailable`, and `apply_update_in_place` ran install.sh
  with the appcast-pinned version. The installed binary's sha
  matched the appcast-pointed tarball byte-for-byte.

Five screenshots were captured during validation (fresh launch,
styled output, rounded transparent window over wallpaper + xterm,
htop alt-screen blank moment, htop fully painted) and posted to
the related GitHub issue / PR — they're not committed to the repo
since `docs/**/*.png` is gitignored.

Honest caveats
--------------

- GPU is `llvmpipe` (software Vulkan) and the desktop is XFCE on
  a cloud VM. Hardware-accelerated Wayland / X11 native sessions
  on multiple desktop environments (GNOME, KDE) is still the
  next useful step before "preview" comes off this row on the
  tracker.
- Real backdrop blur only renders on KDE Plasma Wayland. On X11
  / mutter / sway the toggle ships but the visible result is
  transparency only.
- The long-term GPUI-owned glyph-atlas grid renderer (matching
  the D3D11/DirectWrite path Windows uses) is the next backend
  step. Today's per-row `StyledText` paint covers SGR + bold/
  italic/underline/strike/inverse + block cursor correctly, but
  per-cell metrics still come from layout-time text shaping.

What's next (phase 5/6)
-----------------------

- Glyph-atlas grid renderer.
- Mouse reporting (button + wheel), selection (drag → SGR 1006 /
  X10 reports + clipboard integration). DECCKM and bracketed
  paste are already wired through `libghostty-vt` mode tracking.
- Hardware-accelerated native-desktop validation pass.
- Packaging: desktop entry, icon integration, `.deb` / AppImage
  / Flatpak.

Plan + postmortem
-----------------

- `docs/impl/linux-port.md` (plan, marked phase 4 ✅ landed
  preview)
- `docs/impl/linux-port-tracker-updates/2026-04-23-styled-cell-renderer.md`
  (full mirror with the visual verification + follow-up fixes
  from #58 and #59)
- `postmortem/2026-04-23-linux-styled-cell-renderer.md`
