# Tab transition matte and teardown flash

## What happened

Opening, closing, or switching tabs could show a visible blink across the terminal area.

## Root cause

Two behaviors were interacting:

- tab creation and switching used the same full-terminal layout matte that had been added for earlier transition leaks
- closing a tab shut down the outgoing Ghostty surfaces before the replacement tab's surfaces were guaranteed visible

The matte caused a visible veil, while eager teardown could expose a whole-window transparent gap.

## Fix applied

- Removed the full-terminal matte from tab creation, tab switching, and normal tab close
- Kept incoming tab surfaces visible before hiding or tearing down outgoing surfaces
- Deferred closed-tab surface shutdown to the next frame after the replacement tab is shown

## What we learned

Tab transitions are surface swaps, not seam animations. They should preserve native surface continuity instead of covering the terminal with a GPUI matte.
