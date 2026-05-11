# UI Font Size Scope

## What happened

A user raised issue #190 after setting Appearance -> UI Size to a large value:
the terminal text scaled, but terminal-adjacent UI such as the bottom input bar
and the agent panel run trace still looked small.

## Root cause

The Appearance setting correctly updated `Theme::font_size` and
`Theme::mono_font_size`, but several high-density UI components bypassed those
theme metrics with literal `px(...)` text sizes. The affected pieces were mostly
terminal chrome and agent trace/status cards, so the app looked inconsistent at
larger UI sizes even though the setting itself was saved and applied.

## Fix applied

Added a small `ui_scale` helper that derives UI and mono scale factors from the
active GPUI theme. The helper preserves the shipped default sizes exactly, then
scales only the relevant terminal-adjacent typography, spacing, and control
heights when the user changes UI Size.

After testing at UI Size 24, the scaling was refined into two tracks: text uses
the full font scale, while dense chrome uses a slower density scale. This keeps
large text readable without turning titlebar buttons, pills, icons, and trace
cards into oversized blocks.

Review feedback also caught that text scaling must honor the full configured
range, including the minimum UI size. Font scale clamps are now derived from
the same UI Size bounds the settings panel exposes, while density scale remains
separately capped for layout stability.

The fix covers:

- Bottom input bar text, mode button, pane selector, inline suggestion overlay,
  and send button.
- Agent panel prose markdown base sizing, headings, code block line height, and
  message fallback text.
- Agent run status, run trace cards, tool rows, compact chips, and inline agent
  input when the bottom input bar is hidden.

## What we learned

Theme-level font settings are not enough if component code pins dense UI with
literal pixel sizes. New terminal-adjacent UI should either inherit the theme
font size or use a theme-derived scale helper, with the default scale resolving
to `1.0` to avoid accidental visual drift.

Readable text and spatial density should not share the exact same scale curve.
Accessibility sizing needs larger glyphs; polished app chrome needs controlled
growth so the layout remains stable at large UI sizes.
