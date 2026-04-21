# Study: GPUI Linux interop gap

This note narrows Linux Phase 2b from:

> "figure out how con could display a Linux terminal surface inside a
> GPUI window"

to:

> "what exact GPUI-side capability is missing today, and what fallback
> paths remain if that capability does not arrive?"

## Conclusion

GPUI Linux already gives con a real native host shell, but it does
**not** currently expose a public child-composition contract for a
foreign Linux-rendered terminal surface.

Today GPUI Linux provides:

1. **top-level native windows**
2. **raw Wayland/X11 host handles**
3. **GPUI-owned wgpu rendering**

It does **not** yet provide a downstream public API for:

1. **embedding a foreign Linux surface inside the view tree**
2. **attaching an external GPU surface/texture as a child pane**
3. **zero-copy Linux texture import for arbitrary downstream renderers**

That means a Ghostty-first Linux embed path is blocked on more than
Ghostty alone. Even if Ghostty grew a Linux embedded API tomorrow, con
would still need either:

- new GPUI interop support, or
- a GPUI-owned rendering path instead of a foreign-surface embed

## What GPUI Linux already gives con

### 1. Real Linux windows and compositor integration

`gpui_linux` is not a placeholder shell.

Its platform layer:

- chooses Wayland or X11 at runtime
- opens real platform windows through `PlatformWindow`
- uses `WgpuRenderer` for drawing
- uses `CosmicTextSystem` for text

Relevant local sources:

- `3pp/zed/crates/gpui/src/platform.rs`
- `3pp/zed/crates/gpui_linux/src/linux/platform.rs`
- `3pp/zed/crates/gpui_linux/src/linux/wayland/window.rs`
- `3pp/zed/crates/gpui_linux/src/linux/x11/window.rs`
- `3pp/zed/crates/gpui_wgpu/src/wgpu_renderer.rs`

Important consequence:

- con already has a real Linux app shell
- window creation, input dispatch, clipboard, menus, and top-level GPU
  presentation are not the core blocker

### 2. Raw host handles are available

`PlatformWindow` inherits both:

- `HasWindowHandle`
- `HasDisplayHandle`

and the Linux platform windows implement those contracts with native raw
handles:

- Wayland: `wl_surface*` and `wl_display*`
- X11: X window handle plus display/connection handle

This is enough for GPUI's own renderer, which creates a `wgpu` surface
from raw window handles.

Important consequence:

- GPUI already knows how to bootstrap rendering from Linux native
  handles
- any future external-surface API should fit into that worldview rather
  than inventing a GTK-only embedding lane

## What GPUI does not expose today

### Gap 1: no public foreign-surface embed primitive

At the public `gpui` layer, there is no Linux equivalent of:

- "attach this external child surface to the window"
- "host this foreign compositor object inside these bounds"
- "accept an external GPU texture handle for scene composition"

The `PlatformWindow` trait exposes top-level window management and raw
handle access, but no child-surface API.

Important consequence:

- con cannot currently hand GPUI a Wayland child surface, X11 child
  window, GL drawable, or dmabuf-backed surface and ask GPUI to compose
  it inside a pane

### Gap 2: existing image APIs are CPU-image paths, not GPU interop

The public downstream image path in GPUI is `RenderImage`, surfaced via:

- `ImageSource::Render`
- `Window::paint_image`

That path inserts CPU-backed RGBA bytes into GPUI's sprite atlas and
then paints atlas tiles into the scene.

This is useful as a fallback, but it is not a true foreign-surface or
external-texture composition boundary.

Important consequence:

- a Linux terminal rendered elsewhere can be displayed in GPUI today
  only by CPU readback / image upload style techniques
- that is viable as a compatibility path, but not the long-term
  architecture we want if Ghostty keeps its own GPU renderer

### Gap 3: `paint_surface` exists only for macOS today

GPUI already has one special-case surface path:

- `Window::paint_surface`
- `elements::surface`

But it is compiled only on macOS and only for `CVPixelBuffer`.

