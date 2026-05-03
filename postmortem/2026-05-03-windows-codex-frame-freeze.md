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

The elegant move is to **decouple presentation rate from VT update
rate**, so mailbox-for-bursts and forward-progress-for-streaming fall
out of one rule rather than two competing modes.

`crates/con-ghostty/src/windows/render/mod.rs::Renderer::render`:

1. Always compute `drain_target = ring.oldest_in_flight()` regardless
   of `needs_draw`. Try-draining is non-blocking and cheap.
2. After submitting the fresh copy, if `drained` is `Some` AND
   `prefer_latest` is false AND `presentation_due()` returns true,
   return `RenderOutcome::Rendered(...)` from that prior readback
   instead of `Pending`.
3. Every `Rendered` exit point stamps `last_presented_at = Some(now)`.
   `presentation_due()` is gated on `MIN_FALLBACK_PRESENT_INTERVAL`
   (one 60 Hz vsync = 16 ms).

Three guards keep the established perf wins from the 2026-04-26
postmortem (`windows-command-render-latency.md`) intact:

- **`discard_oldest_in_flight()` stays gated on `!needs_draw`.** Under
  `needs_draw=true` the oldest slot is the most likely to be GPU-
  ready; discarding it would force the drain onto the newer (still
  in-flight) slot. On GPUs where copies take more than one prepaint
  cycle, that loop reproduces the very freeze this fix is meant to
  remove (Bugbot review on PR #116).
- **The drained-during-submit shortcut is gated on `!prefer_latest`.**
  With `prefer_latest` set, the user is waiting on a specific fresh
  frame — a mouse/key event, paste echo, or a
  `low_latency_generation_target` set by `host_view` — and the
  `block_drain` branch above already tried to deliver it. Returning a
  stale readback would prematurely satisfy `host_view`'s "non-Pending
  → clear target" rule and drop the generation target before the
  user-targeted frame is ever presented (CodeRabbit review on PR
  #116).
- **The shortcut is gated on `presentation_due()`.** Short bursts
  (`ls`, `dir`, `clear`) finish under one cap interval, so the
  fallback never fires for them and the established mailbox snap-to-
  final behavior is preserved exactly. Sustained TUIs hit the cap
  and update at 60 Hz, which is the GPUI/vsync ceiling — presenting
  faster is wasted work because GPUI only paints once per refresh,
  and each present pays a full-frame Map + memcpy +
  `bgra_frame_to_image` + sprite-atlas upload.

Effect:

- Continuous-output TUIs (codex, top, watch, btop): screen updates
  at 60 Hz with bounded image-handoff cost — no freeze, no churn
  beyond what GPUI would composite anyway.
- Burst commands (`ls`, `dir`, `clear`): unchanged from the
  04-26 mailbox tuning — snap-to-final, no slideshow.
- User-driven echo / paste / click: unchanged — `block_drain`
  fast path still wins and generation-target handshake stays
  honest.
- Resize drag: capped at 60 Hz feedback (was unbounded before the
  cap), reducing image churn during interactive resize.

The wider long-term answer remains direct swap-chain composition into
GPUI's DirectComposition tree (called out in the 2026-04-26 PM); this
fix is the right tactical bridge inside the existing staging-ring
architecture and will be retired together with that swap-chain work.

## How the fallback composes with `host_view`'s burst arming

Every input event in `host_view.rs` arms `low_latency_burst_until` for
`LOW_LATENCY_BURST_WINDOW = 750 ms`. While that window is active,
`burst_low_latency_active()` returns true, so `prefer_latest` is true,
so the fallback's `!prefer_latest` gate keeps it suppressed — the
`block_drain` fast path owns the entire post-input window.

The fallback only takes over once 750 ms have elapsed since the last
input. That is exactly the regime issue #114 covered: codex / top /
watch streaming with no further user activity. The two systems split
the timeline cleanly, with no overlap and no shared state to
synchronise.

macOS and Linux are unaffected: this code is gated by
`#[cfg(target_os = "windows")]` and the macOS path uses the embedded
libghostty NSView with its own Metal compositor. Linux's preview
renderer paints rows directly through GPUI `StyledText` and has no
staging ring, so the freeze condition cannot arise there in the
current architecture.

## What we learned

- The original mailbox conflated two distinct concerns under one
  rule: "newest-wins" (correct) and "wait for the next prepaint to
  drain" (broken under continuous load). Decoupling presentation
  *rate* from VT update *rate* separates them cleanly — one rule,
  capped at the GPUI/vsync ceiling, covers both bursts and
  streaming.
- Mailbox semantics need a forward-progress invariant. "Skip
  draining an older slot when a fresher one exists" only terminates
  if the fresher slot eventually drains; under a busy producer it
  never does. The cap-paced fallback is what guarantees forward
  progress without sacrificing newest-wins for quiet-VT cases.
- The most expensive thing in this pipeline is the full-frame
  CPU↔GPU↔GPUI handoff, not the GPU draw. The cap exists because
  presenting faster than vsync would do that handoff for a frame
  the compositor will never paint.
- Bug reports framed as "needs a click to refresh" almost always
  point at a redraw scheduler gated on input events; on Windows
  here the gate was the `prefer_latest` `block_drain` path, not
  the `cx.notify()` plumbing.
