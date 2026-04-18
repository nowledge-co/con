# Windows Port — Session Handover (2026-04-18)

Multi-session context for continuing the Windows port of con. Paste the
"Kickoff prompt" below into a fresh Claude session and it has everything
it needs.

---

## Branch

`claude/prepare-windows-support-f7drP` (pushed, up to date on origin).

Most recent commits:

```
769b6ee fix(windows): bind Globals cbuffer to the pixel shader
b7ae5ca feat(windows): render bold/italic/underline/strikethrough + clear atlas on rebuild
9478c78 feat(windows): keyboard input; translate \n→\r for ConPTY
a350e23 feat(windows): render inverse-video block cursor
4eeffe5 fix(windows): use DirectWrite metrics for cell width/height
805c947 fix(windows): fall back to palette default fg/bg on unstyled cells
484e82d fix(windows): decode codepoint via ghostty_cell_get (RAW is opaque)
```

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
- Milestone A (rendering polish) pushed and verified by screenshot.

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