There is no Linux sibling today for:

- dmabuf
- EGL image
- external `wgpu::Texture`
- Wayland/X11 child surface reference

Important consequence:

- the closest conceptual extension point already exists in GPUI
- but the existing implementation is macOS-specific, not a cross-platform
  external-surface abstraction

## Why Wayland dmabuf is not enough by itself

`gpui_linux` does contain Wayland dmabuf probing logic. That proves GPUI
cares about compositor/GPU identity and future interop opportunities.

But that code is:

- backend-internal
- used for compositor GPU hinting
- not exposed as a downstream texture-import API

So "GPUI mentions dmabuf internally" does **not** mean con can already
use dmabuf as a supported pane-composition interface.

## Architecture consequences for con

This leaves con with three realistic Linux integration lanes.

### Option A: GPUI gains a public Linux external-surface API

This is the Linux analog of the upstream Windows external-swapchain
study:

- GPUI would accept some Linux-native child composition object
- con would hand it a Ghostty-rendered surface or texture export
- GPUI would position and clip it inside the pane tree

Plausible shapes:

- Wayland dmabuf / texture import
- X11 texture or pixmap import
- cross-platform external-texture abstraction at the `gpui` layer

Benefits:

- keeps Ghostty as the Linux renderer owner
- closest long-term match to the macOS embed architecture

Costs:

- requires upstream GPUI API and renderer work
- likely needs a cross-platform story to be acceptable upstream

### Option B: GPUI-owned render path

con keeps GPUI as the sole Linux renderer and only reuses Ghostty for
terminal semantics if possible.

This means:

- GPUI paints the terminal cells itself
- Ghostty embed is not needed for final on-screen composition

Benefits:

- no foreign-surface composition problem
- downstream ownership is clear

Costs:

- much closer to the Windows architecture
- forfeits the main benefit of Ghostty's existing Linux renderer

### Option C: CPU-readback bridge

A stopgap lane:

- render elsewhere
- read back pixels
- push them through `RenderImage`

Benefits:

- technically possible with today's public GPUI APIs

Costs:

- likely too expensive and inelegant for the intended long-term terminal
  path
- does not solve input/focus/composition semantics cleanly

## Recommended GPUI-side ask

If con pursues the Ghostty-first Linux path, the GPUI-side upstream ask
should be framed narrowly:

1. define one public external-surface or external-texture composition
   contract that can live at the `gpui` layer
2. let Linux implement it with Linux-appropriate primitives
3. leave unsupported platforms as `NotImplemented` if necessary

The important thing is not to ask for "GTK widget embedding." GPUI
Linux's own architecture is already native-window + wgpu, so the right
abstraction is compositor/texture interop, not foreign toolkit hosting.

## Decision for Phase 2c

Phase 2b now has a bounded output:

- **Ghostty-first Linux embed remains plausible only if both Ghostty and
  GPUI accept upstream interop work**
- if either upstream delta is too large or too slow, con should choose a
  local Linux backend and stop modeling Linux as "macOS with different
  headers"

That means the next Linux milestone is an architecture decision, not
more reconnaissance.

## References

- `docs/impl/linux-port.md`
- `docs/study/linux-port-feasibility.md`
- `docs/study/ghostty-linux-embed-gap.md`
- `docs/study/gpui-external-swapchain-upstream-pr.md`
- `3pp/zed/crates/gpui/src/platform.rs`
- `3pp/zed/crates/gpui/src/window.rs`
- `3pp/zed/crates/gpui/src/elements/img.rs`
- `3pp/zed/crates/gpui/src/elements/surface.rs`
- `3pp/zed/crates/gpui_linux/src/linux/platform.rs`
- `3pp/zed/crates/gpui_linux/src/linux/wayland/window.rs`
- `3pp/zed/crates/gpui_linux/src/linux/x11/window.rs`
- `3pp/zed/crates/gpui_linux/src/linux/wayland/client.rs`
- `3pp/zed/crates/gpui_wgpu/src/wgpu_renderer.rs`
