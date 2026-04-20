# Study: upstream GPUI PR — `attach_external_swap_chain_handle`

**Status**: not started. Self-contained brief so a fresh session in a
Zed worktree can draft the PR.

## Why

con renders the Windows terminal via a dedicated D3D11 renderer
(libghostty-vt → ConPTY → DirectWrite glyph atlas). Phase 3a hosts the
renderer in a `WS_CHILD` HWND parented to GPUI's main HWND. That
works — GPUI already binds the topmost DComposition target on the HWND,
so popups draw correctly over the child HWND — but it has three real
drawbacks vs. a first-class DComposition sibling visual:

1. **No blending with GPUI effects.** HWND children are opaque rects;
   rounded corners, drop shadows, and blur don't apply. We live with
   this today because con's design language is flat/borderless, but it
   precludes future effects on the terminal pane.
2. **Flickering DPI transitions** when dragging across monitors.
3. **Focus/IME/resize coordination** between two windowing layers.

The cleaner architecture: render the terminal to a
`DCompositionCreateSurfaceHandle`-backed swapchain (a kernel HANDLE,
not an HWND), and hand the handle to GPUI so it attaches the handle to
a child `IDCompositionVisual` under its existing compositor tree. DWM
then composites our content with GPUI's as siblings in one pass.

This is exactly the pattern Windows Terminal uses internally
(microsoft/terminal#10023) and what WebView2's composition-hosting API
exposes publicly
(`ICoreWebView2CompositionController::put_RootVisualTarget`).

## The PR

**Scope**: add one method to GPUI's Windows `WindowsWindow`:

```rust
impl WindowsWindow {
    /// Attach an external D3D11 swapchain, identified by a kernel
    /// HANDLE from `DCompositionCreateSurfaceHandle`, as a child
    /// DComposition visual inside this window at the given bounds.
    /// The handle is duplicated; the caller retains ownership of the
    /// original.
    ///
    /// Used by hosts that render their own content (terminals, video,
    /// custom GL surfaces) and want DWM to composite that content
    /// seamlessly with GPUI's own rendering (correct Z-order against
    /// popups, consistent present timing, unified DPI/resize).
    pub fn attach_external_swap_chain_handle(
        &self,
        handle: windows::Win32::Foundation::HANDLE,
        bounds: Bounds<Pixels>,
    ) -> Result<ExternalSurfaceHandle>;
}
```

`ExternalSurfaceHandle` is an opaque token the caller keeps to
reposition (`.set_bounds(...)`) and detach (`Drop`).

**Estimated diff**: ~50–80 LOC in `crates/gpui_windows/src/directx_renderer.rs`
(extend `DirectComposition` to track child visuals), ~30 LOC in
`crates/gpui_windows/src/window.rs` (public API), ~40 LOC of tests.

## Files to touch in `zed-industries/zed`

- `crates/gpui_windows/src/directx_renderer.rs:99-103`
  - `DirectComposition` struct: add `child_visuals:
    HashMap<SurfaceId, (IDCompositionVisual, Bounds<Pixels>)>`.
- `crates/gpui_windows/src/directx_renderer.rs:894-914`
  - `DirectComposition::new` / `set_swap_chain`: the existing single
    root-visual flow stays; add helpers to create extra child visuals
    and commit them.
- `crates/gpui_windows/src/window.rs:533-541`
  - Near the existing `impl HasWindowHandle`: add the public methods
    (`attach_external_swap_chain_handle`, and an
    `ExternalSurfaceHandle` type with `set_bounds` + `Drop`).
- `crates/gpui_windows/src/gpui_windows.rs:24-26`
  - Re-export the new public types from the `gpui_windows` crate root.
- `crates/gpui_windows/examples/external_surface.rs` (new)
  - Minimal repro: a tiny example that hands GPUI a red D3D11
    swapchain and positions it inside a window.
- `crates/gpui/src/platform.rs`
  - The cross-platform trait signature — add a placeholder method
    returning `Err(NotImplemented)` on macOS/Linux for now.
    macOS's CVPixelBuffer path is a future symmetric extension.

## Upstream concerns to pre-empt

mikayla-maki closed PR zed-industries/zed#24330 for being
Windows-only when a cross-platform story wasn't sketched. To avoid the
same fate:

- Present the macOS analog up front. On macOS the same contract can be
  implemented with `CAMetalLayer` via the existing `CVPixelBuffer`
  branch of `PaintSurface`. You don't have to implement macOS in this
  PR, but the API shape has to work for macOS — document it in the PR
  description and stub the method on macOS with a `// TODO` pointing
  at the CAMetalLayer approach.
- Linux: `wgpu::Texture` or dmabuf. Same stub + TODO.
- Show a concrete downstream consumer (con) that's blocked on this.
  Link to the con repo's `docs/impl/windows-port.md` Phase 3d row.

## Implementation steps

1. `git clone https://github.com/<your-user>/zed.git` and branch
   `feat/windows-external-swapchain`.
2. Read `crates/gpui_windows/src/directx_renderer.rs` end-to-end. The
   swapchain + DComp plumbing lives in one file; no spelunking needed.
3. Mirror the existing `set_swap_chain` path for a child visual:
   `comp_device.CreateVisual()` → `visual.SetContent(swapchain_from_handle)`
   → `comp_visual.AddVisual(child_visual, /*insertAbove=*/true, None)`
   → `comp_device.Commit()`.
4. The kernel HANDLE → swapchain conversion uses
   `IDXGIFactoryMedia::CreateSwapChainForCompositionSurfaceHandle`
   (see `microsoft/terminal` PR #10023 for the exact invocation).
5. Add the example + a smoke test.
6. Write the PR description citing: con's Phase 3d (this doc), the
   WebView2 composition-hosting precedent, Windows Terminal #10023,
   the closed PR #24330 and why this is scoped differently.

## Once merged

In con, Phase 3d swap:

```rust
// crates/con-ghostty/src/windows/render.rs
// Before (Phase 3a/3b):
let swapchain = dxgi_factory.CreateSwapChainForHwnd(&device, hwnd, &desc, ...)?;

// After (Phase 3d):
let surface_handle = DCompositionCreateSurfaceHandle(COMPOSITIONOBJECT_ALL_ACCESS, None)?;
let swapchain = dxgi_factory
    .CreateSwapChainForCompositionSurfaceHandle(&device, surface_handle, &desc, None)?;
// Hand the handle to GPUI:
window.attach_external_swap_chain_handle(surface_handle, pane_bounds)?;
```

The WS_CHILD HWND and `host_view.rs` become unnecessary; all Win32
window-proc plumbing goes away; focus and input come through GPUI's
normal event stream. This is the Phase 3d endgame.

## References

- Windows Terminal handle handoff pattern:
  <https://github.com/microsoft/terminal/pull/10023>
- `DCompositionCreateSurfaceHandle`:
  <https://learn.microsoft.com/en-us/windows/win32/api/dcomp/nf-dcomp-dcompositioncreatesurfacehandle>
- `IDXGIFactoryMedia::CreateSwapChainForCompositionSurfaceHandle`:
  <https://learn.microsoft.com/en-us/windows/win32/api/dxgi1_3/nf-dxgi1_3-idxgifactorymedia-createswapchainforcompositionsurfacehandle>
- WebView2 composition hosting (precedent for "host owns compositor,
  embed hands a visual in"):
  <https://learn.microsoft.com/en-us/microsoft-edge/webview2/concepts/windowed-vs-visual-hosting>
- PR that was closed (lessons on what to avoid):
  <https://github.com/zed-industries/zed/pull/24330>
- con Phase 3 plan: `docs/impl/windows-port.md`
