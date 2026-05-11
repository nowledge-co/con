# Windows Pane Resize Stale Frame

## What Happened

On Windows, closing or splitting panes could briefly show the surviving
terminal content stretched to the new pane size before the terminal renderer
published the resized frame. The artifact looked like old text being zoomed or
reshaped for a few frames during pane layout changes.

## Root Cause

The Windows backend paints terminal content through a cached D3D11 readback
image inside a GPUI image element. During a pane layout change, GPUI resized the
image element immediately, but the D3D renderer had not yet produced a frame at
the new physical size. Because the cached image used `ObjectFit::Fill` with
`size_full`, the old readback texture was stretched to the new pane bounds.

The same pass also found that Windows restored sessions did not retain the
resolved ConPTY launch directory as a current-directory fallback. If shell
integration had not yet reported a newer path, session snapshots could lose the
restored cwd even though scrollback text was present.

## Fix Applied

The Windows terminal view now keeps stale cached readback images at their
original logical size while the pane is resizing. The parent terminal surface
clips overflow and paints its normal background until the renderer publishes the
next frame, avoiding the visibly stretched text.

The Windows `RenderSession` now mirrors the Linux fallback model: it stores the
resolved shell cwd and returns it from `current_dir()` until the VT layer reports
a newer directory.

## What We Learned

Cached terminal frames must be treated as size-stamped render artifacts, not
stretchable UI imagery. When a terminal surface changes size, keeping old pixels
stable and letting the terminal background cover new space is less visually
wrong than resampling text.

For session continuity, every platform backend needs a reliable cwd fallback
that survives the gap between process launch and shell-integration events.
