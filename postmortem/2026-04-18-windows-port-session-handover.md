# Windows Port — Session Handover (2026-04-18)

Multi-session context for continuing the Windows port of con. Paste the
"Kickoff prompt" at the bottom into a fresh Claude session and it has
everything it needs.

This doc is intentionally long — it captures not just *what* was done
but *why*, so the next session can make consistent judgment calls
without re-deriving the strategy.

---

## Project context

`con` is an open-source, macOS-native, GPU-accelerated terminal
emulator with a built-in AI agent harness. Rust workspace. Stack:

- **UI**: upstream Zed GPUI (git dependency, Apache 2.0). D3D11 +
  DirectComposition on Windows.
- **Terminal runtime** (macOS): full libghostty via C API, Metal GPU
  rendering, embedded as a native `NSView`.
- **Agent**: Rig v0.34 (13 providers, Tool trait).
- **Control plane**: JSON-RPC 2.0 over Unix domain socket on
  macOS/Linux, Windows Named Pipe (`\\.\pipe\con`) on Windows.

The goal of this work stream is to bring the Windows build to
production quality. The non-UI crates (agent, cli, core, terminal)
already compile for `x86_64-pc-windows-*`; the new engineering is in
the terminal pane backend, HWND embedding, and the Windows-specific
host plumbing.

---

## The three hurdles (strategic view)

The Windows port has three independent hurdles. Progress on each is
tracked in the phase table below.

### 1. Terminal backend — DONE strategy, in-progress implementation

libghostty's full C API is macOS-only (hard dep on Metal + CoreText).
We evaluated three options and chose **Option A**:

