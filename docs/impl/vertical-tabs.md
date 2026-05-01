# Vertical Tabs

Issue: [#66](https://github.com/nowledge-co/con-terminal/issues/66) — `feat: Vertical Tabs`.

A Chrome-style vertical tab strip for the workspace, gated behind a
single config flag so horizontal tabs remain the default for every
shipped beta.

## Feature surface

| Capability | Trigger | Notes |
|---|---|---|
| Toggle orientation | *Settings → Appearance → Tabs → Vertical Tabs*, top-bar sidebar button, Command Palette, or shortcut | Live; no restart. Shortcut defaults to Cmd+B on macOS and Ctrl+Shift+B on Windows/Linux. |
| Collapsed icon rail | default in vertical mode | Smart icon per tab. |
| Floating hover card | cursor over rail icon | Anchored to cursor; shows name + subtitle + pane count + SSH/unread badges. **Does not displace** rail or terminal pane. |
| Pinned panel | click sidebar-toggle in rail or header | Width tweens 220 ms; pin state persists across restart. |
| Resize pinned panel | drag the right edge of the pinned panel | Width clamps to 184–360 px and persists across restart. |
| Activate tab | left-click row | |
| Close tab | middle-click row, or hover-X (pinned mode) | |
| New tab | `+` in rail or panel header | |
| AI label + icon | auto when `agent.suggestion_model.enabled` | Same model the inline shell completions use. SSH tabs short-circuit (no LLM). Per-tab cache + 5 s budget. Bad output is dropped; heuristic takes over. |
| Smart name (heuristic / fallback) | auto from OSC title / SSH host / cwd | Bare shells fall through to cwd basename. |
| Smart icon (heuristic / fallback) | auto from process kind | terminal / globe / code / pulse / book-open / file-code |
| Rename tab | double-click label, or context menu | Enter commits, Esc cancels. |
| Reset name | context menu | Clears `user_label` so smart auto-naming resumes. |
| Reorder tab | drag (rail or pinned), or context menu Move Up / Down | Drop indicator: pinned-mode uses a 2-px line above the target row; rail mode uses a hover-bg pill. |
| Duplicate tab | context menu | New tab in same cwd as source. |
| Close other tabs | context menu | |

## Why a hover card instead of an auto-expanding overlay

The first vertical-tabs landing auto-expanded a full panel on hover (Microsoft Edge style). It read as aggressive — *passive intent* (just trying to remember what tab 3 is) was taking over the workspace. It also broke drag-from-collapsed: the user clicks an icon to drag, the cursor leaves the icon to start the drag, the overlay folds, the drop zones disappear.

Apple's pattern (Finder sidebar item names, Safari tab thumbnails, Mail mailbox tooltips) is a **tooltip-style card** that appears next to the icon without taking over layout. We do that. The card is anchored vertically to the cursor (its row IS the icon under the cursor by construction), 240 px wide, opaque, with a 3-px primary-color stripe on its leading edge to anchor it visually to the rail.

## AI tab summarization

Naming priority becomes:

1. `user_label` — explicit override; user always wins.
2. `ai_label` + `ai_icon` — when the suggestion model is enabled and has produced one.
3. SSH host short name (no LLM needed; engine short-circuits).
4. `parse_focused_process()` heuristic.
5. cwd basename.
6. shell name / `Tab N`.

The engine lives in `con-core::tab_summary` and is constructed via `AgentHarness::tab_summary_engine()` — same `suggestion_agent_config()` the inline shell completions use, gated by the same `agent.suggestion_model.enabled` toggle. Users who turned suggestions off get **no extra LLM traffic** for tab labels.

**Prompt contract.** The model is told to return a JSON object like `{"label":"...","icon":"..."}` where `label` is 1–3 words Title Case (no emoji, no quotes, no "tab" in the name) and `icon` is one of the six closed-set keywords: `terminal | code | pulse | book-open | file-code | globe`. A tolerant bracket-balanced JSON extractor recovers fenced or prose-wrapped answers. Anything that doesn't parse cleanly is dropped — the row falls back to the heuristic name and we never silently render garbage.

**Throttling.** Per-tab cache keyed on `(cwd, top-3-recent-commands, title)` so we don't re-ask while context hasn't moved. At most one in-flight request per tab. At most one request per 5 s per tab as a budget guard against chatty PROMPT_COMMAND cwd updates.

**Triggers.** A summary request fans out from:
- `from_session` (initial seed from restored tab titles).
- `on_terminal_title_changed` (OSC title change — strongest signal a tab's purpose just shifted).
- `activate_tab` (the user is paying attention to this one — catch up if context moved).
- `record_shell_command` (new command in history captured by Con).
- `on_settings_saved` (model changed — clear cache, re-ask).
- `pump_ghostty_views` returning `true` (any output churn on any pane). The engine's per-tab 5 s budget + context-hash dedupe makes this safe to fire on every frame; in steady state it drops 100% of these calls.

**Why use `recent_output` instead of just `recent_commands`.** Earlier versions only fed the model commands explicitly captured by Con (via the input bar or control socket). That missed the most common case: the user typing directly in the terminal pane. Now the request also includes the last ~24 lines of visible scrollback (via `TerminalPane::recent_lines`), which is the strongest signal we have for "what is this tab actually doing", and works regardless of input source.

**Closure.** When a tab closes, the workspace calls `engine.forget(summary_id)` so future tabs can reuse cache slots without inheriting stale state.

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
    with an opacity-based elevated fill, needs-attention dot in the
    corner. A `caret-line-right` unfold control and `+` button sit at
    the top.
  - **Hover card.** When the cursor enters a rail icon in collapsed
    mode, a compact card (~240 px) floats beside the rail as an
    absolute overlay ABOVE the terminal pane. Mouse leave returns to
    the rail. **The card does not displace the terminal area** —
    terminal columns don't reflow when the user is just glancing at tab
    titles.
  - **Pinned.** Click the unfold control in the rail or the
    sidebar-toggle in the panel header (top-right) to pin the panel
    open. The panel stops floating and starts taking user-resizable
    width out of the terminal area's flex row. Pinned state persists
    across restarts via `vertical_tabs_pinned`; resized width persists
    via `vertical_tabs_width` in `~/.local/share/con/session.json`.
- **Interactions:** click activates a tab; middle-click closes it;
  pinned-mode rows expose a hover-only `X` close affordance. The `+`
  button always spawns a new tab.
- **Quick switch:** the top-bar sidebar button, Command Palette action,
  and `keybindings.toggle_vertical_tabs` flip between horizontal and
  vertical orientations. The action persists `appearance.tabs_orientation`
  immediately so Settings stays in sync and the next launch restores the
  selected orientation.

## Code map

```
crates/con-core/src/config.rs
  + TabsOrientation enum (Horizontal | Vertical) on AppearanceConfig
  + keybindings.toggle_vertical_tabs (Cmd+B on macOS, Ctrl+Shift+B on
    Windows/Linux by default)

crates/con-core/src/session.rs
  + Session.vertical_tabs_pinned: bool (persists pin state)
  + Session.vertical_tabs_width: Option<f32> (persists user-resized width)

crates/con-app/src/sidebar.rs       (renamed conceptually: VerticalTabsPanel)
  - SessionEntry { id, name, subtitle, icon, needs_attention, pane_count }
  - SessionSidebar with PanelMode = Collapsed | Pinned + cursor-anchored hover card
  - Pinned panel width clamps to 184–360 px; collapsed rail width stays fixed
  - render() returns the in-flow piece (rail or pinned body)
  - render_hover_card_overlay(window, cx) returns Option<AnyElement>
    for the floating card element (used only in collapsed-with-hover state)
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
  - main_area is .relative(); vertical-tabs sidebar prepended; hover
    card appended last so it stacks above the terminal pane
  - workspace owns the pinned-panel resize gesture because it already
    knows the terminal-area min width and the agent-panel width
  - on_tabs_orientation_changed propagates orientation changes; observe(sidebar)
    saves session whenever pin state changes
  - ToggleVerticalTabs action persists config, updates Settings when open,
    and is exposed through the top bar and Command Palette
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

Instead the overlay is built by `SessionSidebar::render_hover_card_overlay()`
and appended by the workspace as the last child of `main_area` (which
is `.relative()`). That makes it a sibling of the terminal pane and
the agent panel, painted strictly after both, and therefore stacked
above both.

See `postmortem/2026-04-24-vertical-tabs-overlay-stacking.md` for the
full debug story.

## Visual design language

Follows `docs/design/con-design-language.md` and `CLAUDE.md`:

- **No borders.** Surface separation comes from theme-derived
  opacity fills, including the user-configured UI opacity. The 1-px
  strip on the panel's right edge is itself an opacity-based fill, not
  a CSS border.
- **No shadows.** Same opacity-based approach.
- **System font** for tab labels; mono font is reserved for technical
  subtitles like `~/proj/path` and `user@host`.
- **Phosphor icons only.** Used: `terminal.svg` (active tab pill),
  `globe.svg` (SSH session pill), `plus.svg` (new tab),
  `caret-line-right.svg` (rail unfold), `sidebar-simple.svg`
  (panel collapse), `x.svg` (close).
- **Mono-by-context.** Terminal-path and remote-host details use the
  mono font; settings panel, agent panel, and tab names remain on the
  system UI font.

## Validation

- `cargo check --workspace --all-targets` clean (including
  `RUSTFLAGS="-D warnings"`).
- `cargo test --workspace` passes, including the `con-core::tab_summary`
  parser/cache/reorder coverage.
- Visual proof on Linux (XFCE / X11 / `llvmpipe` Vulkan): captured
  during development but not committed — `docs/**/*.png` is
  gitignored. Screenshots live in PR comments instead so they don't
  bloat the repo.
- macOS / Windows: not yet smoke-tested in this PR. The render path
  is identical across platforms (panel is plain GPUI), so behavior
  should match. The macOS top bar still reserves 78 px for traffic
  lights since the vertical panel sits BELOW the top bar (no
  collision); the Windows / Linux caption-button cluster stays in
  the top bar's right edge for the same reason.

## Not in scope (follow-ups)

- Drag-to-reorder for the horizontal tab strip. Vertical rail and
  pinned-panel reordering ship in this PR.
- Tab groups / collapsed groups / pinned tabs (Chrome's other
  vertical-tabs features).
- Hover-peek on the right side for a future vertical agent-panel
  mode.
- Migration of the horizontal strip to share `surface_tone()` for
  visual coherence on themes whose `theme.title_bar` collapses into
  `theme.background`.
