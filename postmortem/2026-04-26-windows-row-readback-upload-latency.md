# Windows row-readback upload latency

## What Happened

After the Windows renderer started copying only dirty VT rows from the D3D
render target, profiling showed the GPU readback path was no longer the
only visible cost. Single-line input and prompt updates still measured
roughly 5-9 ms in `win_renderer` / `win_sync_render`, and Neovim still
felt behind native Windows Terminal.

The profile was useful because it separated the problem:

- `vt_snapshot` was usually sub-millisecond.
- `draw_ms` was usually around 0.1 ms for one-row updates.
- `readback_rows` dropped from full-frame height to one cell row.
- `block_drain_ms` dropped, but total frame time still carried several
  milliseconds not explained by drawing or row readback.

## Root Cause

The first dirty-row fix reduced the D3D `CopyResource` cost, but the app
still reconstructed a full 2400x1544 BGRA image for every rendered frame
before handing it to GPUI. GPUI's `RenderImage` API is immutable and keyed
by a unique `ImageId`, so each new full-frame image also implied a
full-size sprite-atlas upload.

In other words, a one-row terminal change became:

1. one-row D3D readback,
2. full-surface CPU frame clone,
3. full-surface GPUI image upload.

That explained why the dirty-row readback profile improved but typing and
small prompt updates still did not feel native.

## Fix Applied

The Windows renderer now returns BGRA patches instead of always returning
a full reconstructed frame:

- full redraws return one full-height patch;
- row-local redraws return one or more row-strip patches;
- resize, selection, and other unsafe cases still fall back to full
  readback.

`windows_view` originally kept a full base `RenderImage` and layered
dirty-row strip images over it. That reduced uploads, but it was
semantically wrong for Con's translucent terminal background: GPUI
alpha-composited row strips over the old base, so a strip could not erase
stale glyph pixels underneath it. The visible failure was command output
or log rows that looked missing or faint until mouse selection forced a
broader repaint.

The corrected handoff keeps a CPU-side BGRA backing frame. Full redraws
replace the backing frame; dirty-row readbacks replace byte ranges inside
that backing frame; then `windows_view` publishes one `RenderImage`.
This keeps the D3D partial-readback win while preserving true replacement
semantics for transparent pixels.

## What We Learned

- Optimizing only the GPU readback was incomplete because GPUI's image
  contract made the following CPU and upload work just as important.
- Profiling needs to include both sides of the bridge: renderer stages
  and app image handoff. The useful signal was the gap between tiny
  `draw_ms` / reduced `readback_rows` and the remaining total frame time.
- Dirty rectangles must preserve replacement semantics. With translucent
  images, "draw a patch on top" is not the same operation as "replace the
  dirty rectangle."
- The CPU backing-frame approach is still a tactical bridge, not the end
  state. The clean long-term fix remains direct composition of the
  terminal swap chain into GPUI's DirectComposition tree, which removes
  GPU→CPU→GPU entirely.
