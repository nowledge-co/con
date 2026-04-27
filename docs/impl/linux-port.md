# Linux Port — Plan and Status

con ships on macOS, has a working Windows beta, and now has a real
Linux preview built around Unix PTY + `libghostty-vt` + a GPUI-owned
per-row `StyledText` paint path. The same `con` binary that runs on
macOS now opens on Linux with client-side decorations (no native WM
titlebar stacked on top of the GPUI shell), a transparent ARGB
window with rounded corners, the same caption cluster Windows
ships, IoskeleyMono-shaped styled cells, the user's theme palette,
and KWin Wayland backdrop blur where the compositor exposes it.
This document is the Linux single-source-of-truth equivalent of
`docs/impl/windows-port.md`: it captures the upstream constraints,
the recommended architecture, and the staged path from today's
preview pane to a fully shippable Linux terminal backend (the
remaining work is the long-term GPUI-owned glyph-atlas grid
renderer matching the D3D11/DirectWrite path Windows uses, plus
mouse selection / reporting and packaging).

This is a planning document, not an implementation log. The live issue
tracker is GitHub issue #18. The deeper architecture notes live in
`docs/study/linux-port-feasibility.md`,
`docs/study/ghostty-linux-embed-gap.md`, and
`docs/study/gpui-linux-interop-gap.md`.

## Building today

Current platform state:

| Target | UI binary | Terminal pane | Control socket | Agent / settings / CLI |
|:-------|:---------:|:-------------:|:--------------:|:----------------------:|
| macOS  | ✅ real   | ✅ libghostty + Metal | `/tmp/con.sock` | ✅ |
| Windows | ✅ real  | ✅ libghostty-vt + ConPTY + D3D11/DirectWrite | `\\.\pipe\con` | ✅ |
| Linux  | ✅ real   | ✅ Unix PTY + `libghostty-vt` + GPUI per-row `StyledText` paint (preview — long-term glyph-atlas grid renderer pending) | `/tmp/con.sock` | ✅ |

On Linux today:

```bash
cargo build -p con --release
```

What that gives you:

- GPUI window shell on Linux with **client-side decorations** (no
  xfwm4 / mutter / kwin titlebar stacked on top of the GPUI shell);
  the in-app top bar carries the same minimize / maximize / close
  caption cluster Windows uses
- tabs, sidebar, agent panel, settings, command palette
- Unix-domain control socket at `/tmp/con.sock`
- `con-cli` and all portable crates working
- a real Linux terminal pane backed by Unix PTY + `libghostty-vt` and
  rendered as one GPUI `StyledText` per VT row, with one `TextRun`
  per styled span: SGR colors, bold (`FontWeight::BOLD`), italic
  (`FontStyle::Italic`), underline, strikethrough, inverse, and a
  block cursor (fg/bg swap on the cursor cell) all survive
- terminal-local Tab / Shift+Tab capture, so shell completion and TUI
  focus/navigation keys reach the Linux pane instead of being swallowed
  by GPUI focus traversal
- IoskeleyMono shaping (the user-facing display name `"Ioskeley
  Mono"` is normalized to the registered TTF family `"IoskeleyMono"`
  before lookup so GPUI's CosmicTextSystem resolves the embedded
  font instead of falling back to a system proportional sans)
- the user's `TerminalColors` (foreground / background / 16-color
  ANSI palette) plumbed into `VtScreen::set_theme` at session spawn
  and live whenever the user picks a theme in settings
- transparent ARGB window with rounded corners (14 px on Linux, no
  corners from `NSWindow` or DWM here so we clip the GPUI root
  ourselves) and per-pane / per-surface opacity that composites
  through to the desktop
- backdrop blur on KDE Plasma Wayland via `org_kde_kwin_blur` —
  real Gaussian blur of what's behind the window. On X11 / mutter /
  sway the blur toggle still ships but the visible result is
  transparency only (no portable backdrop-blur protocol exists
  outside KWin)
- a snappy paint pipeline: redundant per-tick snapshot refreshes
  and `Vec<Cell>` deep-equality compares were removed, PTY output now
  wakes the Linux terminal view directly instead of waiting for the
  workspace's idle poll loop to discover new output, the view caches
  per-row `StyledText` text/runs and only rebuilds rows flagged dirty
  by the VT snapshot (plus cursor-affected rows), and the placeholder
  for "Waiting for shell prompt…" only ever shows before the first
  prompt — alt-screen TUIs (htop, vim, less, fzf) no longer flash
  the placeholder during their startup gap

What it still does **not** give you:

