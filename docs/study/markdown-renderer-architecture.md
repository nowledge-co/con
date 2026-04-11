# Markdown Renderer Architecture

## Decision

con should keep `pulldown-cmark` as the Markdown parser and continue building a Con-owned renderer/style system on top of it.

con should not import Zed's markdown crate directly.

con should not switch to a heavier AST-first parser unless the product genuinely needs AST mutation or CommonMark extensions that `pulldown-cmark` cannot provide cleanly.

## Why

### 1. Parsing and rendering are different problems

The current quality gap in Con's chat panel is not Markdown parsing correctness. It is rendering and typesetting:

- inline code chips need dedicated typography and spacing
- fenced code blocks need their own surface, language label, and mono treatment
- lists, blockquotes, and headings need a cleaner style ladder

`pulldown-cmark` already gives us a clean event stream for this.

Upstream explicitly positions it as a pull parser with parsing and rendering separated, and notes that it can also support source offsets when needed. That is the right fit for Con's UI renderer.

### 2. Zed is not reusable as-is

Zed's markdown renderer is better, but the crate we studied is GPL-3.0-or-later and is tightly coupled to Zed workspace crates. It is useful as a design reference, not as a dependency we can ship in Con.

That makes the long-term answer straightforward:

- keep the parser layer we already have
- build the missing renderer/style abstraction ourselves
- borrow ideas from Zed's API shape, not its code

### 3. A heavier parser would not solve the UI problem

`comrak` is a reasonable Rust Markdown library when you need an AST and HTML rendering, but Con's current problem is not "we lack an AST." It is "our renderer does not have enough explicit style slots and layout structure."

Switching parsers would add cost and churn without solving the typography problem by itself.

## Recommended Con design

### Parser layer

Keep:

- `pulldown-cmark`

Use it for:

- CommonMark event parsing
- extension flags we actually need
- optional source-offset tracking later through event ranges

### Con-owned intermediate representation

Keep or evolve the current custom block/inline model in `chat_markdown.rs` into an explicit renderer IR:

- block nodes:
  - paragraph
  - heading
  - list
  - list item
  - blockquote
  - fenced code block
  - thematic break
- inline nodes:
  - text
  - code
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

`pulldown-cmark` can support that without forcing a parser rewrite.

## Conclusion

The long-term elegant path is:

- keep `pulldown-cmark`
- keep a Con-owned renderer
- formalize a real markdown style/render abstraction
- do not import Zed's GPL markdown crate
- do not switch parsers just to compensate for missing renderer design

That is the lowest-redundancy, license-safe, long-term architecture.

## Primary sources

- `pulldown-cmark` upstream README: <https://github.com/pulldown-cmark/pulldown-cmark>
- `pulldown-cmark` Rust docs: <https://docs.rs/pulldown-cmark/latest/pulldown_cmark/>
- `comrak` Rust docs: <https://docs.rs/comrak/latest/comrak/>
- CommonMark spec: <https://spec.commonmark.org/>
