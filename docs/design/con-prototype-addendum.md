# con Prototype Addendum — Visual Design Specifications

## Purpose

This document supplements the core design specs with concrete visual decisions validated through the interactive prototype. It provides implementation-ready specifications for the UX design team.

---

## Interactive Prototype

The prototype is built as a Next.js web application for rapid visual exploration. It demonstrates 8 key views:

1. **Default Workspace** — Clean terminal-first state
2. **Split Panes** — Multi-pane terminal layout
3. **Assistant Panel** — AI companion with task timeline
4. **Error Recovery** — Failed command with quick actions
5. **Approval Flow** — Tool execution approval sheet
6. **SSH Workspace** — Remote session with clear context
7. **Command Palette** — Unified search and actions
8. **Settings** — Calm configuration surfaces

---

## Color System (Validated)

### Base Palette

The prototype validates a **deep blue-black ink** base rather than flat gray:

```css
/* Terminal - Level 0 (deepest) */
--terminal: oklch(0.09 0.012 260);

/* Background - Level 0.5 */
--background: oklch(0.11 0.01 260);

/* Chrome - Level 1 (titlebar, rails) */
--chrome: oklch(0.13 0.01 260);

/* Card - Level 2 (panels, cards) */
--card: oklch(0.14 0.01 260);

/* Elevated - Level 3 (overlays) */
--elevated: oklch(0.15 0.01 260);
```

### Semantic Colors

```css
/* Focus / Selection / Links */
--primary: oklch(0.65 0.15 245);  /* Calm blue */

/* Assistant Presence */
--accent: oklch(0.55 0.12 280);   /* Soft lavender */

/* Success / Healthy */
--success: oklch(0.65 0.18 145);  /* Terminal green */

/* Warning / Approval Needed */
--warning: oklch(0.75 0.15 80);   /* Amber */

/* Error / Destructive */
--destructive: oklch(0.55 0.2 25); /* Red */
```

### Text Hierarchy (Dark)

```css
/* Primary text - full brightness */
--foreground: oklch(0.92 0.01 260);

/* Secondary text - slightly dimmed */
--chrome-foreground: oklch(0.85 0.01 260);

/* Muted text - significantly dimmed */
--muted-foreground: oklch(0.55 0.01 260);

/* Terminal content */
--terminal-foreground: oklch(0.88 0.01 260);
```

---

## Light Mode Palette (Nowledge-inspired)

The light theme draws inspiration from the Nowledge production UI:

### Base Palette (Light)

```css
/* Background - Pure white with subtle warmth */
--background: oklch(0.995 0.002 90);

/* Terminal - Slightly cooler for code readability */
--terminal: oklch(0.98 0.003 260);

/* Chrome - Pure white */
--chrome: oklch(1 0 0);

/* Card - Clean white */
--card: oklch(1 0 0);

/* Secondary - Soft gray */
--secondary: oklch(0.96 0.005 260);

/* Muted - Elegant gray */
--muted: oklch(0.97 0.003 260);
```

### Semantic Colors (Light)

```css
/* Focus / Selection / Links - Refined blue */
--primary: oklch(0.55 0.15 245);

/* Assistant Presence - Lavender */
--accent: oklch(0.65 0.12 280);

/* Success - Green */
--success: oklch(0.55 0.18 145);

/* Warning - Amber */
--warning: oklch(0.65 0.15 80);

/* Error - Red */
--destructive: oklch(0.55 0.2 25);
```

### Borders (Light)

```css
/* Hairline - nearly invisible */
--border: oklch(0.92 0.003 260);

/* Subtle - extremely light */
--border-subtle: oklch(0.95 0.002 260);

/* Emphasis - slightly visible */
--border-emphasis: oklch(0.85 0.005 260);
```

### Text Hierarchy (Light)

```css
/* Primary text - strong */
--foreground: oklch(0.18 0.01 260);

/* Secondary text - muted */
--chrome-foreground: oklch(0.25 0.01 260);

/* Muted text - significantly lighter */
--muted-foreground: oklch(0.50 0.01 260);

/* Terminal content */
--terminal-foreground: oklch(0.15 0.01 260);
```

### Light Mode Design Patterns

