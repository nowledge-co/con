# Windows Atlas — PUA Rasterization, Atlas Bleed, and Cursor Z-Order (2026-04-21)

## What happened

Through late March and into April 2026 the Windows terminal renderer
kept oscillating between two symptom classes that looked like font
regressions but were actually renderer ones:

- **"Hyphens look detached"** — consecutive `-`/`_`/`=` rendered with
  visible gaps, and on some days thin glyphs picked up speckle from
  their atlas neighbours.
- **"Nerd-Font icons render half or not at all"** — PUA glyphs
  (`github`, `folder`, `check`, …) either got pre-clipped to a single
  cell, overflowed into the next column and were then overdrawn by
  that column's background, or vanished entirely.

Every fix for one class tended to worsen the other. The commit trail
through `crates/con-ghostty/src/windows/render/atlas.rs` shows the
oscillation clearly:

```
1dece62 render Nerd-Font icons at natural ink width
46110e5 scale Nerd-Font icons to fit their cell
d35e77b shift negative leftSideBearing glyphs into their slot
4d5f727 translate DrawText pen for negative-lsb PUA icons
c3c618f clip atlas slot + shift layoutRect for negative-lsb PUA
9935ba8 apply lsb shift to all negative-lsb PUA glyphs
37a6de3 scale oversized PUA glyphs to fit single cell
ae893a6 fix atlas bleed (black pre-fill + PushAxisAlignedClip)
9a5815d (follow-up)
a52313f three-case rasterization + cursor-before-sort
```

The last commit is the one that stops the oscillation. This writeup
exists so the next engineer who sees "the hyphens look weird" or "the
icons are half" doesn't start chasing font metrics again.

## Root causes

There were four distinct bugs stacked on top of each other. Each one
masked the next.

### 1. Treating hyphen spacing as a font-design concern

IoskeleyMono's hyphen has an ink width of ~430 design units against a
600du advance (see
`~/.claude/projects/-Users-weyl-dev-con/memory/project_ioskeleymono_glyph_metrics.md`
for the measured numbers). At 1:1 render, that leaves roughly 170du of
negative space between consecutive hyphens. Native Ghostty on macOS
compensates in its renderer; Windows Terminal compensates in its
renderer; Con's early Windows atlas did not.

Earlier sessions had miscategorized this as "the font is just designed
that way" and moved on. It isn't. Hyphen continuity is a **renderer
responsibility**, and every credible monospace renderer delivers it.

### 2. Negative-lsb PUA icons versus the cell slot

IoskeleyMono's Nerd-Font icons are authored as ~1000×1000du squares
sitting inside a 600du monospace advance — leftSideBearing and
rightSideBearing both go negative so the ink extends past the advance
on both sides. Powerline glyphs (U+E0A0..U+E0D4) are the exception:
they're authored flush to the advance because any per-glyph trimming
would leave seams between consecutive separators.

Rasterizing these at their natural DirectWrite placement into a
cell-sized D2D layout rect caused three distinct failure modes across
the commit history:

- **Pre-clip**: a cell-sized layoutRect with `DRAW_TEXT_OPTIONS_CLIP`
  trims the ink inside DirectWrite before we ever see the pixels.
- **Out-of-slot pen**: shifting the pen by `|lsb|` without widening
  the atlas slot put the glyph's right half into the next slot,
  which is then overwritten by that slot's own glyph — "half icons".
- **Overdraw in the grid pass**: even with a correctly widened slot,
  the neighbouring cell's background-only instance rendered on top of
  our icon unless we sorted wide instances after narrow ones.

### 3. ClearType AA fringe bleeding between atlas slots

DirectWrite grayscale AA extends 1–2 pixels outside the reported ink
box. The skyline packer (`etagere`) places glyphs tightly, so on a
busy atlas a thin glyph like `-` ended up reading fringe pixels from
the glyph packed immediately above or beside it — hence the occasional
speckle.

### 4. Cursor row-major index invalidated by instance sort

Once we sorted `instances` so wide PUA quads rendered last, the row-
major `idx = row * cols + col` lookup the cursor code used to copy
the underlying cell's atlas coordinates became wrong: after sorting,
the cell at `(row, col)` wasn't at `instances[idx]` anymore, so the
cursor block copied a random nearby cell's glyph/colours and painted
itself one or two columns to the right.

## Fix applied

