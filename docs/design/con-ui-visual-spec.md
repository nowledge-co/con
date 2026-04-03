# con UI and Visual Spec

## Purpose

This document defines the visual and interaction system for **con**.

It translates the product brief into practical guidance for:

- layout
- typography
- color
- spacing
- surface treatment
- motion
- component anatomy

It should be read together with:

- `docs/design/con-design-language.md`
- `docs/design/con-ux-product-spec.md`

---

## Visual Positioning

con should feel:

- native
- light-first (Flexoki Light default, Flexoki Dark available)
- elegant
- low-noise
- terminal-first
- fast even at a glance
- trustworthy under operational pressure
- borderless and shadowless

The app should communicate quality through restraint.
Not through decoration.

---

## Source Interpretation

From the Nowledge Labs reference system, con should inherit:

- typography-led hierarchy
- opacity-based neutrals
- monochrome default surfaces
- generous spacing
- subtle containers
- calm settings and navigation patterns
- progressive disclosure

From Warp-like terminal UX, con should inherit:

- structured discoverability
- thoughtful command/action composition
- keyboard-forward usability
- clearer session and task orientation

What makes con distinct is that all of these qualities must be adapted to a **dark-first, terminal-native** environment.

---

## Theme Strategy

### Dark-first by default

The design system should be built from dark mode outward.
Dark mode is the primary design language, optimized for long terminal sessions.

### Light mode as premium alternative

Light mode must feel equally polished, not an afterthought.
Reference: Nowledge production UI demonstrates the quality bar.

### Surface philosophy

Most surfaces should be created through:

- slight value shifts
- opacity-based overlays
- spacing and text hierarchy
- occasional soft dividers

Not through obvious card borders, loud gradients, or deep shadow stacks.

---

## Dual Theme Color Specifications

### Dark Mode Palette

| Role | Value | Description |
|------|-------|-------------|
| background | oklch(0.11 0.01 260) | Deep blue-black ink |
| terminal | oklch(0.09 0.012 260) | Deepest surface |
| chrome | oklch(0.13 0.01 260) | Slightly elevated |
| elevated | oklch(0.15 0.01 260) | Lifted surfaces |
| card | oklch(0.14 0.01 260) | Panel backgrounds |
| primary | oklch(0.65 0.15 245) | Calm blue focus |
| accent | oklch(0.55 0.12 280) | Lavender assistant |
| success | oklch(0.65 0.18 145) | Green success |
| warning | oklch(0.75 0.15 80) | Amber caution |
| destructive | oklch(0.55 0.2 25) | Red danger |
| border | oklch(0.22 0.01 260) | Hairline |
| border-subtle | oklch(0.18 0.008 260) | Nearly invisible |
| text-primary | oklch(0.92 0.01 260) | Main text |
| text-muted | oklch(0.55 0.01 260) | Secondary |

### Light Mode Palette

| Role | Value | Description |
|------|-------|-------------|
| background | oklch(0.995 0.002 90) | Pure white with warmth |
| terminal | oklch(0.98 0.003 260) | Slightly cooler for code |
| chrome | oklch(1 0 0) | Pure white |
| elevated | oklch(0.995 0.002 90) | Subtle warmth |
| card | oklch(1 0 0) | Clean white |
| primary | oklch(0.55 0.15 245) | Refined blue |
| accent | oklch(0.65 0.12 280) | Lavender presence |
| success | oklch(0.55 0.18 145) | Green success |
| warning | oklch(0.65 0.15 80) | Amber caution |
| destructive | oklch(0.55 0.2 25) | Red danger |
| border | oklch(0.92 0.003 260) | Hairline |
| border-subtle | oklch(0.95 0.002 260) | Nearly invisible |
| text-primary | oklch(0.18 0.01 260) | Main text |
| text-muted | oklch(0.50 0.01 260) | Secondary |

### Theme-Agnostic Rules

- All semantic tokens must work in both themes
- Contrast ratios: 4.5:1 minimum for body text
- Focus rings: visible without being gaudy
- Color never the only differentiator for state

---