1. **Cards**: Pure white with 1px hairline border (barely visible)
2. **Shadows**: Minimal to none; rely on borders for separation
3. **Active states**: Subtle background fill rather than strong colors
4. **Segmented controls**: Active segment is white with soft shadow
5. **Selection**: 20% opacity blue overlay
6. **Scrollbars**: Light gray, darker on hover

---

## Typography Scale (Finalized)

### UI Type Sizes

| Size | Usage | Line Height |
|------|-------|-------------|
| 10px | Micro labels, keyboard hints, status tags | 1.4 |
| 11px | Metadata, context pills, secondary info | 1.4 |
| 12px | Tab labels, assistant cards, secondary controls | 1.5 |
| 13px | Default UI body, composer input, assistant prose | 1.5 |
| 14px | Section headers, emphasized content | 1.5 |
| 16px | Panel titles, overlay headers | 1.4 |

### Font Weights

- **Regular (400)** — Body text, descriptions
- **Medium (500)** — Labels, active states, emphasized text
- **Semibold (600)** — Headers, strong emphasis (use sparingly)

### Terminal Type

- **Size**: 13px (configurable 10-18px)
- **Line Height**: 1.6 (leading-relaxed)
- **Fonts**: IoskeleyMono > Iosevka

---

## Spacing System (8px Grid)

### Core Scale

| Token | Value | Usage |
|-------|-------|-------|
| `1` | 4px | Micro gaps, icon margins |
| `2` | 8px | Tight control spacing, pill padding |
| `3` | 12px | Compact layout spacing |
| `4` | 16px | Standard padding, card insets |
| `5` | 20px | Roomy groupings |
| `6` | 24px | Section spacing |
| `8` | 32px | Major breathing room |

### Component Spacing

- **Tab strip**: 8px horizontal gap between tabs
- **Metadata rail**: 16px horizontal gaps between items
- **Composer**: 12px gap between elements
- **Assistant cards**: 12px gap between cards
- **Settings sections**: 32px gap between groups

---

## Border Radius Scale

| Token | Value | Usage |
|-------|-------|-------|
| `sm` | 4px | Badges, inline tags |
| `md` | 8px | Tabs, buttons, chips |
| `lg` | 12px | Cards, panels |
| `xl` | 16px | Overlays, modals |

---

## Component Specifications

### Titlebar

```
Height: 40px
Background: var(--chrome)
Border: 1px bottom, var(--border-subtle)

Traffic lights: 
  - Size: 12px diameter
  - Gap: 8px
  - Margin-left: 16px

Tabs:
  - Padding: 6px 12px
  - Font: 11px mono
  - Radius: 8px
  - Active: bg-terminal
  - Inactive: transparent, hover bg-secondary
```

### Metadata Rail

```
Height: 28px
Background: var(--chrome)
Border: 1px bottom, var(--border-subtle)
Padding: 0 16px

Items:
  - Gap: 16px
  - Font: 11px
  - Icon: 12px, muted
  - Text: mono for values

Status indicators:
  - Success: green circle + "clean"
  - Modified: amber circle + "modified"  
  - Error: red circle + "build failed"
```

### Terminal Pane

```
Pane header:
  - Height: 28px
  - Active: stronger border, indicator bar
  - Inactive: muted border, no indicator

Active indicator:
  - Width: 4px
  - Height: 12px
  - Radius: full
  - Color: var(--primary)

Content:
  - Padding: 16px
  - Font: 13px mono
  - Line height: 1.6
  
Cursor:
  - Width: 8px
  - Height: 16px
  - Color: var(--foreground)
  - Animation: pulse (1s)
```

### Composer

```
Container:
  - Border-top: 1px var(--border-subtle)
  - Background: var(--chrome)
  - Padding: 12px

Mode selector:
  - Padding: 6px 10px
  - Font: 12px medium
  - Radius: 8px
  - Background: var(--secondary)

Input:
  - Min-height: 36px (single line)
  - Max-height: 120px (multiline)
  - Font: 13px mono
  - Radius: 8px
  - Border: 1px var(--border)
  - Focus: ring var(--ring)

Context pills:
  - Padding: 4px 8px
  - Font: 11px
  - Radius: 6px
  - Gap: 8px
  - Variants: default, error, warning

Send button:
  - Size: 36px square
  - Radius: 8px
  - Active: bg-primary
  - Inactive: bg-secondary
```

