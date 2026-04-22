# Windows Port — Plan and Status

Con is macOS-only today. This document captures what we learned while
preparing the codebase for a Windows port and lays out the staged path to
get there. It is the single source of truth for that work; cross-platform
preparation PRs should link back here.

This is a planning document, not an implementation log. The corresponding
postmortem (`postmortem/2026-04-16-prepare-windows-port.md`) records the
decisions made when this plan was first written.

## Building today

As of Phase 2 the workspace builds on Windows, Linux, and macOS. What
you get per platform:

| Target | UI binary | Terminal pane | Control socket | Agent / settings / CLI |
|:-------|:---------:|:-------------:|:--------------:|:----------------------:|
| macOS  | ✅ real   | ✅ libghostty + Metal | `/tmp/con.sock` | ✅ |
| Windows | ✅ real  | ✅ libghostty-vt + ConPTY + D3D11/DirectWrite | `\\.\pipe\con` | ✅ |
| Linux  | ✅ builds | Placeholder card | `/tmp/con.sock` | ✅ |

On Windows (from a Developer Command Prompt for VS 2022):

```powershell
rustup default stable
git clone https://github.com/nowledge-co/con-terminal.git
cd con-terminal
cargo wbuild -p con --release          # → target\release\con-app.exe
cargo wtest  -p con-core -p con-cli -p con-agent -p con-terminal
```

Prerequisites:
- **Rust** (stable, edition 2024).
- **Visual Studio Build Tools 2022** with "Desktop development with
  C++" (for `link.exe` + Windows SDK).
- **Git for Windows**.
- **Zig 0.13 or newer** on PATH (for `libghostty-vt`); download from
  <https://ziglang.org/download/>. If the `zig` executable isn't on
  PATH, set `CON_ZIG_BIN` to its absolute path.

Build-time env vars for `con-ghostty`:

| Var | Effect |
|-----|--------|
| `CON_ZIG_BIN` | Absolute path to the `zig` executable. |
| `CON_GHOSTTY_SOURCE_DIR` | Reuse a local Ghostty checkout instead of fetching one into `OUT_DIR` (handy on Windows when `MAX_PATH` bites or Defender scans the `target` dir). |
| `CON_GHOSTTY_VT_STEP` | Exact Zig build step/flag to pass for libghostty-vt. Autodetected; override only if the probe picks wrong. |
| `CON_GHOSTTY_VT_SIMD=1` | Opt in to Ghostty's SIMD UTF-8 paths (`-Dsimd=true`). Default off on Windows because the resulting `ghostty-vt-static.lib` references `simdutf` C++ symbols that Zig doesn't bundle into the archive. Flip this once simdutf link-resolution is sorted (see TODO in `build.rs`). |
| `CON_STUB_GHOSTTY_VT=1` | Compile `src/windows/ghostty_vt_stub.c` and link it instead of libghostty-vt. The resulting `con-app.exe` launches fully, the terminal pane creates the real WS_CHILD HWND and swapchain, ConPTY spawns the shell — but the terminal grid is empty because the VT parser is stubbed. Useful for iterating GPUI / HWND / renderer paths while a real libghostty-vt is broken. |
| `CON_GHOSTTY_VT_RENDER_STATE=0` | Skip `ghostty_render_state_new` at startup. Default on (render state is how we read cells back for display). The escape hatch exists because older Ghostty revisions have a broken render-state implementation on Windows; the `GHOSTTY_REV` pin as of 2026-04-17 tip-of-main works, but if you pull a regression from upstream you can ship a runnable app with `=0` while the fix is in flight. |
| `CON_SKIP_GHOSTTY_VT=1` | Skip both. `cargo build` will fail at link. Only useful for `cargo check`. |

Common Windows pitfalls when the Zig step fails mid-build with
`FileNotFound` on a just-compiled `uucode_build_tables.exe`:

