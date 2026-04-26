## What happened

Con still felt semi-hung or fully hung when an operator opened a long real-world assistant reply in the agent panel, even after earlier fixes had improved:

- session restore
- streaming updates
- deferred markdown parsing

The most visible case was a long practical answer with headings, tables, lists, and many inline-code spans such as paths, commands, and filenames. Expanding that reply could stall the UI badly enough that the app looked non-responsive.

## Root cause

The remaining bottleneck was not markdown parsing anymore. It was the render tree shape in `crates/con-app/src/chat_markdown.rs`.

For paragraphs or table cells that contained inline code, the renderer switched away from the cheap `StyledText` path and instead built a flex-wrapped tree of many tiny GPUI elements:

- one element per text token
- one extra element per inline-code chip
- explicit wrapping/layout containers around those tokens

That was aesthetically attractive for short UI text, but it was the wrong architecture for long agent replies. Real-world assistant output often contains dozens or hundreds of inline-code spans mixed into prose, lists, and tables. The result was a very high-cardinality element tree and expensive layout work exactly when the panel needed to stay responsive.

In short:

- parse caching fixed repeated mdast work
- async parse fixed UI-thread stalls during expansion
- but the expanded markdown still rendered into too many UI elements

## Fix applied

- Removed the special flex-wrapped inline-code render path for chat markdown.
- `render_inline_content(...)` now always uses the single-layout `StyledText` path with styled text runs.
- Inline code remains visually distinct through text-run styling:
  - mono font
  - medium weight
  - background color
  - adjusted foreground color
- Expanded assistant bodies are now mounted as cached child `MarkdownDocumentView`s inside the agent panel, so GPUI can reuse their layout/paint across unrelated panel rerenders until the markdown content actually changes.

This keeps the semantic styling while collapsing the render tree back down to one text layout per paragraph/list cell/table cell instead of many small child elements.

## What we learned

- UI-thread parsing and render-tree cardinality are separate performance layers. Fixing one does not automatically fix the other.
- “Pretty” inline chips are not free. For long-form assistant output, they are the wrong primitive if they require many independently laid out elements.
- Even a cheaper document tree is still too expensive if the parent panel forces it to rerender on every unrelated state change. Large document content needs its own cached view boundary.
- In chat surfaces, dense markdown should prefer:
  - parsed IR caching
  - async parse for large bodies
  - single text layouts with styled runs

The durable rule is simple:

- use per-token element composition for small UI chrome
- use run-based text layout for large document-like content
