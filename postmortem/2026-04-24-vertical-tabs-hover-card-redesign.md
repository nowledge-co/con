# Vertical-tabs hover-peek replaced with a floating tab card

Date: 2026-04-24
Issue: #66 (vertical tabs production-flow polish)
Branch: `vertical-tab`

## What happened

The first iteration of vertical tabs auto-expanded a 240 px panel on
rail hover (Microsoft Edge–style). After actually using it for a
session-and-a-half, two problems surfaced:

1. **Drag from collapsed broke.** User clicks a rail icon to drag,
   cursor leaves the icon to start the drag, the peek panel folds,
   the drop targets vanish. To drop anywhere meaningful you'd have
   to keep the cursor inside the rail's 44-px-wide column for the
   entire drag operation, which is unforgiving and unintuitive.

2. **Aggressive layout commandeering.** Passive intent — just
   trying to remember "what's tab 3?" — was making the panel grow
   over the terminal pane. The user has to either commit (move into
   the panel) or move away, with no neutral middle ground. Compared
   to Apple's Finder / Mail / Safari sidebars, where hover gives you
   a tooltip-style card next to the item without altering layout,
   the auto-expand reads as edgy and Microsoft-y.

3. **Two selection signals.** The active row had a 3-px primary-color
   accent bar on the leading edge AND an elevated white pill bg AND
   medium font weight AND foreground (vs muted) text color. Any one
   of those is unambiguous; piling them on top of each other is
   decorative chrome.

4. **Always-on action chrome on the active row.** The rename pencil
   and close X were always visible on the active row "so the user
   has them at hand". In practice the active row felt cluttered
   compared to the quiet inactive rows, and the eye didn't know
   what was important. Better: make actions hover-only on every
   row, active included.

## Fix

- **Remove the auto-expand peek**. Replace with a `render_hover_card_overlay`
  that returns a small floating card (~240 px) anchored vertically
  to the cursor's current y-coordinate. The card never affects
  layout. Hovering the rail's bottom-most icon produces a card
  beside that icon; mouse-leave from the rail dismisses it.
- **Drop the accent bar**. Active row now uses just the elevated
  pill background + medium font weight + foreground text color. One
  signal, three layers of subtlety, zero decoration.
- **Make all action affordances hover-only**, on every row including
  active. Quiet default, reveal on intent.
- **Drag works in collapsed mode now.** Each rail pill carries the
  same `on_drag` / `on_drag_move` / `on_drop` triple as the pinned
  rows, with the same bounds-filter trick from the earlier drag
  postmortem. Drop target during a drag uses the row's hover-bg
  (no separate indicator line — the rail is too narrow for one).
- **Hover card hides during drag.** Otherwise the floating card
  fights the drag chip for the user's attention.

## What we learned

- **Hover-expand-the-whole-panel is the wrong primitive.** Microsoft
  Edge does it; Apple doesn't. The Apple pattern (cursor-anchored
  tooltip card) communicates the same information without taking
  over the workspace. Less surface area = less stuff to break.
- **Cursor-anchored beats element-anchored** for hover cards in
  GPUI. Element-anchored requires you to compute layout offsets
  from the parent; cursor-anchored just reads `window.mouse_position()`
  at render time. The card visually "follows the finger" exactly
  like Finder labels.
- **Pick one selection signal.** When in doubt about which cues to
  use for active state, pick one and double-check by mocking the
  page in greyscale. If the active state is still obvious, you've
  picked enough; if not, lift the contrast on the cue you have
  rather than adding new cues.
- **Quiet rows surface louder rows.** Always-on chrome on the
  active row crowds it. Hover-only chrome on every row makes the
  active row's *content* the focal point, with the chrome
  appearing only when the user reaches for it.