1. **Windows Defender** is quarantining the newborn executable. Add an
   exclusion for `C:\path\to\con` + `%USERPROFILE%\.zig-cache` +
   `%LOCALAPPDATA%\zig`.
2. **MAX_PATH** — the default 260-char limit trips on Zig's deep cache
   layout. Enable long paths (`HKLM:\SYSTEM\CurrentControlSet\Control\FileSystem\LongPathsEnabled = 1`)
   OR point `CON_GHOSTTY_SOURCE_DIR` at a short path like `C:\ghostty`.
3. **Zig version mismatch** — our pinned Ghostty revision was built
   against an older Zig; Zig 0.15+ may break upstream. Either install
   the matching Zig, or bump `GHOSTTY_REV` in `build.rs` (requires
   macOS re-validation of the full libghostty build).

The binary ships as `con-app.exe`, not `con.exe`: `CON` is a reserved
DOS device name and Windows refuses to create `con.exe` via most
Win32 file APIs. The package name is still `con` (so `cargo run -p
con` is unambiguous), and the `wbuild` / `wrun` / `wtest` / `wcheck`
aliases in `.cargo/config.toml` wrap the `--no-default-features
--features con/bin-con-app` incantation that selects the renamed bin.
macOS and Linux keep the plain `con` binary unchanged.