`a52313f` ("Windows atlas: three-case PUA rasterization + cursor-
before-sort") consolidates everything into one coherent architecture.
`atlas.rs` now picks one of three paths for each glyph based on
measured DirectWrite metrics (`leftSideBearing`, `ink_w_px`,
`ink_h_px`):

1. **Fits cell** (`natural_w ≤ cell_w+1 && natural_h ≤ cell_h+1`) —
   single-cell atlas slot, `DRAW_TEXT_OPTIONS_CLIP` on. Covers
   ASCII, hyphens, box-drawing, and Powerline.
2. **Width-only overflow** — widen the slot up to 2× `cell_w`, shift
   the layoutRect right by `|lsb_overhang|` so the glyph's ink left
   edge lands flush at `slot.left`, and use
   `DRAW_TEXT_OPTIONS_NONE` so DirectWrite doesn't re-clip against
   the shifted rect. Covers IoskeleyMono's full NF set at the
   default font size (natural_h ≈ cell_h, natural_w > cell_w).
3. **Height overflow** — scale-around-cell-centre with a `Matrix3x2`
   transform and draw into an oversized layoutRect so DirectWrite
   doesn't pre-clip the natural ink before the transform shrinks it
   back into the slot. Only reachable on pathological short-cell
   configs.

Two concerns span all three paths:

- **Atlas bleed** — every `DrawText` call is bracketed by
  `PushAxisAlignedClip(slot_rect, ALIASED)` plus a `FillRectangle`
  of `slot_rect` with an opaque black brush *before* DirectWrite
  runs. The black fill establishes a known baseline so ClearType
  fringe from whichever glyph was previously packed next door can't
  contaminate the slot; `PushAxisAlignedClip` prevents our own ink
  from bleeding outward. Atlas packing stays tight — no artificial
  gutters, no wasted VRAM.
- **Wide-glyph z-order** — `Renderer::draw_cells` stable-sorts
  instances with a single key: `atlas_size[0] > cell_w_px`. Narrow
  instances come first, wide instances come last, and DX11's in-order
  per-pixel writes within a single `DrawIndexedInstanced` call mean
  the wide glyph wins when its quad overlaps a neighbour's bg-only
  instance.

The cursor fix is the smallest change and the most important for
future modifications: **capture the cursor's source cell from the
pre-sort grid, push the cursor's inverse-colour instance AFTER the
sort, so it's always last in the draw.** Two invariants in one line;
breaking either one moves the cursor or overdraws it.

The matching renderer work in `crates/con-ghostty/src/windows/render/mod.rs`:

```rust
// Capture BEFORE sort — idx = row*cols+col is only valid in grid order.
let cursor_source = if snapshot.cursor.visible {
    instances.get(row * cols_u + col).copied()
} else { None };

instances.sort_by_key(|inst| (inst.atlas_size[0] > cell_w_px) as u8);

// Push AFTER sort — cursor must render last.
if let Some(src) = cursor_source { instances.push(cursor_instance_from(src)); }
```

Runtime validation: IoskeleyMono at the default font size on Windows
11, cells 17×35 px. Ran against an oh-my-posh prompt stack (git
branch chip, folder icon, status icons) and a dense hyphen box-rule
(`------...` spanning the terminal width). Both render clean; cursor
lands on the correct column in every pane after split-pane resize
and rapid typing.

## What we learned

1. **Renderer bugs love to cosplay as font bugs.** Every time hyphen
   rendering looked wrong the instinct was "look at the TTF". The
   actual answer every time was in `atlas.rs`. Ink-box metrics are
   useful as evidence, not as an excuse to close a rendering ticket.
2. **Three cases, not a continuum.** The earlier commits tried to
   parameterize a single "scale + shift + widen" pipeline that did
   the right thing for every glyph. Every tuning dial also created a
   way to tune the wrong answer. A hard split by the smallest set
   of measured predicates (`fits_cell_w`, `fits_cell_h`) turned out
   to be simpler to reason about and simpler to extend.
3. **Sorts must preserve the invariants their callers depend on.**
   Adding an `instances.sort_by_key(...)` silently invalidated a
   `row*cols+col` index used dozens of lines above. Code that
   reorders in place should either run after all consumers of the
   original order, or capture the needed snapshots up front. The
   fix is three lines and completely prevents the regression class.
4. **Commit history is load-bearing documentation.** The oscillation
   between `1dece62` ↔ `46110e5` ↔ `d35e77b` ↔ `9935ba8` ↔ `37a6de3`
   would have been meaningless without the one-line messages showing
   the pendulum. Future renderer work on this file should keep
   commit messages honest: what changed, what symptom, what trade-off.

## Follow-ups

- **Atlas-bleed regression test** — render a test pattern, read
  back the atlas, assert no non-black pixels land outside the
  reported ink box. The check is cheap and prevents the whole class
  from recurring.
- **Width-overflow telemetry** — log once per session if any glyph
  falls into the height-overflow branch at the default font size;
  that signals a font or metrics change worth investigating.
- **Powerline exemption audit** — the `is_scalable_pua` helper
  hard-codes the Powerline range as a negative. When NF range
  definitions shift (NF does rev them), add the new Powerline-like
  ranges here to keep separator seams invisible.
