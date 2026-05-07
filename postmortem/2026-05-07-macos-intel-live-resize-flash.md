# macOS Intel Live Resize Flash

## What Happened

Issue #70 regressed after the macOS surface-geometry and display-scale work.
Intel Mac users again saw aggressive flashing while dragging a Con window
resize, especially with transparent terminal glass enabled.

The report arrived after the workspace refactor, but the timeline showed the
regression was already present before that refactor merged. The relevant change
window was the macOS embedded Ghostty surface path, not the workspace module
split.

## Root Cause

PR #150 correctly moved display scale, backing scale, and pixel-size sync into
a single Objective-C bridge. During that change, `GhosttyView::update_frame`
started committing native geometry in this order:

1. Move the AppKit host scroll view.
2. Move the document view and Ghostty surface view through an early
   `sync_native_scroll_view()` call.
3. Ask libghostty to adopt the new backing size.
4. Move the document view and Ghostty surface view again.

The unsafe step was the first document/surface move. For one live-resize frame,
macOS can present the embedded Metal surface at the new pane bounds while
Ghostty is still drawing the previous framebuffer size. Apple Silicon tended to
hide the mismatch; Intel compositors exposed it as a flash.

The pre-regression path also moved the outer host before resizing Ghostty, but
it did not present the document/surface view at the new pane geometry until
after the terminal resize transaction had run.

This is the same class of bug as the earlier terminal-glass seam work: two
systems commit independently, and a temporary mismatch becomes visible through
the transparent window. The correct fix is not a matte or a border. It is to
make the terminal-size transaction ordered.

The first follow-up fix in PR #152 got the ordering only half right. It moved
`sync_native_backing_properties()` before the AppKit frame commits, but that
function also forced `terminal.draw()`. Ghostty's Metal renderer reads the
hosted IOSurface `CALayer` bounds at draw time. During live resize, forcing a
draw before Con has moved the `NSScrollView`, document view, and surface view
means Ghostty can synchronously draw/present against the old layer bounds. On
Intel compositors that stale draw still shows up as a resize flash.

## Fix Applied

`GhosttyView::update_frame` now updates Ghostty's backing properties first and
then performs one deterministic AppKit scroll/document/surface geometry commit.
The backing sync path no longer forces a draw. The only synchronous draw for
the resize step happens after the native AppKit frames are committed, so
Ghostty's renderer reads the final layer bounds for that frame.

This restores the important ordering boundary from the pre-regression path:
terminal framebuffer and PTY size are ready before the native Ghostty surface
view is presented at the new bounds.

## What We Learned

Live window resize is a stricter path than ordinary pane layout. It can expose
intermediate AppKit states that normal layout coalesces away.

For embedded native terminal surfaces on macOS:

- sync Ghostty's framebuffer metadata before moving the hosted surface view
- draw Ghostty only after the hosted surface view has its final layer bounds
- commit the AppKit hierarchy once per layout pass when possible
- do not solve resize flashes with full-terminal mattes or opaque underlays
- keep this path macOS-only; Windows and Linux do not embed Ghostty through an
  AppKit child view

## Follow-Ups

- If issue #70 still reproduces on Intel hardware, collect
  `CON_GHOSTTY_PROFILE=1 RUST_LOG=con::perf=info,con=debug` logs around a live
  resize and compare `update_frame` duration against `native backing sync`
  timing.
- Add an Intel resize smoke item to release QA: transparent terminal, blur on,
  dark terminal theme, light desktop, resize a content-heavy terminal window.