## Color System

## Neutral roles

Use a small set of semantic neutral roles instead of many explicit gray values.

### Recommended roles

- `bg/app`
- `bg/terminal`
- `bg/chrome`
- `bg/elevated`
- `bg/overlay`
- `bg/subtle`
- `border/subtle`
- `border/emphasis`
- `text/primary`
- `text/secondary`
- `text/muted`
- `text/faint`

### Neutral behavior

- base surfaces should feel like deep blue-black ink, not flat gray
- elevated surfaces should use slight lift, not heavy fill
- large neutral fills should stay light in opacity
- dividers should be hairline and quiet

## Semantic colors

Color should be used rarely and precisely.

### Role mapping

- blue: focus, selected route, active state
- green: success, healthy, completed
- amber: pending, approval needed, caution
- red: failure, destructive, dangerous scope
- lavender: assistant presence and streaming state when useful

### Rule

Never rely on color alone for critical meaning.
Use text, shape, or icon support as well.

---

## Typography

Typography is one of the main carriers of polish in con.

## UI type

Recommended stack on macOS:

- `SF Pro Text` / system UI font

Use:

- regular for body
- medium for labels and controls
- semibold sparingly for headings and state emphasis

## Terminal type

Recommended defaults for exploration:

- `IoskeleyMono`

Choose based on:

- punctuation clarity
- small-size readability
- shell prompt legibility
- box-drawing quality
- low fatigue in long sessions

## Type scale

Keep the scale tight and disciplined.

### Suggested sizes

- 11 px: micro labels, metadata, status tags
- 12 px: secondary UI text, tab labels, helpers
- 13 px: default UI body text
- 14 px: composer text, primary assistant body text
- 16 px: section headers and stronger labels
- 20–24 px: large titles only where appropriate

### Rules

- use opacity before bold when possible
- keep assistant prose readable and compact
- use mono only for terminal data, code, commands, paths, diffs, and technical identifiers
- avoid too many weights and sizes in a single surface

---

## Spacing and Radius

## Spacing rhythm

Use an 8 px base rhythm with 4 px support increments.

### Core scale

- 4 px micro separation
- 8 px tight control spacing
- 12 px compact layout spacing
- 16 px standard padding
- 20 px roomy grouping
- 24 px section spacing
- 32 px major breathing room

### Where to be generous

- assistant cards
- settings sections
- approval sheets
- modal overlays
- outer shell composition

### Where to stay tight

- tab strip
- pane headers
- metadata rows
- inline controls
- chip groups

## Radius

Use a small family of radii.

### Suggested set

- 8 px for tabs and small controls
- 10–12 px for chips and compact surfaces
- 14–16 px for panels, composer, and overlays

Avoid mixing too many radii in one screen.

---

## Elevation Model

Use a small number of surface levels.

### Level 0 — terminal

The deepest surface.
Flat and calm.

### Level 1 — shell chrome

Titlebar, metadata rail, pane headers, composer container.
Slightly lifted from terminal.

### Level 2 — companion surfaces

Assistant panel, timeline cards, inline explain surfaces.
Soft fill, subtle shape.

### Level 3 — decision surfaces

Command palette, approval sheet, settings modal.
May use stronger contrast and slightly more explicit edges.

### Rule

Elevation should come mostly from contrast and layering, not shadow theatrics.

---

## Window Anatomy

The window should be composed as follows:

1. titlebar and tab strip
2. metadata rail
3. main work area
4. persistent composer
5. optional overlays

## 1. Titlebar and tab strip

### Purpose

- native window identity
- tab management
- quick high-level actions

### Guidance

- low-profile height
- quiet active tab treatment
- minimal border usage
- right-side utility cluster stays understated

The titlebar should frame the workspace, not become a control dashboard.

## 2. Metadata rail

A thin contextual strip below the titlebar.

### Use it for

- repo or folder name
- branch
- remote host when relevant
- task or assistant status
- subtle session state

### Guidance

- make it glanceable in under a second
- use subdued text and restrained badges
- avoid overloading it with every possible status

