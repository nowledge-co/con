# Study: Ghostty Linux embed gap

This note narrows Linux Phase 2a from:

> "figure out whether Ghostty embedding can work on Linux"

to:

> "what exact upstream Ghostty changes would con need before a Linux
> embed path becomes viable?"

## Conclusion

A viable Linux embed path for con requires **three** upstream Ghostty
deliverables, not one:

1. **C ABI surface expansion**
2. **embedded Linux platform implementation**
3. **working embedded OpenGL renderer lifecycle**

Today, none of those three exists in a con-consumable way.

## What exists already

Ghostty's embedded C API is already broad in terminal semantics:

- app lifecycle
- surface lifecycle
- resize
- scale factor
- focus
- keyboard
- text / preedit
- mouse / scroll / pressure
- clipboard request completion
- selection reading
- text reading

The exported API surface itself is not the problem. Relevant public
functions already exist in `ghostty.h`:

- `ghostty_app_new`, `ghostty_app_tick`
- `ghostty_surface_new`, `ghostty_surface_draw`,
  `ghostty_surface_refresh`
- `ghostty_surface_set_size`, `ghostty_surface_set_content_scale`
- `ghostty_surface_key`, `ghostty_surface_text`,
  `ghostty_surface_preedit`
- `ghostty_surface_mouse_*`
- `ghostty_surface_complete_clipboard_request`
- `ghostty_surface_read_selection`, `ghostty_surface_read_text`

That means con does **not** need Ghostty to invent a whole new embed API.

## Gap 1: Linux is missing from the public embed platform contract

The current C ABI only defines:

- `GHOSTTY_PLATFORM_MACOS`
- `GHOSTTY_PLATFORM_IOS`

and the platform union only carries:

- `ghostty_platform_macos_s { void* nsview; }`
- `ghostty_platform_ios_s { void* uiview; }`

The embedded Zig runtime matches that exactly:

- `PlatformTag` only has `macos` and `ios`
- `Platform` union only has `macos` and `ios`
- `Platform.init` only understands those tags

### Required upstream delta

Ghostty needs a Linux platform arm in both the C ABI and the Zig runtime.

Minimum shape:

1. Add `GHOSTTY_PLATFORM_LINUX`
2. Add a Linux member to `ghostty_platform_u`
3. Add a Linux platform variant in `apprt/embedded.zig`
4. Teach `Platform.init` how to validate and instantiate it

## Gap 2: the Linux host contract is undefined

Adding `GHOSTTY_PLATFORM_LINUX` is not enough. The bigger question is:

> what object does an embedder pass to Ghostty on Linux so Ghostty knows
> where and how to render?

On macOS the answer is clear:

- pass an `NSView`
- Ghostty attaches its renderer to that host view

On Linux, the equivalent contract is not yet defined in the embed API.

There are several plausible shapes, and Ghostty has not committed to one
publicly in the embeddable API:

- Wayland/X11 native window/surface handles
- host-provided OpenGL context hooks
- host-provided `getProcAddress` + make-current/swap callbacks
- offscreen render target / texture export

### Required upstream delta

Ghostty needs to define one Linux host contract explicitly.

The most plausible families are:

#### Option A: native-handle contract

Ghostty accepts platform-native Linux handles.

Example rough shape:

- Wayland:
  - `wl_display*`
  - `wl_surface*`
- X11:
  - display pointer / connection
  - window ID / visual info

This looks closest to how GPUI itself constructs Linux renderers from raw
Wayland/X11 window handles.

#### Option B: embedder-owned GL context contract

Ghostty accepts callbacks for:

- make current
- clear current
- swap buffers / present
- get-proc-address

This is more portable across Wayland/X11 but pushes more renderer-host
coordination onto the embedder.

#### Option C: offscreen contract

Ghostty renders offscreen and exposes pixels or textures for the host to
paint/composite.

This may be attractive for GPUI interop, but it is a bigger API design
change than the current macOS embed model.

### What con needs from Ghostty

con does not need the final perfect cross-platform abstraction. It needs:

- one stable Linux contract
- documented ownership/threading rules
- documented resize + scale + present semantics

## Gap 3: embedded OpenGL rendering is explicitly stubbed today

This is the strongest blocker.

`3pp/ghostty/src/renderer/OpenGL.zig` already distinguishes:

- `apprt.gtk`
- `apprt.embedded`

But for `apprt.embedded`:

- `surfaceInit` does nothing
- `threadEnter` does nothing
- `threadExit` does nothing
- comments explicitly say libghostty is "strictly broken for rendering on
  this platforms"

That means Ghostty's Linux embed path cannot be unlocked by header work
alone. The renderer lifecycle itself is unfinished.

### Required upstream delta

Ghostty needs a real embedded OpenGL implementation for non-Darwin
targets.

At minimum that includes:

1. `surfaceInit` for embedded OpenGL
   - load GL entry points correctly for the chosen host contract
2. `finalizeSurfaceInit`
   - perform any main-thread setup the renderer requires before draw
3. `threadEnter`
   - define whether render-thread GL access is legal and how contexts are
     made current
4. `threadExit`
   - release context-bound resources correctly
5. present/swap semantics
   - define how `present()` reaches the actual host-visible framebuffer or
     target

## What this means for con

For con, a "Ghostty-first Linux embed path" is viable only if Ghostty
upstream is willing to provide:

- Linux embed platform tags
- a Linux host contract
- working embedded OpenGL runtime behavior

Until then, con cannot honestly say "Linux is basically macOS."

## Suggested upstream issue structure

If we open an upstream Ghostty issue, it should ask for:

1. Linux platform support in the embedded C API
2. a documented Linux host-surface or GL-context contract
3. implementation of the currently-stubbed embedded OpenGL renderer path

and it should cite the exact local lines in:

- `include/ghostty.h`
- `src/apprt/embedded.zig`
- `src/renderer/OpenGL.zig`

## Scope boundary

This study intentionally does **not** solve GPUI interop. That is a
separate Phase 2b question.

This study only answers:

> what would Ghostty itself need to change before con could even try to
> embed Linux Ghostty the way it embeds macOS Ghostty?

## References

- `docs/impl/linux-port.md`
- `docs/study/linux-port-feasibility.md`
- `3pp/ghostty/include/ghostty.h`
- `3pp/ghostty/src/apprt/embedded.zig`
- `3pp/ghostty/src/renderer/OpenGL.zig`