- **Option A (chosen)**: libghostty-vt + ConPTY + custom renderer.
  `libghostty-vt` (upstream PR #8840) exports only the parser + screen
  state machine, cross-platform. We own PTY + key encoding + rendering.
- Option B: port full libghostty to Windows. Too much surface area
  (Metal → D3D11, CoreText → DirectWrite, NSView → HWND, macOS-only
  IPC). Months of work upstream.
- Option C: shell out to conhost / Windows Terminal widget. Loses our
  GPU pipeline and hard to embed in GPUI.

### 2. GPUI HWND embedding — PARTIAL

GPUI's Windows backend is D3D11 + DirectComposition. It doesn't
natively support embedding a child HWND into the window tree. Two
embedding strategies:

- **WS_CHILD now (what ships today)**: create our D3D11 HWND as a
  child of GPUI's main HWND. Works, but has z-order + DPI + input
  quirks (focus never reaches the child; we route keys through GPUI's
  focus chain instead).
- **DComp sibling visual (Phase 3d)**: contribute an "external
  swapchain" API upstream to GPUI so our D3D11 swapchain becomes a
  DirectComposition sibling visual inside GPUI's DComp tree. See
  `docs/study/gpui-external-swapchain-upstream-pr.md`.

### 3. Host-side Windows-isms — PARTIAL

Things that aren't the terminal or the compositor but still need
Windows-specific code:

- DPI (per-monitor v2, `WM_DPICHANGED`)
- ClearType vs grayscale AA
- ConPTY (`CreatePseudoConsole`, thread-per-pipe, CR vs LF)
- Clipboard (`OpenClipboard` / `CF_UNICODETEXT`)
- Reserved DOS device name `CON` → binary renamed to `con-app.exe`
- Defender / long-path exclusions for the build tree (documented in
  `docs/impl/windows-port.md`)

---

## Phase table

| Phase | Name | Status | Key commits |
|---|---|---|---|
| 0 | Workspace prep (rename con→con-app, twin `[[bin]]`, cargo aliases) | ✅ | `b9d0499`, `0e36d44` |
| 1 | CI portable (Linux + Windows cross-check) | ✅ | wired in `.github/workflows/ci-portable.yml` |
| 2 | Stub terminal backend (UI binary builds and runs on Windows) | ✅ | `01f63b7` |
| 3a | Real Windows terminal scaffold (ConPTY + HWND + VT wiring) | ✅ | `5fb0f0e`, `4fd9a17` |
| 3b | Glyph renderer (DWrite atlas + D3D11 grid + attrs) | ✅ | `2c29fd9` → `769b6ee` (26 commits) |
| 3c | Input, mouse, selection, clipboard, DPI, ClearType | ⏳ in progress — **this session** | planned: Milestones A.5 + B + C |
| 3d | GPUI external-swapchain upstream (DComp sibling embed) | — queued | see study doc |
| 4 | Hardening (panic-free resize, atlas overflow, mode queries) | — queued | part of Milestone C |
| 5 | Distribution (MSIX / installer / code signing) | — queued | `docs/impl/windows-port.md` |

---

## Decision log

Captures the key decisions so the next session doesn't re-litigate
them.

1. **libghostty-vt over full libghostty** (Option A). Cross-platform
   parser beats porting a macOS-only renderer. We pay for it by owning
   the renderer and key encoder.
2. **Pinned rev `ca7516bea60190ee2e9a4f9182b61d318d107c6e`** on
   ghostty main. Bumped once (`c4a9bde`) to pick up lib-vt emit
   changes; don't bump casually — cell/style ABI is still evolving
   upstream.
3. **`crates/con-app/` + twin `[[bin]]`** (`bin-con` default on Unix,
   `bin-con-app` on Windows). Driven by `CON` being a reserved DOS
   device name. `wbuild` / `wrun` / `wtest` / `wcheck` aliases in
   `.cargo/config.toml` hide the feature-flag incantation.
4. **`-Dsimd=false` when building libghostty-vt**. simdutf C++ objects
   aren't bundled into zig's static archive; leaving SIMD on yields
   `unresolved external` at link time.
5. **WS_CHILD today, DComp sibling later**. WS_CHILD unblocks
   end-to-end smoke testing immediately; the external-swapchain
   upstream work is a separate Phase 3d track.
6. **Render driven by GPUI paint, not WM_PAINT** (`f59a902`). GPUI's
   compositor frame is the source of truth. WM_PAINT on the child
   HWND is untrustworthy (z-order + DComp interactions).
7. **Keyboard routed through GPUI focus chain**, not `WM_KEYDOWN` on
   the child HWND (`9478c78`). The child never gets focus because
   GPUI owns it.
8. **`DXGI_ALPHA_MODE_IGNORE`** on the swapchain (`8974bd0`).
   `CreateSwapChainForHwnd` rejects `PREMULTIPLIED`.
9. **`\n → \r` translation before ConPTY write** (`9478c78`). ConPTY
   expects carriage return to submit a line, not newline.
10. **Globals cbuffer must be bound to BOTH VS and PS** (`769b6ee`).
    D3D11 binds per stage; PS reads `cellSize` for underline band
    sizing.
11. **`GhosttyCell` is opaque `uint64_t`** (`484e82d`). Never cast.
    Always read via `ghostty_cell_get(cell, KEY, &out)`. Cell BG/FG
    is `GhosttyColorRgb` (3 bytes), packed into RGBA for the shader.
12. **Escape hatches**: `CON_STUB_GHOSTTY_VT=1` (link-time no-op
    stub) and `CON_GHOSTTY_VT_RENDER_STATE=0` (skip render-state
    creation — we crashed at one pin and needed a survival mode while
    we diagnosed it; see `c5524eb`).

---

## Branch

`claude/prepare-windows-support-f7drP` (pushed, up to date on origin).

Most recent commits:

```
d3ff1bd docs: windows-port session handover (2026-04-18)
769b6ee fix(windows): bind Globals cbuffer to the pixel shader
b7ae5ca feat(windows): render bold/italic/underline/strikethrough + clear atlas on rebuild
9478c78 feat(windows): keyboard input; translate \n→\r for ConPTY
a350e23 feat(windows): render inverse-video block cursor
4eeffe5 fix(windows): use DirectWrite metrics for cell width/height
805c947 fix(windows): fall back to palette default fg/bg on unstyled cells
484e82d fix(windows): decode codepoint via ghostty_cell_get (RAW is opaque)
```

---

## Windows commit timeline (grouped by phase)

All on `claude/prepare-windows-support-f7drP`, 33 commits `b9d0499` →
`d3ff1bd`.

### Phase 0 — workspace prep

- `b9d0499` chore: prepare workspace for Windows port
- `0e36d44` chore: rename crates/con to crates/con-app for Windows cloneability

### Phase 2 — stub backend

- `01f63b7` feat(windows): buildable UI binary with stub terminal backend

### Phase 3a — real scaffold

- `5fb0f0e` feat(windows): Phase 3a — real Windows terminal backend scaffold
- `4fd9a17` fix(windows): rename bin to con-app on Windows; diagnose Zig failures

### Phase 3b — glyph renderer + stability (26 commits)

Renderer bring-up, ConPTY, ghostty-vt FFI:

- `2c29fd9` Phase 3b — real glyph renderer (DWrite atlas + D3D11 grid)
- `1648dd7` use -Demit-lib-vt=true for libghostty-vt on current Ghostty
- `a73aee1` CON_STUB_GHOSTTY_VT stub backend + Defender/long-path docs
- `7b8ad61` use per-key `_get` instead of `_get_multi` in vt FFI
- `c507f9f` link ghostty-vt-static.lib instead of DLL import lib
- `df020f5` default Ghostty SIMD off so static lib is self-contained
- `8974bd0` swapchain AlphaMode IGNORE for HWND; latch init failures
- `aa1d5bb` add RENDER_TARGET bind flag to glyph atlas texture
- `f5e4656` panic hook to %TEMP%\con-panic.log + init breadcrumbs
- `9986405` SEH handler + ghostty_terminal_new breadcrumbs
- `c5524eb` skip ghostty_render_state_new by default — crashes at pin
- `c4a9bde` bump GHOSTTY_REV to 2026-04-17 tip-of-main
- `ffb9066` rewrite vt.rs FFI to match real upstream libghostty-vt API
- `f59a902` drive render from GPUI paint + log pane bounds
- `c9c059d` grow instance buffer + normalize atlas UV in shader
- `167d62f` rerun build.rs on env change; hide cmd.exe; validate WM_PAINT
- `35393f3` pwsh-first shell default; conpty + snapshot tracing
- `36f8d5b` ConPTY needs bInheritHandles=true, no CREATE_NO_WINDOW
- `85644ba` pass HPCON value (not &hpcon) to UpdateProcThreadAttribute
- `2f37e2a` read cell BG/FG as GhosttyColorRgb (3 bytes), pack to RGBA
- `5fd5769` VS quad-corner mapping off-by-one; add no-cull rasterizer
- `140cbbf` split atlas_rect into atlas_pos + atlas_size (2x R32G32)

Milestone A — rendering polish (this session):

- `484e82d` decode codepoint via ghostty_cell_get (RAW is opaque)
- `805c947` fall back to palette default fg/bg on unstyled cells
- `4eeffe5` use DirectWrite metrics for cell width/height
- `a350e23` render inverse-video block cursor
- `9478c78` keyboard input; translate \n→\r for ConPTY
- `b7ae5ca` render bold/italic/underline/strikethrough + clear atlas on rebuild
- `769b6ee` bind Globals cbuffer to the pixel shader

Handover:

- `d3ff1bd` docs: windows-port session handover (2026-04-18)

---

## Current session achievements (Milestone A)

Starting state: glyphs rendered but with packed-u32 codepoint decode
bugs, missing palette fallback, wrong cell metrics, no cursor, no
keyboard, no attrs.

Delivered this session (7 commits `484e82d` → `769b6ee`):

1. **Correct codepoint decode** via `ghostty_cell_get(CELL_CODEPOINT,
   &out)` — previously cast the opaque u64 cell to u32, which is
   meaningless.
2. **Palette fallback** — unstyled cells report `(0,0,0)` for fg/bg;
   substitute from render-state default palette.
3. **DirectWrite cell metrics** — cell width/height from
   `GetDesignGlyphMetrics` scaled by em size, not a made-up constant.
4. **Inverse-video block cursor** — snapshot the cursor pos, flip
   `ATTR_INVERSE` on that cell.
5. **Keyboard input end-to-end** — `handle_key_down` in
   `windows_view.rs` translates GPUI key events to bytes; `\n → \r`
   so ConPTY submits Enter correctly.
6. **Bold / italic / underline / strikethrough** — attrs bit layout
   threaded through `vt.rs` → `render/mod.rs` → `shaders.hlsl`.
   Atlas is cleared on rebuild so cached glyphs don't leak across
   font/weight changes.
7. **Globals cbuffer PS fix** — was only bound to VS; PS read stale
   `cellSize`, making underline/strike bands land in the wrong place.

Verified on a real Windows machine by the user (screenshot on
2026-04-18). Side-by-side vs Windows Terminal shown under "State:
visible gaps".

---

## State: what works on Windows today

End-to-end smoke tested on a real Windows machine:

- `con-app.exe` launches into GPUI UI shell, terminal pane becomes a
  `WS_CHILD` HWND parented to GPUI's main HWND.
- ConPTY spawns `pwsh.exe` (falls back to `powershell.exe`, then `cmd.exe`).
- libghostty-vt parses PTY output; render-state iterator feeds D3D11
  glyph pipeline.
- D3D11 + DirectWrite + Direct2D + etagere skyline packing = glyph atlas.
- Typing works (via GPUI `on_key_down` on the root div — NOT `WM_KEYDOWN`
  on the child HWND, which is a no-op stub).
- Input bar (bottom of GPUI) also routes commands through, with `\n→\r`
  translation for Enter.
- Rendering: correct codepoint decode, palette default fg/bg fallback,
  real DirectWrite cell metrics, inverse-video block cursor, bold /
  italic / underline / strikethrough / inverse all working.

## State: visible gaps (user-reported)

Side-by-side vs Windows Terminal on same machine (user's screenshot on
2026-04-18):

1. **DPI**: con renders smaller than Windows Terminal on same monitor —
   we don't read `GetDpiForWindow` or handle `WM_DPICHANGED`. Font is
   14px logical; Windows Terminal scales to physical.
2. **Text quality**: grayscale AA (`D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE`)
   vs Windows Terminal's ClearType — our text is softer.
3. **Font**: renderer hardcodes "Cascadia Mono" default; design language
   (CLAUDE.md) calls for IoskeleyMono.
4. **Mouse**: click / wheel stubbed (`host_view.rs:357-360`).
5. **Selection + clipboard**: none. Can't copy terminal output.

## Planned day-of-work (Milestone A.5 + B + C)

User is away for the day and asked for autonomous implementation of as
much as possible toward "Windows production-ready". See "Plan of
attack" below for the detailed cut list.

---

## Technical context you need

### Repo layout — Windows bits

```
crates/
├── con-ghostty/src/windows/
│   ├── mod.rs              module registrations
│   ├── backend.rs          WindowsGhosttyApp / WindowsGhosttyTerminal (public facade)
│   ├── host_view.rs        WS_CHILD HWND + wndproc + dispatch to renderer/VT/ConPTY
│   ├── conpty.rs           ConPTY spawn + read thread + resize
│   ├── vt.rs               libghostty-vt FFI + ScreenSnapshot
│   ├── ghostty_vt_stub.c   link-time stub for CON_STUB_GHOSTTY_VT=1
│   └── render/
│       ├── mod.rs          Renderer (device, swapchain, instance build, draw)
│       ├── pipeline.rs     HLSL compile + IA layout + buffers
│       ├── atlas.rs        GlyphCache (Direct2D DrawText into DXGI-backed RT)
│       └── shaders.hlsl    vs_main / ps_main
└── con-app/src/windows_view.rs   GPUI GhosttyView (hosts HostView)
```

### libghostty-vt

Pinned rev `ca7516bea60190ee2e9a4f9182b61d318d107c6e`. Headers live
upstream at `github.com/ghostty-org/ghostty/include/ghostty/vt/*.h`.
Read them via WebFetch (raw.githubusercontent.com/...) if you need a
new API — the vendored source isn't checked in locally.

Key APIs already bound (vt.rs):

- `ghostty_terminal_*`: new, free, resize, vt_write, get
- `ghostty_render_state_*`: new, free, update, get, row_iterator_*, row_cells_*
- `ghostty_cell_get` for per-cell KEY access
- `GhosttyStyle` (full struct) + `ghostty_style_default` / `ghostty_style_is_default`

Key APIs **not yet bound** that we'll want:

- `ghostty_terminal_mode_get(mode, *bool)` — query DECCKM, bracketed
  paste, mouse-tracking modes
- `GHOSTTY_TERMINAL_DATA_MOUSE_TRACKING` — get mouse tracking state
- `GHOSTTY_TERMINAL_DATA_SCROLLBACK_ROWS` — scrollback size
- Key encoder API (`ghostty_key_encoder_*`) — we roll our own for now, fine
- OSC parser (`ghostty_osc_*`) — would be needed for OSC 52 clipboard /
  OSC 8 hyperlinks

### Build

```bash
# Windows (on actual Windows host, from VS 2022 dev cmd):
cargo wbuild -p con --release            # produces target\release\con-app.exe
cargo wrun   -p con

# Linux dev machine cross-check (no libghostty-vt; uses stub):
CON_STUB_GHOSTTY_VT=1 cargo check -p con-ghostty --target x86_64-pc-windows-gnu --no-default-features
```

`CON_STUB_GHOSTTY_VT=1` + `x86_64-pc-windows-gnu` is the fastest feedback
loop on Linux — compiles everything including the Windows wndproc, just
links to a no-op stub instead of libghostty-vt.

### Critical architecture notes

1. **Keyboard flow is GPUI, not Win32**. GPUI owns focus; our WS_CHILD
   HWND never gets `WM_CHAR` / `WM_KEYDOWN` through the focus chain.
   `windows_view.rs::handle_key_down` is where keys are translated into
   bytes and fed to ConPTY. The `WM_KEYDOWN` TODO in `host_view.rs:352`
   is a fallback path we haven't wired; user confirmed typing works via
   the GPUI path.

2. **Render is driven by GPUI paint**, not Windows WM_PAINT. See
   `HostView::paint_frame` called from `windows_view.rs::paint_host`
   which is called from `on_children_prepainted`. Don't depend on
   `WM_PAINT` firing.

3. **DXGI swapchain uses `DXGI_ALPHA_MODE_IGNORE`** — `CreateSwapChainForHwnd`
   rejects PREMULTIPLIED. The HWND already clips the rect; no per-pixel
   alpha against GPUI compositor needed.

4. **Attrs bit layout** (kept in sync between `vt.rs`, `render/mod.rs`,
   `shaders.hlsl`):
   - bit 0 = bold
   - bit 1 = italic
   - bit 2 = underline
   - bit 3 = strikethrough
   - bit 4 = inverse

5. **Globals cbuffer must be bound to BOTH VS and PS** —
   `pipeline.rs::bind_and_draw`. PS reads `cellSize` for underline band
   sizing.

6. **GhosttyCell** is an opaque `uint64_t`, not a packed codepoint. Use
   `ghostty_cell_get(cell, KEY, &out)`. `BG_COLOR`/`FG_COLOR` on
   `row_cells` write `GhosttyColorRgb` (3 bytes), not packed u32.
   Palette default fg/bg is read from render state and substituted when
   the cell reports (0,0,0).

---

## Plan of attack (the day)

Approx 4 hours of focused work. Commit per milestone. Push each.

### Milestone A.5 — rendering polish (~1.5h)

| # | Task | Files |
|---|---|---|
| 1 | `GetDpiForWindow` on HostView::new; scale `RendererConfig.font_size_px` by `dpi/96`. | `host_view.rs`, `render/mod.rs` |
| 2 | `WM_DPICHANGED`: extract new DPI from `HIWORD(wparam)`, call `Renderer::rebuild_atlas` with scaled size, `MoveWindow` to suggested rect from lparam. | `host_view.rs` |
| 3 | Switch AA to `D2D1_TEXT_ANTIALIAS_MODE_CLEARTYPE`. ClearType writes per-subpixel R,G,B coverage into atlas; update PS to sample (r,g,b) separately and lerp each channel with fg→bg. Fall back to DEFAULT if the D2D factory rejects CLEARTYPE on this GPU. | `render/atlas.rs`, `render/shaders.hlsl` |
| 4 | `IDWriteFactory::CreateCustomRenderingParams` (gamma=1.8, enhanced contrast=0.5, ClearType level=1.0, RGB pixel geometry, natural rendering mode). Apply via `ID2D1RenderTarget::SetTextRenderingParams`. | `render/atlas.rs` |
| 5 | Default font → IoskeleyMono. | `render/mod.rs::RendererConfig::default` |

### Milestone B — interaction (~2h)

| # | Task | Files |
|---|---|---|
| 6 | `WM_MOUSEWHEEL` → write `\x1b[<64M` / `\x1b[<65M` (SGR mouse wheel) when mouse tracking is on; otherwise ignore (libghostty-vt doesn't expose viewport scroll, so plain scroll-past-prompt doesn't work without real scrollback API). Gate via `ghostty_terminal_mode_get(MOUSE_TRACKING, &on)`. | `host_view.rs`, `vt.rs` |
| 7 | Selection state in `HostState`: `{ start: (col,row), end: (col,row), active: bool, rectangle: bool }`. `WM_LBUTTONDOWN` starts, `WM_MOUSEMOVE` (if button held) updates, `WM_LBUTTONUP` finalizes. Convert mouse coords to cell coords via `metrics.cell_width_px / cell_height_px`. | `host_view.rs` |
| 8 | Render selection: in `Renderer::render`, walk selection range and set `attrs |= ATTR_INVERSE` on those cells before pushing instances. Reuses existing inverse path. | `render/mod.rs` |
| 9 | Extract selection text: iterate snapshot.cells in selection range, collect codepoints, join rows with `\n`. Add `HostView::copy_selection() -> Option<String>`. | `host_view.rs`, `vt.rs` |
| 10 | Win32 clipboard helpers: `clipboard_set(&str)` / `clipboard_get() -> Option<String>` using `OpenClipboard` / `SetClipboardData(CF_UNICODETEXT)` / `EmptyClipboard` / `CloseClipboard`. | new file `windows/clipboard.rs` |
| 11 | Ctrl+Shift+C copies selection, Ctrl+Shift+V pastes. Intercept in `windows_view.rs::handle_key_down` BEFORE the ctrl-letter branch. | `windows_view.rs` |

### Milestone C — robustness (~1h)

| # | Task | Files |
|---|---|---|
| 12 | `WM_SIZE`: propagate errors through a `?`-chain; log each stage. Ensure `renderer.resize`, `vt.resize`, `conpty.resize` either all succeed or we bail cleanly (don't leave state inconsistent). | `host_view.rs` |
| 13 | Atlas overflow: when `allocate` returns `None` in `GlyphCache::get_or_rasterize`, drop all entries + rebuild (cheap LRU stand-in). Log a warning. | `render/atlas.rs` |
| 14 | Panic hardening: replace `.unwrap()` in hot paths (Windows mod, render, atlas, pipeline) with `.expect(&'static str)` where the precondition is structural and documented, or bubble via `anyhow::Result`. | all `windows/*` |
| 15 | `ghostty_terminal_mode_get` binding + use: bracketed paste → wrap pasted text with `\x1b[200~...\x1b[201~`; DECCKM → use `\x1bOA/B/C/D` arrow sequences. | `vt.rs`, `windows_view.rs` |

### Out of scope today

- IME (WM_IME_*) — CJK input; deferred.
- Hyperlinks (OSC 8 rendering) — parsing exists upstream but needs UI.
- Full scrollback navigation — libghostty-vt has no viewport API; would
  need either upstream contribution or a secondary scrollback buffer.
- OSC 52 clipboard (remote copy) — needs OSC parser wiring.
- DComp sibling embedding (Phase 3d) — upstream GPUI work, separate track.

---

## Gotchas from this series of sessions

- **`CON` is a reserved DOS name**. Binary is `con-app.exe` via the
  `bin-con-app` feature and `wbuild` / `wrun` aliases. Crate dir is
  `crates/con-app/` not `crates/con/`.
- **`-Dsimd=false` for libghostty-vt on Windows**. SIMD requires
  linking simdutf C++ objects which zig doesn't bundle into the
  static archive.
- **WebFetch rate-limits** on raw.githubusercontent.com — if you hit
  it, wait a couple minutes.
- **GhosttyStyle is a versioned struct**. Caller MUST set `.size =
  sizeof(GhosttyStyle)` before each `row_cells_get(STYLE, &style)` so
  upstream writes the right range of bytes.
- **ClearType** needs either dual-source blend OR a 3-channel sample
  in the PS. Our current PS samples `.r` only. When switching to
  ClearType, update `ps_main` accordingly.
- **`D3D11_CULL_NONE` rasterizer** is intentional; don't "fix" it by
  enabling cull — we don't reason about winding order after the
  Y-flip.
- **HLSL `if (uint_val)` auto-converts to bool**. `if (attrs & 4u)`
  works.
- **`CON_GHOSTTY_VT_RENDER_STATE=0`** is an escape hatch — if
  `ghostty_render_state_new` starts crashing after a ghostty rev bump,
  set this and file a reproducer before bisecting.

---

## Verification after each milestone

On the user's Windows machine:

```powershell
cargo wbuild -p con --release
.\target\release\con-app.exe
```

Manual tests per milestone:

- **A.5**: crisp text at native DPI, matches Windows Terminal size.
- **B.mouse**: wheel scrolls tmux / nvim (where mouse tracking is on).
- **B.select**: click-drag highlights cells inversely; Ctrl+Shift+C
  copies to system clipboard (verify by pasting into Windows Terminal).
  Ctrl+Shift+V pastes into shell.
- **C.resize**: dragging the window edge keeps text aligned, no crash
  under rapid resize.
- **C.hardening**: no panics under stress — run a big `Get-ChildItem
  -Recurse` on a deep tree.

---

## Study docs index

Background reading in `docs/study/` — each is the research that
informed a decision above:

- `ghostty-vt.md` — libghostty-vt ABI deep dive, cell/style layout,
  render-state iterator. Anchor for Option A.
- `gpui-external-swapchain-upstream-pr.md` — proposed GPUI API to
  accept an external D3D11 swapchain as a DComp sibling visual. Phase
  3d plan.
- `gpui.md` — GPUI internals relevant to the Windows backend
  (DirectComposition tree, input routing, focus chain).
- `pr-12167-windows-apprt-reference.md` — upstream ghostty PR with
  Windows app-runtime reference code consulted for HWND + DComp
  patterns.
- `terminal-control-plane.md` — JSON-RPC 2.0 over socket/pipe,
  relevant to Windows named-pipe transport.
- `socket-control-patterns.md` — patterns for the `con-cli` client,
  cross-platform transport.
- `markdown-renderer-architecture.md`, `rig.md` — unrelated to Windows
  port but referenced from the agent panel.

And the implementation plan:

- `docs/impl/windows-port.md` — staged plan; keep this updated as
  phases land.

---

## Related postmortems

- `postmortem/2026-04-16-prepare-windows-port.md` — the first
  preparation PR (workspace rename, twin bin, cargo aliases, CI).
  Context for Phase 0.
- `postmortem/2026-04-18-windows-port-session-handover.md` — this
  doc.

---

## Kickoff prompt for next session

> Resume Windows port of con. Branch: `claude/prepare-windows-support-f7drP`.
> Full state + plan: `postmortem/2026-04-18-windows-port-session-handover.md`.
>
> Continue from Milestone A.5 (rendering polish: DPI, ClearType,
> IoskeleyMono). Commit + push per milestone. Linux cross-check via
> `CON_STUB_GHOSTTY_VT=1 cargo check -p con-ghostty --target
> x86_64-pc-windows-gnu --no-default-features`.
>
> Do not break any working rendering / input paths (typing, bold,
> italic, underline, strike, inverse, cursor). Read the handover doc
> end-to-end before editing.
