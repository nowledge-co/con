# macOS Terminal Covered By Pane Background

## What happened

While polishing pane divider seams, the workspace began painting an opaque
terminal-colored GPUI background on the full terminal content area on macOS.
The app chrome still rendered, but Ghostty's terminal content disappeared,
leaving only a blank terminal-colored surface.

## Root cause

On macOS, Ghostty renders through a native NSView hosted outside GPUI's normal
element tree. A full-size GPUI background on the pane host can cover that
native view. The seam fix treated macOS like the portable renderers and painted
the whole pane host instead of limiting opaque paint to narrow seam covers.

## Fix applied

Restored transparent macOS pane and terminal-area hosts. The GPUI terminal
backdrop remains only on non-macOS renderers, where the terminal is painted in
the GPUI tree and needs that fallback surface.

## What we learned

Do not paint full-size opaque GPUI surfaces over Ghostty's macOS native NSView.
For macOS resize or transition seams, use targeted narrow covers only; keep the
terminal host transparent so the native surface can remain visible.
