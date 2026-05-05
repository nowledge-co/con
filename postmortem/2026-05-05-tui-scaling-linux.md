# TUI text wrapping incorrectly in Linux split panes

**Date**: 2026-05-05
**Issue**: #142 triage, Linux preview side fix

## What happened

While triaging #142, we found a separate Linux preview bug: TUI
applications (htop, vim, less, etc.) displayed garbled layouts in split
panes. Continuous text runs like `12345` were broken across lines, for
example `abcde, 123` on one line and `45` on the next, instead of being
clipped at the pane edge. The same TUI in Ghostty's own split rendered
correctly.

## Root cause

`render_cached_terminal_row()` in `linux_view.rs` builds each terminal
row as a GPUI `div` containing a `StyledText`. GPUI's default text
style is `WhiteSpace::Normal`, which enables word-wrap: when a
continuous run of non-whitespace characters (a "word" like `12345`)
doesn't fit in the remaining width, GPUI moves the whole run to the
next line.

A terminal row is not prose — it is a fixed-width cell grid. There is
no concept of word boundaries; each column is independent. Word-wrap
must be disabled so that content which overflows the pane width is
simply clipped by `overflow_hidden()`, exactly as a real terminal
emulator would behave.

## Fix applied

Added `.whitespace_nowrap()` to the row `div` in
`render_cached_terminal_row()`:

```rust
div()
    .w_full()
    .h(line_height)
    .min_h(line_height)
    .overflow_hidden()
    .whitespace_nowrap()   // ← added
    ...
```

`WhiteSpace::Nowrap` tells GPUI's text layout to never break the run
onto a new line. The existing `overflow_hidden()` clips anything that
extends past the pane edge.

## What we learned

Any GPUI element that renders terminal content (fixed-width cell grid)
must set `.whitespace_nowrap()`. The default `WhiteSpace::Normal` is
correct for prose UI but wrong for terminal rows — it silently
reflows content that should be clipped, producing layout corruption
in TUI apps that fill the full reported column width.

When the real glyph-atlas grid renderer lands, it will paint cells
directly and won't go through GPUI text layout, so this won't apply.
Until then, every `StyledText`-based terminal row needs `nowrap`.
