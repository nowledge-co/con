# Vertical-tabs hover-peek panel rendered behind the translucent terminal pane

Date: 2026-04-24
Issue: #66 (feat: Vertical Tabs)
Branch: `vertical-tab`

## What happened

While implementing Chrome-style vertical tabs (issue #66) the hover-peek
overlay — the panel that floats out from the icon rail when the cursor
enters it — appeared washed-out on Linux:

- The collapsed rail rendered at the expected `surface_tone(0.10)`
  shade (sampled `#E1E0DD` against the paper-light terminal pane).
- But the peek panel painted into the same region at `surface_tone(0.18)`
  read as `#F9F8F3` — barely distinguishable from the terminal pane's
  `#FAF9F4`.
- A diagnostic that swapped the panel bg to vivid red (`gpui::rgb(0xFF0000)`)
  rendered as **pale pink** in the same region.

That ruled out colorspace bugs — the panel WAS being painted, just with
the wrong opacity stack.

## Root cause

The original `SessionSidebar::render` returned a single `div().relative()`
with two children: the rail (in flow) and the hover-peek overlay
(`.absolute().left(RAIL_WIDTH).w(PANEL_WIDTH)`). That entire tree was
inserted into `main_area` as the FIRST child, with the terminal pane
appended as the SECOND child.

In con's flex-row layout the rail's parent only occupies `RAIL_WIDTH`
(44 px) of horizontal space. The terminal pane sibling fills the rest
of `main_area`. Even though the absolute peek overlay positioned itself
to extend BEYOND its parent's bounding box (into the terminal pane's
x-range), GPUI paints **siblings in source order**: rail-with-overlay
first, terminal pane second.

So the peek overlay rendered first at full alpha, and the terminal pane
then painted ON TOP of it with `theme.background.opacity(terminal_opacity)`
(default `0.69`). The overlay was visible through the translucent
terminal pane background, but only at ~30% effective alpha — exactly
what the screenshot showed.

This was easy to miss in the design pass because the rail itself reads
correctly: it occupies its parent's full bounding box (44 px), so the
terminal pane sibling never stacks on top of it.

## Fix

Move the overlay out of the sidebar's render subtree and into the
workspace's render subtree as an absolute sibling of the terminal
pane:

```text
crates/con-app/src/sidebar.rs
  - Render returns rail (collapsed) | panel-body (pinned). No
    absolute children.
  - New `pub fn render_hover_card_overlay(&mut self, window, cx) -> Option<AnyElement>`
    returns the overlay element when collapsed-and-hovered.

crates/con-app/src/workspace.rs
  - main_area is now `.relative()` so absolute children position
    against it.
  - Compute the peek overlay BEFORE `let theme = cx.theme();` so the
    sidebar's mutable cx update doesn't conflict with the immutable
    theme borrow used downstream.
  - Append the overlay as the LAST child of main_area, AFTER the
    terminal pane and AFTER the agent panel — so it stacks above
    both.
```

The overlay now paints over the terminal pane (and the agent panel,
when both are active simultaneously), at full alpha, against the
elevated `surface_tone(0.18)` body bg.

## What we learned

- **Absolute children of a translucent-background sibling render
  behind, not in front of, the sibling — even when their `.left/right`
  positioning visually places them over the sibling.** GPUI doesn't
  promote absolute children to a separate paint layer; they share their
  parent's z-index. To get a true overlay above a sibling, the absolute
  element has to be a child of the **shared ancestor**, appended AFTER
  the sibling.
- **`theme.background` (gpui-component) is not the same color as the
  terminal pane's actual rendered background**, because the terminal
  pane uses the *terminal* theme's `background` (Flexoki "paper")
  instead of the gpui-component theme's bg, and applies
  `terminal_opacity`. Any helper that picks a "step away from
  background" surface (we wrote `surface_tone()`) needs to pick a
  delta large enough that it reads against both surfaces, not just the
  one the gpui-component palette computes against.
- A red-square diagnostic in the suspect region is the fastest way to
  distinguish "not painted at all" from "painted but stacked wrong".
  The first attempt at this bug spent 20 minutes debugging the blend
  formula before the diagnostic immediately revealed the actual stack
  ordering.

## Follow-ups not in scope

- Drag-to-reorder for the horizontal tab strip. Vertical strip
  reordering shipped in this PR.
- Tab groups / pinned tabs (Chrome-style top-of-list separator).
- Hover-peek on the agent panel side (right edge), if we ever ship a
  vertical agent-panel mode.
- Migration of the existing horizontal tab strip to share the
  `surface_tone()` helper for visual coherence on themes whose
  `theme.title_bar` collapses into background.
