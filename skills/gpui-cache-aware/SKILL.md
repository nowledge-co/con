# GPUI Cache-Aware Review

Use this skill when reviewing or changing Con UI performance, especially when a view feels janky, expensive, or unexpectedly re-renders during unrelated updates.

## Goal

Keep Con visually rich without paying repeated GPUI layout/render cost for work that could be cached or isolated.

## Core Rule

Cache at the cheapest correct layer first.

Order of preference:

1. Cache parsed / normalized data.
2. Cache expensive text-run or highlight transforms.
3. Isolate expensive subtrees behind stable entity boundaries.
4. Use GPUI `AnyView::cached(...)` only when the subtree has a stable size contract.

Do not jump straight to view caching.

## Review Checklist

For any slow UI path, inspect these in order:

### 1. Parse / transform churn

- Is the code reparsing markdown / JSON / syntax / layout input on every render?
- Is the code rebuilding `SharedString`, `Vec<TextRun>`, highlighted runs, or table cell text on every render?
- Can the expensive transform be retained on the model object and invalidated only on real content change?

Preferred fix:

- add data-layer caches on parsed document/block/cell/message structs
- invalidate only when source text or theme-dependent key changes

### 2. Render-tree cardinality

- Is the renderer producing many tiny `div()` children for content that could be one `StyledText`?
- Are inline chips, per-token wrappers, nested flex rows, or deep container stacks used for long-form content?

Preferred fix:

- collapse long-form prose to text-first rendering
- keep decorative element composition for UI chrome and short content only

### 3. Entity boundaries

- Is a very large subtree being rebuilt because the parent panel rerendered for unrelated state?
- Can the subtree live in its own `Entity<V>` with a narrower invalidation surface?

Preferred fix:

- split large stable regions into their own render entities
- keep mutation paths explicit so only that entity gets `notify()`

### 4. GPUI cached view suitability

Use `AnyView::cached(...)` only if all of these are true:

- the subtree size is externally constrained or stable
- the cached style can describe the layout contract correctly
- reusing previous layout/paint is actually valid for the subtree

Good fits:

- panes that fill known bounds
- fixed-size or externally-sized tool panels
- stable canvases / editors / native-host surfaces

Bad fits:

- intrinsic-height rich text documents
- content whose height depends on wrapping and dynamic width unless that width/height contract is explicitly handled

If a cached view causes overlap, clipping, or stale layout, the cache boundary is wrong.

### 5. Lists and scrolling surfaces

- If there are many repeated items, consider virtualization before micro-optimizing item chrome.
- If the item count is small but each item is expensive, focus on item-level caches instead.

## Con-Specific Guidance

### Markdown and chat surfaces

- Prefer parsed markdown caches on the message/document model.
- Cache inline text-run generation for paragraphs, headings, and table cells.
- Cache syntax-highlight runs for code blocks.
- Avoid per-token flex trees for long replies.

### Terminal-adjacent UI

- Keep terminal surfaces and heavy side panels isolated.
- Avoid reading terminal runtime state during ordinary render unless already cached.
- Be cautious with transparency, animation, and resize interactions; measure before adding visual layers.

## Validation

After a cache-related change:

- verify `cargo check -p con`
- run targeted tests if the subsystem has them
- confirm there is no layout regression
- confirm the cache invalidates on real content/theme changes
- confirm the cache does not hide stale data

## Anti-Patterns

- caching a view because it "seems expensive" without proving its size contract is stable
- keeping old cached output after source text changed
- using collapsed/default-hidden UI as the main performance strategy
- replacing high-fidelity rendering with degraded output when a correct cache boundary exists

## Deliverable Standard

The final fix should preserve UX quality first, then reduce repeated work structurally.

If the only way a change feels fast is by degrading rendering fidelity, treat that as an incomplete fix and keep going.
