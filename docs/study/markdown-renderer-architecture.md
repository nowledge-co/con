# Markdown Renderer Architecture

## Decision

con should use the `markdown` crate's mdast parser for agent-panel Markdown and continue building a Con-owned renderer/style system on top of it.

con should not import Zed's markdown crate directly.

The mdast parser choice is intentional now: GFM tables, display math, and inline math are product requirements, and the renderer benefits from a stable block/inline tree with per-node caches.

## Why

### 1. Parsing and rendering are different problems

The current quality gap in Con's chat panel is not Markdown parsing correctness. It is rendering and typesetting:

- inline code chips need dedicated typography and spacing
- fenced code blocks need their own surface, language label, and mono treatment
- lists, blockquotes, and headings need a cleaner style ladder

The `markdown` crate gives us a normalized mdast tree with GFM and math constructs, while keeping parsing and rendering separated. That is the right fit for Con's UI renderer because parsed block entities can own cache state without reparsing during ordinary GPUI paints.

### 2. Zed is not reusable as-is

Zed's markdown renderer is better, but the crate we studied is GPL-3.0-or-later and is tightly coupled to Zed workspace crates. It is useful as a design reference, not as a dependency we can ship in Con.

That makes the long-term answer straightforward:

- keep the parser layer we already have
- build the missing renderer/style abstraction ourselves
- borrow ideas from Zed's API shape, not its code

### 3. A heavier parser would not solve the UI problem

`comrak` is a reasonable Rust Markdown library when you need HTML rendering or deeper AST mutation, but Con's current problem is not HTML generation. It is "our renderer must turn a known Markdown tree into cached GPUI text/image blocks without stalling the app."

Switching parsers again would add cost and churn without solving the typography and cache-boundary problem by itself.

## Recommended Con design

### Parser layer

Keep:

- `markdown`

Use it for:

- mdast parsing
- GFM tables
- fenced code blocks
- math flow and math text constructs

### Con-owned intermediate representation

Keep or evolve the current custom block/inline model in `chat_markdown.rs` into an explicit renderer IR:

- block nodes:
  - paragraph
  - heading
  - list
  - list item
  - blockquote
  - fenced code block
  - mermaid diagram
  - display math
  - thematic break
- inline nodes:
  - text
  - code
  - math
  - emphasis
  - strong
  - strikethrough
  - link
  - soft break
  - hard break

This is enough for the current chat surface.

### Style system

Build a dedicated `MarkdownStyle` layer with explicit slots, instead of ad hoc `div()` tweaks scattered through rendering code:

- `paragraph`
- `heading_h1` ... `heading_h6`
- `list_marker`
- `blockquote_shell`
- `blockquote_bar`
- `inline_code`
- `code_block_shell`
- `code_block_label`
- `code_block_text`
- `link`
- `rule`

Each slot should own:

- font family
- font size
- line height
- weight/style
- text color
- background color
- padding
- corner radius
- spacing above/below

That is the abstraction Zed effectively has and Con currently lacks.

### Layout rules

The renderer should make these rules explicit:

- inline code is rendered as a real mono chip with horizontal breathing room
- fenced code is rendered as a dedicated mono block, not paragraph text on a tinted box
- lists use one consistent marker gutter
- nested blocks use a clear but restrained spacing scale
- prose, thinking text, and trace output can share structure while differing in tone

Performance rule:

- long-form chat content must prefer one text layout with styled runs over per-token element trees
- fenced code blocks must prefer one highlighted text layout per block over one UI row per line
- mermaid diagram rendering must happen off the UI thread and cache the rendered image by source text + scale
- display math rendering must happen off the UI thread and cache the rendered image by source text + font-size key
- inline math should use a conservative false-positive filter so normal prose such as prices does not become math
- `mathjax-svg-rs` pulls in a real JavaScript/MathJax engine; keep it isolated to the UI crate, render only on a background executor, and revisit only with measured binary-size or cold-start data
- inline code inside dense prose, lists, or table cells should not force flex-wrapped chip segmentation at chat-message scale
- decorative inline-code chips are acceptable for short UI copy, not for large agent replies
- cache parsed/flattened text-run transforms at the markdown data layer before trying view-level caching
- isolate expensive message documents behind their own entities before reaching for whole-document cached views
- if the parent chat surface still rerenders too broadly, isolate the full assistant row behind its own entity so live streaming only repaints the active row
- do not keep the entire transcript on one `overflow_y_scroll()` flex column once replies become rich or long; move the conversation surface to GPUI `ListState` so visibility, remeasurement, and follow-tail behavior happen at the list layer
- use GPUI cached child views only for size-stable subtrees; intrinsic-height rich text is not a safe first cache boundary

## Migration path

### Phase 1

Refactor `chat_markdown.rs` so styling is driven by explicit markdown-style slots instead of embedded one-off values.

### Phase 2

Split rendering into smaller block/inline renderers:

- block renderer
- inline renderer
- code renderer

This reduces the current single-file fragility and makes typography changes less risky.

### Phase 3

Add source-range support if needed for:

- selection
- hover actions
- future copy/open behaviors on links or code blocks

The mdast layer can be extended with source-position tracking if we need this later.

## Conclusion

The long-term elegant path is:

- keep `markdown` mdast parsing
- keep a Con-owned renderer
- formalize a real markdown style/render abstraction
- do not import Zed's GPL markdown crate
- do not switch parsers just to compensate for missing renderer design

That is the lowest-redundancy, license-safe, long-term architecture.

## Primary sources

- `markdown` Rust docs: <https://docs.rs/markdown/latest/markdown/>
- `comrak` Rust docs: <https://docs.rs/comrak/latest/comrak/>
- CommonMark spec: <https://spec.commonmark.org/>