- a real glyph-atlas / GPU grid renderer (today's per-row
  `StyledText` shape covers SGR + bold/italic/underline/inverse +
  a block cursor correctly, but per-cell metrics still come from
  layout-time text shaping rather than a fixed cell grid the way
  the Windows D3D11/DirectWrite path does)
- validated mouse selection / reporting
- native packaging artifacts (`.deb`, AppImage, Flatpak, …); the
  current release pipeline already ships a tarball, one-line installer,
  desktop entry, icon install, appcast, and notify-only updater

Current verification note:

- ChromeOS/Crostini is acceptable for Linux build/startup smoke checks.
- The Linux preview has been verified end-to-end on an XFCE +
  software-Vulkan (llvmpipe) cloud-VM session — full launch flow,
  styled output, transparent rounded chrome, htop in the
  alternate screen — so the paint path is proven on a real Linux
  desktop session, not just a headless harness. Validation on a
  hardware-accelerated Wayland / X11 native session and on
  multiple desktop environments (GNOME, KDE) is still the next
  useful step before the Linux preview comes off "preview" on the
  tracker.

The Linux CI job already installs the GPUI runtime dependencies on
`ubuntu-latest` (`libxcb-*`, `libxkbcommon-x11-dev`, `libwayland-dev`,
`libvulkan-dev`, `libfreetype-dev`, `libfontconfig1-dev`) and verifies
that the workspace still compiles there. The local Linux backend
also requires Zig so `con-ghostty` can build `libghostty-vt`, just like
the Windows backend does.

## Upstream status

### GPUI / Zed on Linux

Zed's upstream Linux backend is real and production-grade:

- `gpui_linux` selects a native client at runtime: Wayland when
  `WAYLAND_DISPLAY` is present, X11 when `DISPLAY` is present, and
  headless otherwise.
- Linux rendering goes through GPUI's own GPU path, not an embedded GTK
  widget. The backend uses `WgpuRenderer` against Wayland/X11 raw window
  handles.
- Text goes through `CosmicTextSystem`.
- Window kinds on Linux are `Normal`, `PopUp`, `Floating`, `Dialog`,
  plus Wayland-only `LayerShell`.

Important consequence for con:

- GPUI Linux gives us a real host app shell, clipboard, menus, input,
  and top-level windows.
- It does **not** currently give con a "host an arbitrary foreign Linux
  surface/widget inside the view tree" API analogous to the macOS
  `NSView` embedding boundary.

### Ghostty on Linux

Ghostty itself has real Linux support upstream:

- the Linux application runtime is GTK-based
- the Linux renderer is OpenGL-based
- Ghostty's own PTY/runtime path works on Linux already

That means Linux is not in the same place as Windows. We do not need to
assume "Ghostty has no Linux runtime."

But con does not consume Ghostty as a standalone GTK app. It consumes the
**embedded C API** via `con-ghostty`, and that is the critical boundary:

- the public `ghostty.h` platform enum still only exposes `MACOS` and
  `IOS`
- the embedded runtime's `Platform` union still only accepts macOS/iOS
  host views

So from con's point of view today, upstream Ghostty Linux support exists
but is not yet exposed through the embedding interface con uses on macOS.

One more important detail from the feasibility study: Ghostty's OpenGL
renderer already contains an `apprt.embedded` code path, but it is
explicitly stubbed out today for non-Darwin embedding and comments mark
rendering there as broken. So the Linux embed gap is not just "missing a
Linux enum tag in the C header"; it also includes real embedded-renderer
work upstream.

## Design conclusion

Linux is **closer to macOS in upstream capability** than Windows was:
Ghostty already owns PTY, terminal runtime, and renderer on Linux.

Linux is **not yet equivalent to macOS in embedding shape**:

- con is a GPUI app, not a GTK app
- Ghostty's embeddable C API does not yet expose a Linux host surface
- GPUI Linux does not currently expose a child/foreign-surface embedding
  primitive inside the view tree

We no longer need a split-brain Linux plan. The architecture decision is
now frozen:

- **con will ship a local Linux backend**
- **we will not block Linux on upstream Ghostty or GPUI embed work**

Upstream improvements remain welcome future optimizations, but they are
not the delivery plan.

## Terminal integration options

### Option A — upstream-first Linux embedding

Extend Ghostty's embedded C API so Linux is a first-class platform, then
teach con to host that Linux surface inside its GPUI shell.

What this would require:

- upstream Ghostty embedded-platform additions for Linux
- a Linux host-surface contract that works on Wayland and X11
- either:
  - GPUI support for embedding/interop with a foreign Linux-rendered
    surface, or
  - an offscreen-texture/export path that GPUI can paint directly