### Assistant Panel

```
Width states:
  - Collapsed: 40px
  - Expanded: 360px

Header:
  - Height: 40px
  - Status indicator: 8px diameter pulse
  
Timeline cards:
  - Radius: 8px
  - Border: 1px var(--border-subtle)
  - Accent border: var(--accent)/30
  - Gap: 12px between cards
  
Card header:
  - Padding: 8px 12px
  - Icon: 14px
  - Badge: 10px, bg-secondary
  
Card content:
  - Padding: 0 12px 12px

Tool call cards:
  - Padding: 8px
  - Radius: 6px
  - Background: var(--secondary)/50
  - Status badges: colored backgrounds
```

### Approval Sheet

```
Position: bottom-center, above composer
Width: max 600px
Margin: 16px sides, 96px bottom

Container:
  - Radius: 12px
  - Border: 1px var(--warning)/30
  - Shadow: 2xl

Header:
  - Padding: 12px 16px
  - Background: var(--warning)/5
  - Border-bottom: var(--warning)/20

Diff preview:
  - Max-height: 200px
  - Background: var(--terminal)
  - Font: 11px mono
  - Line numbers: muted/50

Actions:
  - Gap: 12px
  - Button height: 40px
  - Primary: bg-success
  - Secondary: bg-secondary
  - Destructive: text only
```

### Command Palette

```
Position: top-center, 15vh from top
Width: max 560px

Search input:
  - Padding: 12px 16px
  - Font: 14px
  - Placeholder: muted

Results:
  - Max-height: 400px
  - Item height: ~44px
  - Item padding: 8px 16px
  - Selected: bg-primary
  - Hover: bg-secondary

Group headers:
  - Font: 10px uppercase
  - Color: muted
  - Padding: 6px 16px
```

### Settings Panel

```
Size: 800px × 600px (centered)
Layout: 200px sidebar + flex content

Sidebar:
  - Background: var(--chrome)
  - Item padding: 8px 12px
  - Active: bg-primary

Content:
  - Padding: 24px

Section spacing: 32px
Control groups: 12px gap
```

---

## Motion Specifications

### Durations

| Type | Duration | Easing |
|------|----------|--------|
| Hover/Focus | 100ms | ease-out |
| Panel open/close | 150ms | ease-out |
| Overlay appear | 180ms | ease-out |
| Overlay dismiss | 120ms | ease-in |

### Animations

```css
/* Cursor blink */
@keyframes cursor-pulse {
  0%, 100% { opacity: 1; }
  50% { opacity: 0; }
}
animation: cursor-pulse 1s infinite;

/* Status pulse */
@keyframes status-pulse {
  0%, 100% { opacity: 1; }
  50% { opacity: 0.5; }
}
animation: status-pulse 2s infinite;

/* Panel slide */
transform: translateX(0);
transition: transform 150ms ease-out;
```

---

## Keyboard Interactions

### Global

