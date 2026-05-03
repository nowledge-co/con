# Terminal-layer screen restore

## What happened

The first screen-text restore implementation persisted bounded recent terminal
text, then painted it as a GPUI overlay above the embedded Ghostty view until
the first input event. It made restart continuity visible, but the text was not
part of the terminal renderer: it did not share terminal clipping, selection,
scrollback, cursor behavior, or z-order.

## Root cause

Con could read text from embedded Ghostty through `ghostty_surface_read_text`,
but Ghostty's published embedded C API did not expose a way to seed terminal
output before the child shell starts. The quick overlay violated the product
model: terminal history must belong to the terminal engine, not adjacent UI.

## Fix applied

Con now applies a narrow build-time extension to the vendored Ghostty checkout
used for macOS builds:

- add an embedding-only `initial_output` surface option
- parse that output with `Termio.processOutput()`
- do it after the renderer thread is alive but before the IO thread starts the
  shell process
- keep `3pp/` and any caller-provided `CON_GHOSTTY_SOURCE_DIR` checkout
  unmodified by patching only the build output copy

Con converts the private plain-text snapshot into sanitized terminal output and
passes it during surface creation. The restored text now lives in Ghostty's
screen/scrollback layer and is never written to the shell pty.

## What we learned

Continuity features must preserve ownership boundaries. If terminal content is
shown, it must be terminal state. UI overlays are valid for explanations and
warnings, not for pretending to be scrollback.

## Follow-ups

- Upstream a proper embedded Ghostty restore/output-seeding API so Con does not
  need a build-time extension.
- Extend the preview Windows/Linux backends with equivalent terminal-layer
  restore once their readback and transcript paths are complete.
- Add style-aware snapshots only after the plain-text path is stable and cheap.
