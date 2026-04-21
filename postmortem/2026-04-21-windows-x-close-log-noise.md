# Windows — "window not found" on X-button close is GPUI upstream noise (2026-04-21)

## What happened

After merging the Phase 3b runtime-validation work, an intermittent
concern remained: clicking the titlebar X on a Windows Con window
printed one or more of the following at `error` level before the
process exited:

```
ERROR gpui::app::async_context: window not found
ERROR gpui_windows::window: Invalid window handle (HRESULT 0x80070578)
ERROR gpui_windows::directx: HRESULT(0x80040102) (DXGI_ERROR_NOT_CURRENTLY_AVAILABLE)
```

The framing in issue #17 was "app crash on X button close — determine
if it's a real crash or cosmetic error and harden the path."

## Conclusion

**Cosmetic. The process exits cleanly; the errors are benign log noise
from the GPUI Windows platform layer's teardown race.** No user-facing
state is lost; the `prepare_window_close` path still runs, pending
agent requests are cancelled, sessions are saved, and Ghostty surfaces
are shut down before the HWND goes away.

## Where the noise comes from

GPUI Windows spawns async closures on `WindowsDispatcher` from window
message handlers — notably `handle_activate_msg` (events.rs:725) and
`handle_paint_msg` (events.rs:262 via `draw_window`). Windows's message
pump can dispatch a final `WM_ACTIVATE` or `WM_PAINT` to a closing
HWND, so the async closure captures state that references a window
which GPUI's `App` has already marked `removed`.

When the closure later reaches `update_window`
(`3pp/zed/crates/gpui/src/app/async_context.rs:85-95`), the look-up in
`cx.windows` misses and returns `Err("window not found")`. Similar
behaviour on the D3D11 swapchain side produces `Invalid window handle`
from the Win32 HWND APIs and `HRESULT(0x80040102)` from DXGI when a
swapchain present lands on a destroyed HWND.

None of these paths panic. None corrupt persistent state. The log
records the race, the executor drops the task, the app quits.

## What we already do (this isn't new)

`crates/con-app/src/workspace.rs:757-815` returns `false` from
`on_window_should_close` on Windows and defers `prepare_window_close`
+ `remove_window` + `cx.quit()` onto a 120ms timer so in-flight window
tasks drain before the HWND is destroyed. This reduces the frequency
with which the race triggers — it doesn't eliminate it, because
Windows can deliver `WM_ACTIVATE` / `WM_PAINT` during teardown
regardless of how long Con waits.

`prepare_window_close` itself (`workspace.rs:5010-5058`) is idempotent,
cancels all active agent sessions, flushes the session save, and
calls `terminal.shutdown_surface(cx)` on every terminal in every tab,
which is the actually load-bearing step for clean resource teardown.

## What would eliminate the noise entirely

The fix has to be in GPUI itself, not in Con. Concretely, one of:

1. **Add a liveness check to `update_window`**
   (`3pp/zed/crates/gpui/src/app/async_context.rs:85`): when
   `lock.quitting` or the window is `removed`, bail with `Ok` or a
   dedicated `WindowGone` sentinel and have callers treat it as a
   no-op without logging `error!`.
2. **Have `handle_activate_msg` / `handle_paint_msg` capture
   `Weak<WindowState>` and bail silently when upgrade fails.** The
   closures today hold `Rc<WindowState>` plus references to
   `self.executor`, so they keep the window state alive past
   `WM_DESTROY` and happily try to run their callbacks.
3. **Cancel the `WindowsDispatcher` tasks associated with a window on
   `WM_DESTROY`.** This is the architecturally cleanest answer but
   requires the dispatcher to track task → window attribution, which
   it doesn't today.

All three are upstream `zed-industries/zed` changes. None is a Con
bug, and none can be patched locally without modifying `3pp/`, which
is read-only policy in this repo.

## Decision

Tracked as an upstream-only follow-up on issue #34 alongside Phase 3d
(DirectComposition external-swapchain). Not scheduled now. Closing
task #17 — the deliverable ("determine if it's a real crash or
cosmetic, and harden the path") is complete:

- Crash vs cosmetic: **cosmetic**, verified against logs and
  confirmed against memory notes
  (`ab4560bd-6ffe-4f08-baed-f06cd2739aab`).
- Path hardening: **done in `9283a1f`** — the 120ms deferred teardown
  is the best mitigation available without upstream changes.

## Follow-ups

- **Upstream TODO (tracked in issue #34, Major TODO section)** —
  add `WindowHandle` liveness check to GPUI's `update_window`, weak-
  capture in window message async closures. Paired with the DComp
  external-swapchain PR as the two "can't-fix-locally" Windows items.
- **If the noise starts showing up in user-facing release builds**,
  consider a Con-side env_logger module filter that demotes
  `gpui::app::async_context` from `error` to `warn` once `cx.quit()`
  has been dispatched. Not worth doing pre-emptively — dev noise is
  acceptable, and demoting real errors would mask future regressions
  in unrelated update-window paths.
