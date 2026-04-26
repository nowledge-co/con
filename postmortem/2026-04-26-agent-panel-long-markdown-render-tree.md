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
- Added inline render caches for paragraphs, headings, and table cells so repeated rich-render passes can reuse flattened `SharedString` + `TextRun` output instead of rebuilding those transforms every time.
- Moved assistant message bodies behind their own markdown-document entities so unrelated agent-panel updates do not force a full message-body rerender.
- Kept whole-document view caching out of the final design. A prior attempt at caching the entire markdown subtree at the wrong boundary caused overlap/layout regressions with tool cards.
- Collapsed fenced code blocks from one GPUI row per line down to one highlighted `StyledText` layout per block, which sharply reduces render-tree size for long command/code-heavy replies.

This keeps the semantic styling while collapsing the render tree back down to one text layout per paragraph/list cell/table cell instead of many small child elements.

## What we learned

- UI-thread parsing and render-tree cardinality are separate performance layers. Fixing one does not automatically fix the other.
- “Pretty” inline chips are not free. For long-form assistant output, they are the wrong primitive if they require many independently laid out elements.
- GPUI view caching is not a universal answer. Caching an intrinsic-height rich-text subtree at the wrong boundary can create layout bugs. Data-transform caches are the safer first move.
- Entity isolation is often a better boundary than cached views for large document subtrees. It narrows invalidation without lying about layout.
- In chat surfaces, dense markdown should prefer:
  - parsed IR caching
  - async parse for large bodies
  - single text layouts with styled runs
  - single highlighted text layouts for fenced code blocks
  - cached inline/text-run transforms for repeated rich renders

The durable rule is simple:

- use per-token element composition for small UI chrome
- use run-based text layout for large document-like content
