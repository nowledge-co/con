# Study: Ghostty Windows port (PR ghostty-org/ghostty#12167)

**Status**: unmerged upstream. This doc captures what's there, why we
aren't pulling it in directly, and what we may want to borrow.

## The PR

- Upstream: <https://github.com/ghostty-org/ghostty/pull/12167>
- Authors: contributors led by the Ghostty community (per the PR
  thread and follow-up work). Credit individual authors explicitly
  if any of their code lands in this repo — they did the hard
  archeology on Win32 terminal hosting that we're benefiting from by
  reading, even if we don't copy code verbatim.
- Scope: implements a Windows application runtime (`apprt=win32`)
  for Ghostty — a full native `.exe` with a DWM-composed window,
  D3D11 renderer, DirectWrite font stack, ConPTY, and the surface
  lifecycle upstream's embedded C API expects. Thousands of lines.

## Why we aren't pulling it in

- **Different embedding model.** PR #12167 produces a *standalone*
  Ghostty.exe. con needs `libghostty-vt` usable as a library inside
  a GPUI host window — a different decomposition. The PR's apprt
  owns the window and renderer; we own the window and want to feed
  libghostty-vt the bytes and render via our own D3D11 path inside a
  child HWND.
- **Closed unmerged.** The PR thread shows maintainers wanting more
  convergence work before accepting; the author continued pushing
  updates after the close. Pulling in "whatever state it's in"
  would couple us to a moving fork of Ghostty.
- **License / attribution.** Ghostty is MIT; we can reuse code with
  attribution. If/when we do, each file we borrow or adapt gets a
  header comment naming the original author and linking the PR or
  commit.

## What's worth borrowing (patterns, not necessarily code)

These are the places where PR #12167 likely solved things we'll hit
too. Look at its code for reference before inventing our own
solution:

1. **ConPTY lifecycle details.** Ours works (spawns, reads, writes),
   but there are edge cases around clean shutdown on parent exit,
   `ClosePseudoConsole` ordering, and the background output reader
   thread termination that are easy to get wrong. Cross-check ours
   against the PR before shipping beta.
2. **D3D11 renderer structure.** The PR implements the full Ghostty
   renderer (not just a VT cell grid). Cell-grid rendering we've
   done ourselves from the AtlasEngine pattern; for future work like
   background images, sixel, and cursor animation, the PR is the
   reference for the Ghostty-native look.
3. **DirectWrite font fallback & metrics.** We use a 0.6×fs heuristic
   for cell width today; real DWrite `IDWriteFontFace::GetMetrics`
   is in the PR. Adopt that pattern when Phase 3b-2 wires up real
   font metrics.
4. **IME handling.** We have `WM_CHAR` forwarding; real IME (Chinese,
   Japanese, Korean input composition) needs `WM_IME_*` messages,
   candidate window placement, and cursor-rect reporting back to
   Windows. The PR has this working.
5. **Clipboard.** OSC 52 + Win32 clipboard interop is a classic
   get-the-Unicode-surrogate-pairs-right trap; see how the PR does it.
6. **DWM / DComposition integration.** If we ever pursue the
   "DComp sibling visual inside GPUI" path (`docs/study/gpui-external-swapchain-upstream-pr.md`),
   the PR is the nearest prior art for how Ghostty's visuals should
   behave in a DWM-composed window.

## When to look at it

We don't need to reference PR #12167 for bring-up. Everything in
Phase 3a/3b is self-contained against the upstream libghostty-vt C
API. Revisit when:

- Polishing input (IME, dead keys, international keyboards) — Phase 4.
- Matching Ghostty's exact pixel-level look (font rendering, cursor
  shapes, underline styles) — Phase 4.
- Doing the DComp-sibling-visual upstream work in GPUI — Phase 3d.
- Investigating any Win32 edge case we don't understand — always a
  good starting point.

## Notes

- If a function, constant, or algorithm in our codebase comes from
  reading the PR (not just inspired by it), add a `// Derived from
  PR ghostty#12167 by <author>, commit <sha> — MIT license` comment.
- Keep this doc updated with specific patterns we adopt so the
  attribution trail is clear.
