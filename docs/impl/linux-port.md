# Linux Port — Plan and Status

con ships on macOS, has a working Windows beta, and now has an
in-progress local Linux terminal backend built around Unix PTY +
`libghostty-vt`. This document is the Linux
single-source-of-truth equivalent of `docs/impl/windows-port.md`: it
captures the upstream constraints, the recommended architecture, and the
staged path from today's first VT-backed pane to a fully shippable Linux
terminal backend.

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
| Linux  | ✅ builds | Unix PTY + `libghostty-vt` + GPUI text pane (in progress) | `/tmp/con.sock` | ✅ |

On Linux today:

```bash
cargo build -p con --release
```

What that gives you:

- GPUI window shell on Linux
- tabs, sidebar, agent panel, settings, command palette
- Unix-domain control socket at `/tmp/con.sock`
- `con-cli` and all portable crates working
- a first Linux terminal pane backed by Unix PTY + `libghostty-vt`
  snapshots plus a temporary GPUI text renderer

What it still does **not** give you:

- final styled cell rendering
- validated mouse selection / reporting
- polished Linux window chrome / focus behavior across environments

Current verification note:

- ChromeOS/Crostini is acceptable for Linux build/startup smoke checks.
- It is not a reliable primary environment for con's Linux UI
  verification anymore.
- The current Linux backend reaches a live PTY + `libghostty-vt` prompt
  state under Crostini, but click/focus/paint behavior there remains
  unreliable.
- Further Linux UI verification should move to a native Linux desktop
  before we draw stronger conclusions from runtime behavior.

The Linux CI job already installs the GPUI runtime dependencies on
`ubuntu-latest` (`libxcb-*`, `libxkbcommon-x11-dev`, `libwayland-dev`,
`libvulkan-dev`, `libfreetype-dev`, `libfontconfig1-dev`) and verifies
that the workspace still compiles there. The local Linux backend now
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
| 4 | First real terminal surface | PTY spawn, resize, exit, `libghostty-vt` state, GPUI-owned pane paint | VT-backed Linux pane compiles and displays live shell state | 🚧 in progress |
| 5 | Input + selection | keyboard, mouse, clipboard, bracketed paste, DECCKM, selection | vim/tmux/fzf/less usable on Linux | ⏳ pending |
| 6 | Packaging | desktop entry, icon integration, artifact strategy (`.deb`, AppImage, Flatpak, etc.) | installable Linux artifact exists | ⏳ pending |

## Immediate next work

The next concrete Linux tasks should be:

1. Replace the interim text-pane rendering with a real GPUI-owned styled
   cell renderer fed from `libghostty-vt` snapshots.
2. Finish Linux input correctness: bracketed paste, DECCKM, mouse
   reporting, selection, and scrollback behavior.
3. Validate shell bring-up and app focus/chrome on real Linux desktops
   (Wayland, X11, and ChromeOS/Crostini).
4. Once the shell path is stable, iterate on packaging and Linux-native
   polish.

## Tracker shape for issue #18

Issue #18 should mirror the Windows tracker:

- concise TL;DR at the top
- explicit "Done" vs "Pending"
- architecture conclusion called out directly
- links to the Linux plan doc and the Windows tracker where relevant
- phase-based progress rather than a flat brainstorm list

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
