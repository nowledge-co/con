# Windows codex frame freeze (2026-05-03)

## What happened

Issue #114: on Windows, codex's TUI output stopped refreshing on screen
unless the user pressed Enter or clicked the terminal. The reporter also
noticed that codex's input cursor visually "jumped" several rows when
they clicked — the catch-up frame painted the cursor at its true row
while the screen had been frozen showing a much older row.

The same pattern reproduces with any TUI that never lets the VT
quiesce: `watch`, `top`, `btop`, streaming output, spinners.

## Root cause

The Windows renderer (`crates/con-ghostty/src/windows/render/mod.rs`)
uses a two-slot D3D11 staging-texture ring with mailbox semantics:
draws are queued, copies of the render target are submitted into a
staging slot, and the next prepaint maps the slot to read the BGRA
bytes back into a GPUI image.

`Renderer::render` only chose a `drain_target` when `needs_draw` was
false:

```rust
let drain_target = (!needs_draw).then(|| ring.oldest_in_flight()).flatten();
```

The intent was a "newest-frame-wins" mailbox: if a fresher VT snapshot
exists, don't waste UI-thread time mapping an older slot whose pixels
are about to be superseded. After submitting the fresh copy the path
returned `Pending` and relied on the next prepaint to drain it.

That works when the VT eventually quiets down. With a continuously
streaming TUI it doesn't:

1. PTY chunk arrives, wake fires, prepaint runs.
2. `snapshot.generation` is new → `needs_draw = true`.
3. We submit a fresh copy, skip drain, return `Pending`, `cx.notify()`.
4. Before the next prepaint, the GPU finishes the copy AND another PTY
   chunk arrives. New generation again.
5. Goto 2 — forever. The just-submitted copy never gets mapped.

Result: `cached_image` was frozen at whatever frame happened to land
when output began. A user click flipped `prefer_latest = true`, which
took the `block_drain` branch, blocked until the latest copy was ready,
and presented it — making the screen "jump" forward in one step,
including the cursor row.

## Fix

`crates/con-ghostty/src/windows/render/mod.rs::Renderer::render`:

- Always compute `drain_target = ring.oldest_in_flight()` regardless of
  `needs_draw`. Try-draining the older slot is non-blocking and cheap.
- After submitting the fresh copy, if `drained` is `Some` *and*
  `prefer_latest` is false, return `RenderOutcome::Rendered(...)` from
  that prior readback instead of `Pending`.

Two important guards keep the original behavior intact in cases the
naive ungate would break:

- `discard_oldest_in_flight()` stays gated on `!needs_draw`. Under
  `needs_draw=true` the oldest slot is the most likely to be GPU-
  ready; discarding it would force the drain onto the newer (still
  in-flight) slot. On GPUs where copies take more than one prepaint
  cycle, that loop reproduces the very freeze this fix is meant to
  remove (cursor / Bugbot review on PR #116).
- The drained-during-submit shortcut is gated on `!prefer_latest`.
  With `prefer_latest` set, the user is waiting on a specific fresh
  frame — a mouse/key event, paste echo, or a `low_latency_generation_target`
  set by `host_view` — and the `block_drain` branch above already
  tried to deliver it. Returning a stale readback would prematurely
  satisfy `host_view`'s "non-Pending → clear target" rule and drop
  the generation target before the user-targeted frame is ever
  presented (CodeRabbit review on PR #116).

Effect: with continuous output the screen now lags the live VT by at
most one frame per prepaint instead of freezing indefinitely. The
just-submitted copy is still in flight and will be picked up by the
next prepaint, so the pipeline depth is unchanged. User-driven
interactive frames keep their fast-path through `block_drain` and
their generation-target handshake intact.

The original "no slideshow during burst" intent (`ls`, `dir`, `clear`)
is largely preserved because such bursts are short — at most one or
two intermediate frames are presented before the burst ends and the
quiet-VT path returns the freshest frame as before.

macOS and Linux are unaffected: this code is gated by
`#[cfg(target_os = "windows")]` and the macOS path uses the embedded
libghostty NSView with its own Metal compositor.

## What we learned

- Mailbox semantics need a forward-progress invariant. "Skip draining
  an older slot when a fresher one exists" only terminates if the
  fresher slot eventually drains; under a busy producer it never does.
- For interactive UIs, presenting a one-frame-old image is strictly
  better than freezing on the assumption that "the next frame will be
  fresher." Falling back to drain-on-submit gives us that floor without
  giving up the newest-wins preference for quiet-VT cases.
- Bug reports framed as "needs a click to refresh" almost always point
  at a redraw scheduler that's gated on input events; on Windows here
  the gate was the `prefer_latest` block_drain path, not the
  `cx.notify()` plumbing.
