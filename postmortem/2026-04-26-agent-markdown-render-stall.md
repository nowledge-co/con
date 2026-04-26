# Agent Markdown Render Stall

## What Happened

Restored agent conversations with long markdown replies could make the whole app feel stale. The local repro was a 114-line assistant response with a mix of prose, tables, code fences, and tool traces. Expanding the hidden tail or typing in the agent panel caused visible jank even though the content size was modest for a Rust UI.

## Root Cause

The assistant reply was rendered as one large GPUI list item. Parsed markdown was cached, but rendered markdown blocks were anonymous element trees created by the assistant row on each invalidation. That meant a small interaction could still rebuild and relayout many `StyledText` trees inside the same visible row.

This was a cache-bound UI architecture problem, not a raw markdown parsing problem. The expensive part was repeatedly recreating layout-owning elements for content that had already been rendered.

## Fix Applied

- Added `ChatMarkdownBlockView`, a stable GPUI entity for one parsed markdown block.
- Changed assistant message rendering to reuse one child entity per visible markdown block.
- Kept block entities scoped to the parsed markdown generation, so streaming or content replacement resets stale block views safely.
- Kept markdown parsing and syntax runs cached, and added a regression test that highlighted code runs retain the embedded mono font family.

## What We Learned

Long transcript rendering must be cache-aware at the same granularity the user interacts with. A whole assistant message is too coarse: it mixes header chrome, markdown blocks, tables, code, tool traces, and reveal controls into a single invalidation island.

The next durable step is full transcript-row virtualization: model assistant content and tool trace sections as independent list rows rather than one message-sized list row. Block-level entity caching fixes repeated relayout of stable markdown blocks, but the list architecture should eventually make the outer transcript itself block-aware.