Why this is attractive:

- maximum reuse of Ghostty's real Linux PTY/runtime/renderer stack
- best long-term semantic alignment with macOS
- fewer duplicated terminal behaviors in con

Why this is risky:

- depends on upstream API work in Ghostty
- may also require upstream GPUI work
- Wayland/X11 host-surface semantics are more fragmented than AppKit

### Option B — local Linux backend in con

Build a real Linux backend inside `con-ghostty`, using a Unix PTY plus a
GPUI-native render path, in the same spirit as the Windows backend.

Likely ingredients:

- Unix PTY session management (`forkpty` / `openpty`)
- either `libghostty-vt` or another bounded terminal-state contract
- GPUI/wgpu rendering on Linux
- Linux clipboard, keyboard, mouse, selection, and resize plumbing

Why this is attractive:

- no upstream blocker to start implementation
- predictable ownership boundaries
- shares some shape with the existing Windows backend architecture

Why this is less attractive than macOS-style embedding:

- duplicates more terminal integration locally
- gives up the advantage of Ghostty's existing Linux application/runtime
  work

### Architecture decision

Linux now follows the same delivery principle that worked on Windows:

- choose the long-term-correct local architecture
- do not wait on upstream platform hooks to become available

Concretely, that means:

- Unix PTY ownership lives in `con-ghostty/src/linux/`
- terminal rendering will be GPUI-owned on Linux
- Ghostty's Linux runtime remains relevant study material, but not a
  ship blocker for con

This preserves control over schedule and keeps Linux aligned with con's
actual product boundary: a GPUI application with a terminal backend it
can ship.

## Staged plan

| Phase | Goal | Deliverable | Exit criteria | Status |
|---|---|---|---|---|
| 0 | Portable groundwork | Linux CI smoke test, stub backend, Unix socket transport | `cargo build -p con` works on Linux with a placeholder pane | ✅ landed |
| 1 | Architecture freeze | `docs/impl/linux-port.md`, tracker refresh, upstream constraints written down, Linux-specific placeholder module boundaries | Linux plan is explicit and no longer piggy-backs on the Windows plan | ✅ landed |
| 2a | Ghostty feasibility spike | confirm exact upstream delta needed for Linux embed (`ghostty_platform_*` additions, host-surface contract) | bounded upstream worklist exists, or path is ruled out | ✅ landed |
| 2b | GPUI feasibility spike | confirm whether con needs foreign-surface embedding, texture interop, or neither | bounded GPUI worklist exists, or path is ruled out | ✅ landed |
| 2c | Architecture decision | pick Linux backend lane | one recommended implementation path, no split-brain plan | ✅ landed |
| 3 | Linux backend scaffold | `con-ghostty/src/linux/` plus `con-app/src/linux_view.rs` (or equivalent) with real lifecycle types | Linux no longer routes through the generic stub path conceptually | ✅ landed |
| 4 | First real terminal surface | PTY spawn, resize, exit, `libghostty-vt` state, GPUI-owned pane paint, real product chrome (no native WM titlebar), embedded mono font, transparent + rounded window with compositor-gated blur | VT-backed Linux pane compiles and displays live shell state with SGR colors / bold / italic / underline / strikethrough / inverse, cursor block, theme palette synced from settings, IoskeleyMono shaping, client-side titlebar with min/max/close caption cluster, transparent ARGB window with rounded corners and per-pane / per-surface opacity, real KWin Wayland blur where available, fast paint pipeline (16 ms keystroke-echo round-trip), no placeholder flash on alt-screen TUIs | ✅ landed (preview) |
| 5 | Input + selection + glyph-atlas grid renderer | keyboard, mouse, clipboard, bracketed paste, DECCKM, selection, plus the long-term GPUI-owned glyph-atlas grid renderer matching the D3D11/DirectWrite path Windows uses | vim/tmux/fzf/less usable on Linux at full speed | 🚧 in progress (DECCKM, bracketed paste, and terminal-local Tab / Shift+Tab capture are wired; mouse reporting, selection, and the glyph-atlas renderer remain) |
| 6 | Packaging | one-line installer, tarball release, desktop entry, icon integration, appcast / notify-only updater, plus native artifact strategy (`.deb`, AppImage, Flatpak, etc.) | tarball installer exists; native package format decision remains | 🚧 partially landed |

## Immediate next work

With phase 4 landed (preview), the remaining Linux tasks are:

