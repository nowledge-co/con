# Windows maximize/readback hang

## What happened

On Windows, maximizing a terminal pane into a large alt-screen app such as Neovim could leave the pane apparently fine until the next interactive update. Typing or otherwise triggering a redraw after the maximize could then leave con marked "Not Responding", and the bottom rows often appeared stale or missing.

## Root cause

The Windows renderer's staging ring still treated unread readback textures as a queue that needed preserving. After a large resize, one fullscreen readback could remain in flight. The next interactive render path then tried to synchronously drain either that older slot or the newly submitted fullscreen-sized slot on GPUI's UI thread. When the GPU had not finished the copy yet, `Map()` blocked long enough to freeze the app.

The bug was structural: staging textures are only cached copies of already-known terminal state. The VT snapshot is authoritative, so preserving an older unread readback is never worth stalling the UI thread.

## Fix applied

- Changed the Windows staging ring to mailbox semantics.
- Drains of older in-flight slots are now non-blocking only.
- If the ring is truly backlogged, the renderer returns `Pending` and schedules another prepaint instead of blocking.
- When all slots are busy, the renderer reclaims the oldest unread slot for the newest frame instead of rescuing stale pixels.
- The synchronous low-latency path remains for interactive renders when the newest frame can still land in a clean slot.
- Follow-up hardening: the VT snapshot now uses Ghostty's actual render-state geometry during asynchronous resize catch-up, and the renderer invalidation key now includes snapshot geometry so a size-only catch-up frame is never skipped.

## What we learned

- Display pipelines should treat unread intermediate readbacks as disposable unless they are the system of record.
- A "low latency" path on the UI thread needs an explicit backlog guard; otherwise it turns into an unbounded stall under the exact workloads users notice most.
- Maximize/fullscreen bugs on Windows are often scheduling problems in the render pipeline, not geometry mistakes from the taskbar or work-area calculation.
- Ghostty's render-state dimensions can intentionally lag the host surface by one or two frames during resize; consumers must treat render-state geometry as authoritative for each snapshot instead of assuming the requested size has already taken effect.