| Key | Action |
|-----|--------|
| `⌘K` | Command palette |
| `⌘,` | Settings |
| `⌘J` | Toggle assistant |
| `⌘\` | Switch pane |
| `⌘T` | New tab |
| `⌘W` | Close tab |

### Composer

| Key | Action |
|-----|--------|
| `↵` | Send |
| `⇧↵` | Newline |
| `Tab` | Autocomplete |
| `⌃1/2/3` | Switch mode |
| `⌘⇧A` | Attach file |

### Approval

| Key | Action |
|-----|--------|
| `Y` | Approve |
| `E` | Edit first |
| `N` | Deny |
| `D` | View full diff |

---

## Accessibility

### Contrast Requirements

- All text meets WCAG AA (4.5:1 for normal, 3:1 for large)
- Focus rings visible at 3:1 minimum
- Error states have icon + color + text support

### Focus Management

- All interactive elements have visible focus states
- Overlays trap focus
- Keyboard navigation follows logical order

### Screen Reader

- Use semantic HTML (button, input, etc.)
- ARIA labels for icon-only buttons
- Live regions for status updates

---

## Implementation Notes

### For GPUI

The prototype uses web CSS custom properties. For GPUI implementation:

1. **Colors**: Convert OKLCH to sRGB hex or use Gpui::hsla()
2. **Typography**: Use system fonts with fallback stack
3. **Spacing**: Use density-aware layout primitives
4. **Motion**: Match web timing with native animation curves

### Critical Surfaces

Priority order for GPUI implementation:

1. Terminal pane rendering (PTY + grid)
2. Composer with mode switching
3. Titlebar and tab strip
4. Assistant panel timeline
5. Approval sheet
6. Command palette
7. Settings overlay

---

## Next Steps

1. **UX team**: Create high-fidelity Figma from these specs
2. **Eng team**: Validate GPUI color/spacing primitives
3. **Design review**: Refine motion and micro-interactions
4. **User testing**: Validate approval flow and recovery patterns

---

## Reference Materials

### Nowledge Production UI (Design Reference)

The following screenshots from Nowledge production demonstrate the target quality level for con's light mode and component patterns:

#### 1. Knowledge Graph View

![Graph View](https://hebbkx1anhila5yf.public.blob.vercel-storage.com/Screenshot%202026-03-12%20at%2018.46.49-Wk1dLpYt1EyWqksABc9Jm6YCvYB0qy.png)

Key patterns:

- Lavender/purple accent for data visualization
- Clean white canvas with minimal borders
- Right panel with segmented controls (Graph Algo / Inspection)
- Slider controls with clear value display
- Action buttons: filled primary, outlined secondary

#### 2. Timeline View

![Timeline View](https://hebbkx1anhila5yf.public.blob.vercel-storage.com/Screenshot%202026-03-12%20at%2018.46.17-tTIMjvsd6GM4LojxdJ8T3nLJuGKZdq.png)

Key patterns:

- Filter chips with "All" active state
- Expandable cards with preview content
- Version history timeline in sidebar
- Entity tags as compact pills
- Modal/popover with action footer

#### 3. Memories List View

![Memories View](https://hebbkx1anhila5yf.public.blob.vercel-storage.com/Screenshot%202026-03-12%20at%2018.46.24-cm6EP4jx1GwesUEYkD3ifKc1zW4faX.png)

Key patterns:

- Search with mode switcher (Normal / Deep)
- Results count and refresh action
- List items with title/description/metadata hierarchy
- Star ratings (right-aligned)
- Tag pills inline with items
- Filter button with count indicator

#### 4. AI Tasks View

![AI Now View](https://hebbkx1anhila5yf.public.blob.vercel-storage.com/Screenshot%202026-03-12%20at%2018.46.28-5AcLqNPohPGYxl87YwCjciFP8akyrR.png)

Key patterns:

- Two-column master/detail layout
- Task list with status indicators (paused)
- Suggestion cards in empty state
- Bottom composer with mode chips (Memory, Plan, Research)
- Compact metadata (message count, time elapsed)

#### 5. Integrations View

![Integrations View](https://hebbkx1anhila5yf.public.blob.vercel-storage.com/Screenshot%202026-03-12%20at%2018.47.00-uH9FcecX1h0zjvpga5QwFe0mkTz7zx.png)

Key patterns:

- Tab navigation (MCP / Browser Extension / Thread Import / More)
- Illustrated explanation area
- Platform/service badges with icons
- Card grid for extension options
- Filled action buttons

#### 6. Settings Preferences

![Settings View](https://hebbkx1anhila5yf.public.blob.vercel-storage.com/Screenshot%202026-03-12%20at%2018.47.11-UDecnv8SxntFdBDvjZworpIyqpfnUF.png)

Key patterns:

- Left sidebar navigation
- Segmented controls for theme (Light/Dark/System)
- Language and font size selectors
- Keyboard shortcut display with kbd badges
- Toggle switches (filled when on)
- Section grouping with generous spacing

### Design Principles Extracted

From the Nowledge reference, con should adopt:

1. **Whitespace mastery** — Generous padding, sections breathe
2. **Hairline borders** — Present but nearly invisible
3. **Segmented controls** — For any 2-4 option choice
4. **Lavender accent** — Consistent assistant/AI presence color
5. **Typography hierarchy** — Size + weight + opacity as primary tools
6. **Kbd badges** — For all keyboard shortcuts
7. **Clean toggles** — Dark fill when active, minimal styling
8. **Compact pills** — For tags, filters, context
9. **Status indicators** — Small dots with semantic colors
10. **Section labels** — ALL CAPS, 10-11px, very muted