1. Replace the per-row `StyledText` paint path with the long-term
   GPUI-owned glyph-atlas grid renderer that paints background runs
   under the text and pre-rasterizes per-cell glyphs, matching the
   D3D11/DirectWrite path used on Windows. Today's view already
   honors fg / bg / bold / italic / underline / strikethrough /
   inverse and draws a block cursor, and the preview path now caches
   row text/runs so steady-state updates only rebuild dirty rows.
   The remaining cost is structural: per-cell metrics still come from
   layout-time text shaping rather than a fixed cell grid, and the
   shared `libghostty-vt` snapshot contract still clones the full cell
   buffer each changed frame. That limits how dense the renderer can
   stay on huge panes (the gap shows up first on `top -d 0.1`-class
   workloads and large command-start redraws).
2. Finish Linux input correctness: mouse reporting (button + wheel),
   selection (mouse drag → SGR 1006 / X10 reports + clipboard
   integration), and scrollback gestures. DECCKM and bracketed
   paste are already wired through `libghostty-vt` mode tracking
   and exercised by the existing keystroke encoder; Tab / Shift+Tab
   now use a terminal-local key context so shell completion and reverse
   focus traversal are not intercepted by GPUI's app-level focus
   navigation.
3. Validate on hardware-accelerated native Linux desktops (Wayland
   on KDE Plasma to confirm `org_kde_kwin_blur`; Wayland on
   GNOME / sway and X11 on each major WM to confirm the
   transparency / CSD / caption-cluster fallback paths). The
   cloud-VM XFCE + llvmpipe + Xvfb-style display has covered the
   software path end to end (full launch flow, styled output,
   transparent rounded chrome, htop in the alternate screen,
   keystroke-echo benchmark), but the hardware path still wants a
   real-desktop pass before we drop the "preview" label.
4. Packaging: the tarball + one-line installer + desktop entry + icon +
   notify-only appcast path is landed. The remaining packaging decision
   is whether to add `.deb`, AppImage, Flatpak, or another native
   artifact for Linux distributions.

## Tracker shape for issue #18

Issue #18 should mirror the Windows tracker:

- concise TL;DR at the top
- explicit "Done" vs "Pending"
- architecture conclusion called out directly
- links to the Linux plan doc and the Windows tracker where relevant
- phase-based progress rather than a flat brainstorm list

### Status updates mirrored to issue #18

Per-PR status updates land on issue #18 directly. We also keep
the same write-up in this directory, dated and named after the
landed milestone, so the rationale survives outside of GitHub. Keep
this list in chronological order:

- `docs/impl/linux-port-tracker-updates/2026-04-23-styled-cell-renderer.md` — phase 4 milestone landing. Covers the styled-cell paint over `libghostty-vt`, theme plumbing, block cursor, env-bootstrap notes for a fresh Ubuntu cloud VM, plus the seven follow-on fixes that got the preview to ship-ready state: client-side decorations + caption cluster, IoskeleyMono normalization, transparent + rounded window, KWin Wayland blur, the 16 ms → 8 ms paint-loop tightening (keystroke echo 32.6 ms → 16.6 ms mean), and the alt-screen `seen_any_output` placeholder fix. Corresponds to merged PR #58.
- `docs/impl/linux-port-tracker-updates/2026-04-23-release-pipeline.md` — full milestone summary covering both #58 (runtime backend) and #59 (release pipeline). One-liner installer (`install.sh` Unix dispatcher), tarball release script, `release-linux.yml` workflow, and the notify-only updater extension (Sparkle-shaped appcast XML + `apply_update_in_place` re-runs install.sh with `CON_INSTALL_VERSION` so beta-channel users never get silently downgraded to stable). Corresponds to PR #59.
- `docs/impl/linux-port-tracker-updates/2026-04-26-render-latency-followup.md` — post-preview latency and correctness follow-up. Covers PR #65 row-level `StyledText` cache, PR #68 direct PTY wake / shared profiling, and the final htop/vim stale-row fix that rebuilds visible rows on each VT generation update.

## References

- Linux tracker: issue #18
- Windows tracker: issue #34
- Windows plan: `docs/impl/windows-port.md`
- Linux feasibility study: `docs/study/linux-port-feasibility.md`
- Ghostty embed gap study: `docs/study/ghostty-linux-embed-gap.md`
- GPUI interop gap study: `docs/study/gpui-linux-interop-gap.md`
- GPUI Linux backend: `3pp/zed/crates/gpui_linux/`
- Ghostty embedded runtime: `3pp/ghostty/src/apprt/embedded.zig`
- Ghostty GTK runtime: `3pp/ghostty/src/apprt/gtk/`
