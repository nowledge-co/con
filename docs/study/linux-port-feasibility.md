# Study: Linux port feasibility

This note answers the question behind con's Linux port more precisely
than the top-level tracker:

> Is Linux closer to macOS or closer to Windows for con?

Answer:

- **closer to macOS in upstream capability**
- **closer to Windows in embedding difficulty**

That is why Linux is likely easier than Windows, but still materially
harder than macOS.

## What we verified locally

### 1. GPUI/Zed already has a real Linux backend

`gpui_linux` is not a placeholder:

- it selects Wayland or X11 at runtime from `WAYLAND_DISPLAY` /
  `DISPLAY`
- it uses native `PlatformWindow` implementations for both paths
- it renders through GPUI's own `WgpuRenderer`
- it uses `CosmicTextSystem`

Relevant local sources:

- `3pp/zed/crates/gpui_linux/src/linux.rs`
- `3pp/zed/crates/gpui_linux/src/linux/platform.rs`
- `3pp/zed/crates/gpui_linux/src/linux/wayland/window.rs`
- `3pp/zed/crates/gpui_linux/src/linux/x11/window.rs`

Important consequence:

- con already has a credible Linux host app shell via GPUI
- the host shell is not the blocker

### 2. Ghostty already has a real Linux runtime

Ghostty's upstream Linux path is real:

- GTK application runtime
- OpenGL renderer
- PTY/runtime path

Relevant local sources:

- `3pp/ghostty/src/apprt/gtk/`
- `3pp/ghostty/src/renderer/OpenGL.zig`

Important consequence:

- Linux should not be treated like Windows, where con had to assume the
  upstream terminal runtime was unusable for embedding

### 3. con cannot consume Ghostty Linux the same way it consumes macOS Ghostty

This is the main blocker.

Ghostty's public embedded C API currently exposes only macOS and iOS
platform tags:

- `ghostty_platform_e` only includes `GHOSTTY_PLATFORM_MACOS` and
  `GHOSTTY_PLATFORM_IOS`
- `ghostty_platform_u` only includes `macos`/`ios`
- the embedded runtime `Platform` union only accepts those variants

Relevant local sources:

- `3pp/ghostty/include/ghostty.h`
- `3pp/ghostty/src/apprt/embedded.zig`

Important consequence:

- con cannot just "point Linux at libghostty" today
- there is no Linux host-surface contract in the embeddable API

## Stronger finding: Ghostty's embedded OpenGL path is explicitly broken today

This matters because it narrows the architecture.

`3pp/ghostty/src/renderer/OpenGL.zig` contains explicit TODOs for
`apprt.embedded`:

- `surfaceInit` does nothing for `apprt.embedded`
- `threadEnter` does nothing for `apprt.embedded`
- comments say libghostty is "strictly broken for rendering on this
  platforms"

That means a Linux embed path needs more than:

- adding `GHOSTTY_PLATFORM_LINUX`
- adding a Linux union arm in `ghostty_surface_config_s`

It also needs actual embedded-renderer work upstream in Ghostty.

## What GPUI Linux gives con today

GPUI Linux exposes:

- raw Wayland/X11 window/display handles
- its own GPU renderer bound to those platform windows
- normal top-level window kinds (`Normal`, `Floating`, `Dialog`,
  Wayland `LayerShell`)

What it does **not** currently expose as a ready-made public contract:

- embed a foreign Linux surface/widget inside the view tree
- hand GPUI a foreign GPU surface and let it compose it as a child pane
- zero-copy external-texture composition API for arbitrary downstream
  renderers

The nearest existing downstream technique in con is the Windows path:

- render elsewhere
- CPU-read back BGRA
- wrap it as `RenderImage`
- paint it through GPUI's image element

That works, but it is not equivalent to true foreign-surface embedding.

## Linux architecture options

### Option A — upstream-first Ghostty embed

Goal:

- extend Ghostty's embedded API for Linux
- make Ghostty's embedded OpenGL path real
- find a host/render interop story that a GPUI app can consume

Concrete Ghostty delta likely needed:

1. Add Linux platform tags and C ABI structs to `ghostty.h`
2. Add Linux `Platform` variants in `apprt/embedded.zig`
3. Define the host contract for an embedded Linux surface
4. Implement embedded OpenGL context lifecycle for non-Darwin targets
5. Define render-thread semantics for embedded OpenGL on Linux
6. Expose whatever "present" or "draw now" handshake a non-GTK host
   needs

Benefits:

- maximum reuse of Ghostty's own Linux runtime
- long-term closest match to macOS architecture

Costs:

- upstream work in Ghostty is required
- likely also some GPUI interop work
- Wayland/X11 host contracts are more fragmented than AppKit

### Option B — local Linux backend inside con

Goal:

- keep GPUI Linux as the host shell
- build terminal integration locally in `con-ghostty/src/linux/`

Likely shape:

1. Unix PTY lifecycle (`forkpty` / `openpty`)
2. terminal-state contract, likely `libghostty-vt`
3. GPUI-native rendering path on Linux
4. keyboard/mouse/clipboard/selection integration

Benefits:

- con can start independently of Ghostty embed upstream work
- architecture is fully under con's control
- shares some engineering shape with the Windows backend

Costs:

- duplicates more terminal integration locally
- gives up the strategic advantage of Ghostty's existing Linux app/runtime

## Current recommendation

Do not start Linux by writing a renderer.

The right order is:

1. Freeze the exact Ghostty upstream delta needed for Linux embed
2. Freeze the exact GPUI interop delta needed, if any
3. Decide whether those deltas are small enough to pursue
4. Only then choose between:
   - upstream-first embed
   - local Linux backend

## Effort implication

This study narrows the effort bands:

- **best case**: Ghostty embed delta is moderate and GPUI only needs a
  small interop addition
  - Linux effort is meaningfully smaller than Windows
- **worst case**: Ghostty embed delta is large or blocked, and con must
  build a local Linux backend
  - Linux effort approaches Windows territory, though still with better
    upstream platform context

Practical conclusion:

- Linux is not a "small follow-up" to Windows
- Linux is also not another full unknown
- the next milestone should be a bounded feasibility decision, not a
  backend implementation sprint

## Decision gate

Before implementing a real Linux terminal backend, answer these two
questions explicitly:

1. Can Ghostty embedded Linux be made real without a large upstream
   project?
2. If yes, can GPUI host that result without invasive compositor work?

If either answer is "no" or "not soon," con should proceed with a local
Linux backend and stop pretending the macOS embed model is available.

## References

- `docs/impl/linux-port.md`
- `3pp/ghostty/include/ghostty.h`
- `3pp/ghostty/src/apprt/embedded.zig`
- `3pp/ghostty/src/apprt/gtk/`
- `3pp/ghostty/src/renderer/OpenGL.zig`
- `3pp/zed/crates/gpui_linux/`