## 3. Main work area

Contains split panes and, when present, the assistant companion.

### Guidance

- keep the terminal the visual anchor
- dividers stay hairline
- pane headers are optional but useful
- assistant should dock elegantly and not feel bolted on

## 4. Composer

The composer should be persistent, calm, and visually stable.

### Guidance

- compact by default
- expands gracefully for multiline asks
- route selection is visible
- attached context is compact and removable
- focus state is obvious but refined

## 5. Overlays

Used for:

- command palette
- approvals
- settings
- search
- session switching

These should feel like light sheets, not bulky modals.

---

## Pane Design

## Pane headers

Pane headers should provide orientation without feeling like cards.

### Suggested content

- pane title or cwd
- process label
- remote/local state
- activity state
- lightweight actions on hover or focus

### Guidance

- keep them short
- use typography and spacing before icons
- let hover actions stay hidden until needed

## Active pane state

Active pane state must be obvious enough for keyboard-heavy use.

### Use a mix of

- slightly stronger edge or divider
- clearer title text
- stronger caret/focus treatment
- subtle semantic state where useful

Avoid thick glowing frames or loud neon focus rings.

---

## Assistant Companion

The assistant panel should feel like part of the terminal workspace, not a separate product.

## Widths to design

- collapsed rail
- compact docked state
- full companion state

### Suggested defaults

- collapsed: narrow signal rail
- open: around 340–380 px on a comfortable desktop width
- resizable when the layout allows

## Panel model

Design it as a **task timeline**, not a chat transcript.

### Timeline sections

- intent
- context attached
- plan
- tool calls
- outputs
- diffs / artifacts
- final summary

### Visual guidance

- one calm card model repeated consistently
- strong title + subdued metadata
- collapsible detail regions
- code and diff areas get mono treatment and slightly firmer containment

---

## Composer Specification

The composer is one of the defining surfaces of con.

## Structure

- route chip on the left
- primary input region
- attached context pills
- right-side send or action affordance
- minimal but visible hinting

## States

- idle
- focused
- multiline expanded
- context attached
- waiting approval
- disabled or unavailable when appropriate

## Styling guidance

- soft container
- no heavy outline by default
- stronger but still elegant focus state
- placeholder low contrast but readable
- pills remain compact and quiet

### Example context pills

- `Pane: backend`
- `Repo: kingston`
- `Branch: main`
- `Selection: 18 lines`
- `Last command failed`

---

## Tool and Approval Cards

These are operational surfaces and should feel more explicit than passive assistant text.

## Tool cards should show

- action name
- target scope
- short command or file summary
- state
- risk level
- key actions

## Approval cards should show

- what will happen
- where it will happen
- why approval is required
- direct approve / edit / deny actions
- expandable detail for full output or command text

### Visual guidance

- use semantic badges sparingly
- do not rely on giant warning panels
- let the severity come from wording, icon, and precise accent color

---

## Command Palette

The command palette should be a first-class surface.

## Use it for

- navigate tabs and panes
- open projects or sessions
- invoke product actions
- attach context
- discover skills and commands
- access settings quickly

## Visual guidance

- centered lightweight sheet
- immediate keyboard focus
- strong text hierarchy
- minimal visual clutter
- grouped result sets with compact metadata

---

## Settings

The Nowledge Labs settings screens are a strong quality reference for calm configuration UI.
con should adopt the same level of restraint while adapting it for a darker, more operational context.

## Guidance

- category navigation should be simple and low-noise
- advanced provider and automation settings stay collapsed until relevant
- settings groups should breathe
- controls should look calm, not enterprise-heavy

Good categories may include:

- appearance
- terminal
- keybindings
- AI providers and models
- automation / socket permissions
- sessions

---

## Notifications

Notifications should help people resume flow.
They should not compete with the terminal.

## Preferred surfaces

- pane signal
- tab badge
- subtle in-window toast
- system notification only when justified

## Styling guidance

- neutral by default
- semantic color only for strong meaning
- concise title and optional one-line detail
- low visual mass

---

## Motion

Motion should make con feel smoother and faster.

