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

After the stretch was fixed, deleting the right pane exposed a second artifact:
the old frame stayed correctly sized, but the newly revealed area to its right
had no terminal-colored filler until the resized D3D frame arrived. That showed
the transparent window backing for a few frames.

The cwd fallback also only covered the launch directory. It could not learn
`cd` changes in default Windows PowerShell/pwsh sessions because those shells
do not emit OSC 7 cwd updates by default.

One more layer was missed in the first fix: even after Con injected OSC 7 into
PowerShell/pwsh, `libghostty-vt`'s terminal-only stream handler ignores
`report_pwd`. That is correct for upstream's low-level VT helper, but it means
the Rust wrapper must capture OSC 7 itself if portable backends want live cwd
state.

## Fix Applied

The Windows terminal view now keeps stale cached readback images at their
original logical size while the pane is resizing. The parent terminal surface
clips overflow and paints its normal background until the renderer publishes the
next frame, avoiding the visibly stretched text. When the pane grows before the
next frame arrives, only the uncovered stale-frame gutters are filled with the
terminal background so glass opacity is preserved without double-compositing
behind the old image.

The Windows `RenderSession` now mirrors the Linux fallback model: it stores the
resolved shell cwd and returns it from `current_dir()` until the VT layer reports
a newer directory. PowerShell/pwsh launches also get a lightweight prompt hook
that emits OSC 7 on every prompt, so cwd snapshots follow real `cd` changes
instead of freezing at the launch directory. The shared Rust `VtScreen` wrapper
now parses OSC 7 and sets `GHOSTTY_TERMINAL_OPT_PWD` manually, which makes the
cwd path work on both Windows and Linux.

The pane divider and chrome transition path now uses terminal-colored seam
covers across portable backends too. Divider hit areas are absolute overlays
around a 1px visible divider instead of 5px transparent layout strips, so drag
targets stay usable without exposing the window backdrop between panes.

## What We Learned

Cached terminal frames must be treated as size-stamped render artifacts, not
stretchable UI imagery. When a terminal surface changes size, keeping old pixels
stable and letting the terminal background cover new space is less visually
wrong than resampling text.

For session continuity, every platform backend needs a reliable cwd fallback
that survives the gap between process launch and shell-integration events. On
Windows that fallback is not enough by itself: shells that do not emit OSC 7
need integration at process launch, otherwise the app has no grounded way to
know the user's current directory after `cd`.