The produced `con-app.exe` launches into the full UI with a real,
GPU-rendered terminal pane — libghostty-vt parses the VT stream from a
ConPTY child, the D3D11/DirectWrite atlas renderer rasterizes glyphs
(including IoskeleyMono's Nerd-Font icon set), and the grid is drawn
in a single `DrawIndexedInstanced` per frame.

## What works today (macOS)

The macOS stack is fully native:

```
GPUI window (upstream zed-industries/zed)
  └── GhosttyView                          crates/con-app/src/ghostty_view.rs
        ├── child NSView (host)            cocoa::base::id
        ├── GhosttyApp     (per window)    crates/con-ghostty
        └── GhosttyTerminal (per surface)  ghostty_surface_t
              ├── PTY + child process      libghostty
              ├── VT parser + scrollback   libghostty
              ├── Metal renderer           libghostty (IOSurface layer)
              └── action callbacks         libghostty
```

The UI crate lives at `crates/con-app/` (not `crates/con/`) because `CON`
is a reserved DOS device name on Windows — `git clone` and `git checkout`
refuse to create any path component named `con`. The Cargo package name
and the binary name are both still `con` on macOS; only the filesystem
directory changed. See "Binary naming on Windows" below for how the
future Windows build will produce a valid executable filename.

`con-ghostty` is a thin Rust wrapper over libghostty's C embedding API.
libghostty is built from a pinned Ghostty revision via `zig build` and
linked statically as `libghostty-fat.a`. Surface creation hands a NSView
pointer to libghostty, which attaches a Metal `IOSurfaceLayer` and renders
the terminal directly into it. PTY, VT parsing, and rendering are all
inside libghostty.

## Per-platform status of the upstream layers

### libghostty (Ghostty embedded C API)

As of April 2026, libghostty's full embedded C API
(`ghostty_app_*`, `ghostty_surface_*`, `ghostty_config_*`) is **macOS-only**
upstream. The platform tag in `ghostty_platform_e` only has `MACOS` and
`IOS` variants — there is no `WINDOWS` variant in
`include/ghostty.h`.

Ghostty has split out a smaller library called **`libghostty-vt`**
(PR ghostty-org/ghostty#8840) which exposes only the VT parser and
terminal state machine and is buildable for macOS, Linux, Windows, and
WebAssembly. It does **not** include a renderer, font stack, or PTY —
those become the embedder's responsibility.

A Win32 apprt (`-Dapp-runtime=win32`) is in progress in community forks
(notably `InsipidPoint/ghostty-windows` and `adilahmeddev/windows-apprt`)
but is not merged upstream. None of these forks expose the embedded C API
that `con-ghostty` consumes — they ship a standalone `.exe`.

Cross-compiling libghostty for Windows from Linux is currently broken
(discussion ghostty-org/ghostty#11697); building **on** Windows works
after the fix in PR ghostty-org/ghostty#11698.

### GPUI (Zed)

The Zed Windows backend lives at `crates/gpui/src/platform/windows/`
in `zed-industries/zed`. It uses Direct3D 11.1 via DirectComposition,
DirectWrite for fonts, and is in public beta as of "Zed for Windows is
here" (zed.dev). Core features (rendering, dialogs, clipboard, fonts,
HiDPI, IME for most input methods, dark/light mode) work; rough edges are
multi-monitor, 120-144 Hz scheduling, certain IMEs, and some GPU
variants.

Two GPUI gaps matter for con's Windows port:

1. **HWND child-window embedding is missing.** PR
   zed-industries/zed#24330 attempted exactly this for Windows 11
   Taskbar embedding and was closed unmerged because maintainers wanted
   a cross-platform API first. The macOS pattern of "give libghostty an
   NSView and let it render into your view tree" has no upstream Windows
   equivalent.
2. **No tray icon, accessibility (UIA), or background-running app
   support.** Not blockers for con today but worth tracking.

The workspace currently uses upstream `zed-industries/zed` gpui directly
(see `Cargo.toml`), not the GPUI-CE community fork. CLAUDE.md previously
described a GPUI-CE pin; that wording is being corrected as part of this
prep work.

### gpui-component (longbridge)

Officially supports Windows x86_64 with CI. Depends on upstream Zed's
gpui by git, matching our workspace pin. Should require no per-target
changes.

### Rig, tokio, serde, etc.

All cross-platform. Rig only assumes std + reqwest, which we already
build with rustls.

## The path to a working Windows build

The port has to clear three independent technical hurdles. They can be
worked in parallel by different contributors.

### Hurdle 1 — Terminal backend on Windows

`con-ghostty` cannot be reused as-is. The realistic options are, in
order from lowest-risk to most-coupled:

**Option A. `libghostty-vt` + custom renderer + ConPTY (lowest risk).**

- Embed `libghostty-vt` for the VT parser and screen state machine.
- Implement a Direct3D 11 / DirectWrite renderer in a new
  `con-ghostty-win` crate (or as a `cfg(windows)` module inside
  `con-ghostty`).
- Drive a child shell via `CreatePseudoConsole` (ConPTY) and feed bytes
  into the libghostty-vt parser; pull back grid/scrollback diffs and
  render them.
- Reuse `con-ghostty`'s public Rust types (`TerminalColors`,
  `GhosttySurfaceEvent`, `GhosttyTerminal::write`, etc.) so
  `terminal_pane.rs` doesn't have to know which backend it is talking to.

This is what Ghostty maintainers point external embedders at today.
Largest engineering investment but cleanest layering.

**Option B. Carry a Win32 embedded apprt patch on libghostty.**

- Fork `InsipidPoint/ghostty-windows` and add an `apprt-embedded`
  variant that exposes `ghostty_surface_*` on Windows by accepting an
  HWND in a new `ghostty_platform_windows_s` variant.
- Update `con-ghostty/src/ffi.rs` to add the `WINDOWS` enum variant and
  the HWND union member.
- Update `con-ghostty/build.rs` to invoke `zig build` with the win32
  toolchain on Windows.

Smaller engineering investment per feature, but you own a Ghostty fork
forever unless the changes upstream.

**Option C. Wait for upstream.** Track `zig build` on Windows and the
in-progress Win32 apprt. No action item except subscribing to the
relevant issues.

We recommend **Option A** as the primary direction. Option B is a
viable fallback if libghostty-vt's surface area turns out to be
insufficient (e.g. if shell integration features that today live in the
full libghostty are needed and would be expensive to re-implement).

### Hurdle 2 — Embedding the terminal surface in the GPUI window

Whichever backend wins Hurdle 1, the rendered output has to land
inside a GPUI window. Three sub-options:

1. **Patch GPUI to support `WindowKind::Child` on Windows** (revive
   PR zed-industries/zed#24330). This is the structural equivalent of
   what we do on macOS. It needs upstream coordination, ideally with a
   cross-platform API that also covers macOS and Linux.
2. **Render the terminal grid via GPUI's own painter.** If the renderer
   from Hurdle 1A produces a pixel buffer or a textured quad, GPUI can
   paint it as part of its own scene. This avoids HWND embedding
   altogether but means we don't get libghostty's GPU pipeline — we'd
   re-render through GPUI's D3D11.
3. **Sibling HWND** with manual Z-order / position coordination. Fast
   to prototype but well-known to glitch on resize, alt-tab, and
   fullscreen.

Recommendation: prototype option 2 first because it keeps the rendering
contract entirely inside GPUI. If perf is unacceptable (likely fine for
text), pursue option 1.

### Binary naming on Windows

`CON` is a reserved DOS device name on Windows. That reservation covers
both filesystem paths (`git checkout` refuses to create `crates/con/`
even read-only, which is why the UI crate now lives at
`crates/con-app/`) and the produced executable — a `con.exe` file
cannot be created by most file-creation APIs. The `CON` reservation
applies regardless of extension.

Today this is latent: the UI binary is macOS-only via a `compile_error!`
in `crates/con-app/src/main.rs`, so no `con.exe` is ever produced. When
the Windows backend actually builds, the binary needs a new name on
Windows only — macOS and Linux should keep the plain `con` muscle
memory. Cargo does not support per-target `[[bin]] name` in the
manifest, so the two practical mechanisms are:

1. **Feature-gated twin `[[bin]]` entries pointing at the same
   `main.rs`** —

   ```toml
   [features]
   default = ["bin-con"]
   bin-con = []      # enabled on macOS/Linux via a .cargo/config.toml
   bin-con-app = []  # enabled on Windows

   [[bin]]
   name = "con"
   path = "src/main.rs"
   required-features = ["bin-con"]

   [[bin]]
   name = "con-app"
   path = "src/main.rs"
   required-features = ["bin-con-app"]
   ```

   Combined with a `.cargo/config.toml` that sets the right feature
   per `[target.cfg(...)]`, `cargo run -p con` works everywhere and
   produces `con` on Unix / `con-app.exe` on Windows.
2. **Rename the single `[[bin]]` to `con-app` and install a shell
   wrapper named `con`** in the macOS cask / Homebrew formula so the
   command-line experience is unchanged. Simpler manifest, more work
   in the packaging scripts.

We recommend mechanism 1 when Windows support lands. It keeps the
binary-name behavior platform-local and doesn't require external
installers to hide the difference. Until then, no change is needed —
the `compile_error!` gate ensures the name only matters on macOS,
where `con` is a valid filename.

### Hurdle 3 — Host-side Windows-isms

These are small, but they compound into "the binary won't even compile"
if untouched. The Windows-prep PR series should land these incrementally:

- **Sockets.** Replace `tokio::net::UnixListener`/`UnixStream` with a
  small transport abstraction backed by Unix sockets on `cfg(unix)` and
  Windows Named Pipes (`\\.\pipe\con-<user>`) on `cfg(windows)`.
- **Path conventions.** Replace hard-coded `/tmp` fallbacks with
  `std::env::temp_dir()`. Use `dirs::config_dir()` for config locations
  (already done in most places).
- **Permissions.** Gate `set_permissions(0o600)` and any
  `std::os::unix::*` imports behind `cfg(unix)`.
- **Bundle metadata.** `bundle_info_value`, `set_dock_icon`,
  `macos_major_version`, `global_hotkey`, `updater` (Sparkle) are
  macOS-only. Each call site needs a `cfg(target_os = "macos")` gate so
  the non-macOS paths use sensible defaults (e.g. version from
  `CARGO_PKG_VERSION`).
- **Global hotkey.** Carbon RegisterEventHotKey on macOS;
  `RegisterHotKey` on Windows. Out of scope for the first port but
  worth abstracting at the call site.
- **Auto-update.** Sparkle on macOS; the standard Windows pattern is
  WiX MSI + a separate updater binary, or `cargo dist`'s self-update
  flow. Out of scope for the first port.
- **Process spawn / shell discovery.** Currently delegated entirely to
  libghostty. The Windows backend (Hurdle 1) will need to discover
  `cmd.exe` / `powershell.exe` / `pwsh.exe` / `wsl.exe` and pipe
  through ConPTY itself.

## Phased work breakdown

The port should land in small, mergeable PRs in roughly this order. Each
phase keeps the macOS build green.

| Phase | Scope | What changes | Build state |
|------:|-------|--------------|-------------|
| 0 | prep | Cfg-gates, transport abstraction, docs, cleaner non-macOS error message | macOS unchanged; non-UI crates compile on Windows | ✅ landed |
| 1 | Windows + Linux CI smoke test | `.github/workflows/ci-portable.yml` runs `cargo check` + `cargo test` for the portable crates and the UI binary on `windows-latest` and `ubuntu-latest` | CI green on both targets | ✅ landed |
| 2 | Stub terminal backend | `con-ghostty` exposes stub types on non-macOS; `stub_view.rs` is a GPUI placeholder view selected via `#[path]`; the `compile_error!` is gone | **`cargo build -p con` works on Windows and Linux** — terminal pane paints a "backend under construction" card; agent panel, settings, command palette, control socket all functional | ✅ landed |
| 3a | Windows backend scaffold | `con-ghostty/src/windows/` modules: ConPTY wrapper, libghostty-vt FFI, D3D11 + DirectWrite renderer skeleton (clear-to-color), WS_CHILD HWND host with WM_SIZE/WM_DPICHANGED/WM_CHAR/WM_KEYDOWN/WM_PAINT plumbing, `WindowsGhosttyApp`/`WindowsGhosttyTerminal` facade. `con-app/src/windows_view.rs` instantiates a `HostView` lazily once GPUI's parent HWND is available. `build.rs` builds `libghostty-vt` via `zig build` on Windows host targets (probes available step names; `CON_GHOSTTY_VT_STEP` override). Binary renamed to `con-app.exe` on Windows via feature-gated twin `[[bin]]` (CON is reserved DOS device name). | `cargo wbuild -p con --release` on Windows produces a `con-app.exe` that launches; terminal pane creates a real WS_CHILD HWND, spawns a real ConPTY shell, drives libghostty-vt. Compiles clean on Linux + Windows cross-target. | ✅ landed |
| 3b | Glyph atlas + grid render | HLSL shaders embedded and runtime-compiled via `D3DCompile`. Skyline-packed (`etagere`) BGRA8 glyph atlas (`con-ghostty/src/windows/render/atlas.rs`), glyphs rasterized via Direct2D `DrawText` with grayscale AA. Three-case PUA rasterization (fits-cell / width-overflow with lsb shift / height-overflow with scale-around-centre) for Nerd-Font icons, plus per-slot `PushAxisAlignedClip` + black pre-fill to prevent ClearType fringe bleeding between atlas neighbours. D3D11 pipeline (`pipeline.rs`): per-instance IA layout, dynamic instance buffer with `Map(WRITE_DISCARD)`, single `DrawIndexedInstanced(6, cell_count)` per frame — matches Windows Terminal AtlasEngine's architecture. Wide PUA instances are stable-sorted after narrow ones so DX11's in-order per-pixel writes let them win overlap with neighbour backgrounds; the cursor inverse-colour instance is captured pre-sort and pushed post-sort so it always draws last. `vt.rs` uses the `ghostty_render_state` API (row iterator + `row_cells_get_multi` + DIRTY row skip) — not the `grid_ref` path that the upstream header explicitly warns against for render loops. | Real glyph rendering on Windows, runtime-validated on real hardware against oh-my-posh prompt stacks and dense hyphen rules. Postmortem: `postmortem/2026-04-21-windows-atlas-pua-rasterization.md`. | ✅ landed (runtime-validated) |
| 3c | Input + selection | VK_* → xterm escape sequence translation in `host_view::WM_KEYDOWN`; mouse selection / scroll forwarding; clipboard via OSC 52 + Win32 clipboard. | Real terminal interactivity. | — |
| 3d | (Parallel, upstream) | `WindowsWindow::attach_external_swap_chain_handle(bounds, HANDLE)` PR to `zed-industries/zed`. ~50 LOC. When merged, swap our backend from `CreateSwapChainForHwnd(WS_CHILD HWND)` to `CreateSwapChainForCompositionSurfaceHandle` so DWM composites our visual cleanly with GPUI's. Zero user-visible change except popup Z-order becomes pixel-perfect. | (No con change required pre-merge.) | — |
| 3e | **Renderer perf tuning** (in progress) | The current pipeline draws into an offscreen D3D11 texture, reads back BGRA bytes (`RenderSession::render_frame`), wraps them as `ImageSource::Render(Arc<RenderImage>)`, and lets GPUI re-upload them into its DComp tree. The GPU→CPU readback per dirty frame plus the CPU→GPU re-upload adds 5–15ms over Windows Terminal's direct-DComp present path. Plan: (1) ✅ wake GPUI from the ConPTY reader thread so freshly arrived shell output paints on the next prepaint instead of waiting for user input; (2) once Phase 3d lands, present the swap chain directly via `attach_external_swap_chain_handle` and drop the readback entirely; (3) profile with PIX / GPUView / ETW to verify Enter→glyph latency parity with WT. | First win in `crates/con-app/src/windows_view.rs` (`wake_tx` + coalescer task). | 🚧 in progress |
| 4 | Hardening | Multi-pane, splits, IME, focus, resize, copy/paste, drag/drop, OSC 133 shell integration, ligatures | Beta-quality | — |
| 5 | Distribution | MSI installer, code signing, auto-update (WiX or `cargo dist`), `con-app.exe` rename via feature-gated twin `[[bin]]` | Release-ready | — |

Phases 0-3b are **complete**. Phase 3b shipped the full glyph atlas,
the three-case PUA rasterization that lets IoskeleyMono's Nerd-Font
icons and ASCII punctuation coexist, and the cursor z-order fix. Phase
3c (input + selection) is the remaining interactivity work. Phase 3d
is independent upstream work. **Phase 3e (renderer perf tuning) is
active** — see the row above and the `Renderer perf` section below.

### Renderer perf — current cost and tuning track

The macOS path uses libghostty's Metal pipeline, which presents directly
to a `CAMetalLayer` and is profiled (`docs/impl/macos-terminal-profiling.md`).
The Windows backend is a custom D3D11/DirectWrite stack that — to side-step
the WS_CHILD HWND z-order pain modals had — currently composites through
GPUI's image path:

```
ConPTY reader thread ─→ vt.feed(bytes) ─→ updates grid
                                          (now: signals wake_tx)
GPUI prepaint tick   ─→ render_frame()
                       └→ D3D11 draw to OFFSCREEN texture     (GPU)
                       └→ GPU→CPU readback of BGRA bytes      ← intrinsic cost
                       └→ wrap as RenderImage(Arc)
                       └→ GPUI uploads back to GPU            (CPU→GPU)
                       └→ DComp composite                     (GPU)
```

Compared to Windows Terminal's AtlasEngine, two costs are structural:

1. **GPU→CPU→GPU round-trip per dirty frame.** Synchronous readback alone
   is typically 5–15ms; re-upload adds a few more. WT presents directly to
   a DComp swap chain — no readback.
2. **PTY arrival → repaint latency.** The reader thread used to update the
   grid in place but never poke the view. The next prompt only painted on
   the next user input event or animation tick (up to ~16ms slack).
   Phase 3e step (1) closes this — `RenderSession::new` now takes a
   `wake: Fn() + Send + Sync` callback that the view uses to push a
   coalesced `cx.notify()` per PTY chunk (`crates/con-app/src/windows_view.rs`).

The remaining structural cost (the readback) goes away once Phase 3d
(`attach_external_swap_chain_handle`) lands and we can present the swap
chain into GPUI's DComp tree directly.

### What you can do *today* on Windows after Phase 3b

```powershell
git clone https://github.com/nowledge-co/con-terminal.git
cd con-terminal
cargo wbuild -p con --release
target\release\con-app.exe
```

The window comes up with the full chrome and a live terminal pane.
ConPTY spawns `pwsh.exe`/`cmd.exe`, libghostty-vt parses the VT
stream, and the D3D11/DirectWrite atlas renderer draws the grid at
native refresh rate — IoskeleyMono ASCII, box-drawing, Powerline
separators, and the Nerd-Font icon set (folder, git, status, …) all
render correctly, cursor lands on the right column, and typing flows
through `WM_CHAR` to the shell. Resize works. Window close kills
the shell. Phase 3c tracks the remaining input/selection polish.

Caveats:
- You must have **Zig 0.13+** on `PATH` (one-time install from
  <https://ziglang.org/download/>) for `cargo build` to compile
  `libghostty-vt`. `cargo check` works without Zig (compile-only, no
  link); set `CON_SKIP_GHOSTTY_VT=1` to skip the build entirely.
- The shell is `$env:COMSPEC` if set, else `pwsh.exe`/`powershell.exe`
  if on PATH, else `cmd.exe`. User-overridable via config (Phase 4).
- Cell grid sizing uses a stub 8×16 px until DirectWrite font metrics
  are wired (3b). Windows users will see the wrong column/row count
  reported to the shell until then.

## Open questions

These do not need to be resolved to merge Phase 0, but the answers shape
Phase 3 onward. Track them in follow-up issues:

1. Does libghostty-vt expose enough surface for shell integration
   (OSC 133, OSC 7) the way the full libghostty does? If not, do we
   re-implement those in `con-ghostty-win` or push for the surface to
   grow upstream?
2. Is GPUI's text render quality close enough to libghostty's
   subpixel/grayscale tuning that a GPUI-painted terminal looks right?
   We will not know until Phase 3.
3. How does `dispatcher` / `display_link` frame scheduling on GPUI
   Windows compare to macOS? Terminal scrolling at 120 Hz is a
   feature, not a nice-to-have.
4. ConPTY's resize behavior is famously finicky — what's the right
   place to live (inside the backend trait or above it)?

## References

- Ghostty: <https://github.com/ghostty-org/ghostty>
- Ghostty Windows roadmap discussion #2563:
  <https://github.com/ghostty-org/ghostty/discussions/2563>
- libghostty-vt PR #8840:
  <https://github.com/ghostty-org/ghostty/pull/8840>
- Ghostty libxml2 Windows build issue #11697:
  <https://github.com/ghostty-org/ghostty/discussions/11697>
- Community Win32 apprt fork:
  <https://github.com/InsipidPoint/ghostty-windows>
- Zed for Windows launch: <https://zed.dev/blog/zed-for-windows-is-here>
- GPUI WindowKind::Child PR (closed):
  <https://github.com/zed-industries/zed/pull/24330>
- Microsoft ConPTY:
  <https://learn.microsoft.com/en-us/windows/console/creating-a-pseudoconsole-session>