## Principles

- short durations
- low travel distance
- opacity + slight translation as default
- no bounce in primary workflows

## Suggested durations

- hover and focus: 80–120 ms
- panel open/close: 140–180 ms
- overlay open/close: 180–220 ms
- notification: 160–220 ms

## Recommended uses

- assistant reveal
- disclosure rows
- composer growth
- notification appearance

## Avoid

- spring-heavy motion
- large scale shifts
- long ornamental transitions

---

## Accessibility and Legibility

Minimum expectations:

- contrast remains strong in dark mode
- focus states are visible without being gaudy
- semantic state is never color-only
- small text remains readable
- motion can be reduced
- mono and sans usage remain intentional and consistent

---

## Prototype Screen Checklist

The design team should produce polished views for at least:

- default local workspace
- multi-pane workspace with assistant open
- failed command recovery state
- approval / diff review state
- SSH / tmux workspace state
- background task completion state
- command palette / attach-context flow
- settings surface

---

## Implementation Alignment Notes

The current code already points toward a strong architecture:

- `crates/con/src/workspace.rs` gives a useful shell composition model
- `crates/con/src/terminal_view.rs` keeps the terminal primary
- `crates/con/src/input_bar.rs` proves the value of a bottom-anchored composer
- `crates/con/src/agent_panel.rs` is the seed of the future companion timeline
- `crates/con/src/theme.rs` should evolve from hard-coded values into semantic design tokens

The next design pass should refine this direction, not replace it with a totally different product shape.

---

## v0.8 Prototype Reference (Final)

The React/Next.js prototype at `components/con/terminal-app.tsx` serves as the production-ready design reference for GPUI implementation.

### Key Design Decisions (Final)

**Colors**

- Dark base: `#0a0a0a` (pure black, not gray)
- Light content: `#ffffff`, light chrome: `#fafafa`
- Primary accent: `#007AFF` (Apple system blue)
- Opacity overlays: white 3-8% (dark), black 2-6% (light)
- Amber for warnings/broadcast, emerald for success, red for destructive

**Typography**

- UI: System sans (SF Pro), 10-17px range
- Terminal: Mono, 13px default, leading-[1.7]
- Weights: regular, medium, semibold (no bold in UI)
- Hierarchy via size, weight, and opacity (not color)

**Spacing**

- Base unit: 4px
- Standard padding: 16-20px
- Component gaps: 8-12px
- Generous outer margins: 20px

**Radii**

- Small controls (checkboxes): 4px
- Buttons/inputs: 8-12px
- Cards/modals: 16px (rounded-2xl)
- Pills/badges: fully rounded

**Interactions**

- Hover: +3-4% opacity fill
- Active: scale-95 for tactile feedback
- Focus: no rings, just subtle background lift
- Transitions: 150ms default, all properties

### Component Reference

| Component | Dark BG | Light BG | Notes |
|-----------|---------|----------|-------|
| Window chrome | #0a0a0a | #fafafa | Matches base, no border |
| Sidebar | #0a0a0a | #fafafa | Same as chrome |
| Terminal content | #0a0a0a | #ffffff | Content area pure white in light |
| Pane headers | white/1.5% | black/1.5% | Barely visible lift |
| Input container | white/3% | black/2% | Lifts on focus |
| Status bar | border-t 4% | border-t 4% | Single hairline separator |
| Modals | #1c1c1e | #ffffff | Slightly lifted in dark |
| Target pills selected | #007AFF | #007AFF | Same blue both themes |
| Target pills unselected | transparent | transparent | Text only, hover reveals fill |

### GPUI Implementation Notes

1. **No shadows** - All elevation through opacity-based fills
2. **No borders by default** - Only for status bar and pane dividers (4% opacity)
3. **Checkbox style** - 15x15px, 4px radius, 1px border unselected, solid fill selected
4. **Send button** - 32x32px circle, Apple Messages style
5. **Mode detection** - Shell vs Agent auto-detected from input content
6. **Broadcast mode** - Amber "ALL" badge appears when multiple panes targeted
