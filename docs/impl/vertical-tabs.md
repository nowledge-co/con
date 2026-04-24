# Vertical Tabs

Issue: [#66](https://github.com/nowledge-co/con-terminal/issues/66) — `feat: Vertical Tabs`.

A Chrome-style vertical tab strip for the workspace, gated behind a
single config flag so horizontal tabs remain the default for every
shipped beta.

## Feature surface

| Capability | Trigger | Notes |
|---|---|---|
| Toggle orientation | *Settings → Appearance → Tabs → Vertical Tabs* | Live; no restart. |
| Collapsed icon rail | default in vertical mode | Smart icon per tab. |
| Floating hover card | cursor over rail icon | Anchored to cursor; shows name + subtitle + pane count + SSH/unread badges. **Does not displace** rail or terminal pane. |
| Pinned panel | click sidebar-toggle in rail or header | Width tweens 220 ms; persists across restart. |
| Activate tab | left-click row | |
| Close tab | middle-click row, or hover-X (pinned mode) | |
| New tab | `+` in rail or panel header | |
| Smart name | auto from OSC title / SSH host / cwd | Bare shells fall through to cwd basename. |
| Smart icon | auto from process kind | terminal / globe / code / pulse / book-open / file-code |
| Rename tab | double-click label, or context menu | Enter commits, Esc cancels. |
| Reset name | context menu | Clears `user_label` so smart auto-naming resumes. |
| Reorder tab | drag (rail or pinned), or context menu Move Up / Down | Drop indicator: pinned-mode uses a 2-px line above the target row; rail mode uses a hover-bg pill. |
| Duplicate tab | context menu | New tab in same cwd as source. |
| Close other tabs | context menu | |

## Why a hover card instead of an auto-expanding overlay

The first vertical-tabs landing auto-expanded a full panel on hover (Microsoft Edge style). It read as aggressive — *passive intent* (just trying to remember what tab 3 is) was taking over the workspace. It also broke drag-from-collapsed: the user clicks an icon to drag, the cursor leaves the icon to start the drag, the overlay folds, the drop zones disappear.

Apple's pattern (Finder sidebar item names, Safari tab thumbnails, Mail mailbox tooltips) is a **tooltip-style card** that appears next to the icon without taking over layout. We do that. The card is anchored vertically to the cursor (its row IS the icon under the cursor by construction), 240 px wide, opaque, with a 3-px primary-color stripe on its leading edge to anchor it visually to the rail.

## Visual rules

- **Single selection signal.** Active row uses the elevated pill bg + foreground text + medium font weight. **No accent bar.** A single unambiguous cue is enough; doubling it (pill + bar + bold + accent color) is decorative chrome.
- **Quiet by default.** Action affordances (rename pencil, close X) are hover-only on every row, including the active one. Reveal on intent.
- **Surface separation via opacity.** `surface_tone()` blends a desaturated extreme-luminance overlay into `theme.background` at 0.10 (rail), 0.18 (panel body), 0.22 (hover card), 0.32 (card edge stripe). No borders, no shadows.
- **Mono font for technical detail only.** Tab names use `theme.font_family` (system) — tabs are named *things*. Subtitles (`~/proj/path`, `user@host`) use `theme.mono_font_family`.

## User-visible surface

- **Setting:** *Settings → Appearance → Tabs → Vertical Tabs*. Backed
  by `appearance.tabs_orientation` in `~/.config/con/config.toml`
  (`"horizontal"` | `"vertical"`).
- **Switching is live:** flipping the toggle re-renders immediately;
  no restart needed. The horizontal tab strip motion target collapses
  to 0 in vertical mode so the top bar shrinks back to the compact
  one-tab form.
- **Three states** in vertical mode:
  - **Collapsed (default).** Narrow icon rail (~44 px). Per-tab
    terminal-prompt icon stacked vertically, active pill highlighted
    with `theme.background`, needs-attention dot in the corner. `+`
    button at the top, sidebar-toggle icon at the bottom.
  - **Hover-peek.** When the cursor enters the rail in collapsed
    mode, a panel (~220 px) floats out as an absolute overlay ABOVE
    the terminal pane. Mouse leave returns to the rail. **The peek
    overlay does not displace the terminal area** — terminal columns
    don't reflow when the user is just glancing at tab titles. This
    matches Chrome's behavior.
  - **Pinned.** Click the sidebar-toggle in either the rail (bottom)
    or the panel header (top-right) to pin the panel open. The panel
    stops floating and starts taking 220 px out of the terminal
    area's flex row. Pinned state persists across restarts via
    `vertical_tabs_pinned` in `~/.local/share/con/session.json`.
- **Interactions:** click activates a tab; middle-click closes it;
  pinned-mode rows expose a hover-only `X` close affordance. The `+`
  button always spawns a new tab.

## Code map

```
crates/con-core/src/config.rs
  + TabsOrientation enum (Horizontal | Vertical) on AppearanceConfig

crates/con-core/src/session.rs
  + Session.vertical_tabs_pinned: bool (persists pin state)

crates/con-app/src/sidebar.rs       (renamed conceptually: VerticalTabsPanel)
  - SessionEntry { name, is_ssh, needs_attention }  ← was missing needs_attention
  - SessionSidebar with PanelMode = Collapsed | Pinned + bool hover_peek
  - render() returns the in-flow piece (rail or pinned body)
  - render_peek_overlay(cx) returns Option<AnyElement> for the
    floating overlay element (used only in collapsed-with-hover state)
  + SidebarCloseTab event (per-tab close from the panel)
  + surface_tone(theme, intensity) helper — blends a desaturated
    extreme-luminance overlay into theme.background so the panel
    surface reads on both light and dark themes regardless of which
    palette tokens the theme author picked

crates/con-app/src/workspace.rs
  - tabs_orientation: TabsOrientation field on ConWorkspace
  - horizontal_tabs_visible() / vertical_tabs_active() helpers
  - sync_tab_strip_motion now respects orientation (horizontal strip
    collapses in vertical mode regardless of tab count)
  - main_area is .relative(); vertical-tabs sidebar prepended; peek
    overlay appended last so it stacks above the terminal pane
  - on_settings_saved propagates orientation changes; observe(sidebar)
    saves session whenever pin state changes
  - sync_sidebar now forwards needs_attention
  - Pane drag-resize math subtracts the vertical-tabs panel width
    from horizontal split totals so resize handles stay aligned

crates/con-app/src/settings_panel.rs
  + "Tabs" group in Appearance section with the Vertical Tabs Switch
```

## Stacking gotcha (peek overlay)

The peek overlay must NOT live inside the sidebar entity's render
subtree. If it does, GPUI paints it as an absolute child of a
44-px-wide parent, then paints the terminal pane sibling on top with
`theme.background.opacity(terminal_opacity)` — the overlay reads at
~30% effective alpha through the translucent terminal bg, looking
washed out.

Instead the overlay is built by `SessionSidebar::render_peek_overlay()`
and appended by the workspace as the last child of `main_area` (which
is `.relative()`). That makes it a sibling of the terminal pane and
the agent panel, painted strictly after both, and therefore stacked
above both.

See `postmortem/2026-04-24-vertical-tabs-overlay-stacking.md` for the
full debug story.

## Visual design language

Follows `docs/design/con-design-language.md` and `CLAUDE.md`:

- **No borders.** Surface separation comes from `surface_tone`
  (foreground blended into background at 8 / 14 / 18% intensity for
  rail / body hover / panel body). The 1-px strip on the panel's
  right edge is itself an opacity-based fill, not a CSS border.
- **No shadows.** Same opacity-based approach.
- **Mono font** (Ioskeley Mono) for tab labels — terminal chrome
  consistency with the existing horizontal strip.
- **Phosphor icons only.** Used: `terminal.svg` (active tab pill),
  `globe.svg` (SSH session pill, future), `plus.svg` (new tab),
  `sidebar-simple.svg` (collapse/expand toggle), `x.svg` (close).
- **Mono-by-context.** Terminal-chrome surfaces (rail, panel header,
  tab labels) use the mono font; settings panel and agent panel
  remain on the system UI font.

## Validation

- `cargo check --workspace --all-targets` clean (including
  `RUSTFLAGS="-D warnings"`).
- `cargo test --workspace`: 119 tests pass, 0 fail.
- Visual proof on Linux (XFCE / X11 / `llvmpipe` Vulkan):
  - `docs/impl/vertical-tabs-shots/01-rail.png` — collapsed rail
  - `docs/impl/vertical-tabs-shots/02-peek.png` — hover-peek overlay
  - `docs/impl/vertical-tabs-shots/03-pinned.png` — pinned panel
  - `docs/impl/vertical-tabs-shots/04-horizontal-comparison.png`
    — horizontal strip (default) for regression comparison
- macOS / Windows: not yet smoke-tested in this PR. The render path
  is identical across platforms (panel is plain GPUI), so behavior
  should match. The macOS top bar still reserves 78 px for traffic
  lights since the vertical panel sits BELOW the top bar (no
  collision); the Windows / Linux caption-button cluster stays in
  the top bar's right edge for the same reason.

## Not in scope (follow-ups)

- Drag-to-reorder tabs (neither orientation supports it today).
- Tab groups / collapsed groups / pinned tabs (Chrome's other
  vertical-tabs features).
- Hover-peek on the right side for a future vertical agent-panel
  mode.
- Migration of the horizontal strip to share `surface_tone()` for
  visual coherence on themes whose `theme.title_bar` collapses into
  `theme.background`.
