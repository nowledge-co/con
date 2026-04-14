## What happened

Con felt noticeably less responsive when the agent panel was visible and contained rendered messages. The slowdown was easy to feel during pane splits, new-tab creation, and other workspace actions, but disappeared when the agent panel was hidden or visible with no messages.

## Root cause

The hot path was in the agent panel render loop, not Ghostty.

Assistant message bodies and thinking blocks were rendered through `chat_markdown.rs`, and that renderer reparsed markdown into mdast on every render. Because the agent panel stays visible while the workspace changes, those reparses happened repeatedly for the same static messages.

That meant message-heavy panels turned ordinary workspace renders into repeated markdown parsing work.

## Fix applied

- Added `ParsedChatMarkdown` in `chat_markdown.rs` so parsed markdown can be retained and reused.
- Cached parsed message content and parsed thinking blocks on `PanelMessage` in `agent_panel.rs`.
- Invalidated the cache only when streamed content or thinking text actually changed.
- Switched the render loop to reuse the parsed representation instead of reparsing source strings every frame.

## What we learned

- Performance regressions in Con often present as “terminal lag” even when the terminal runtime is not the bottleneck.
- For visible side panels, repeated parse/format work must be treated as render-path work and cached at the model layer.
- Ghostty and GPUI are only part of the frame budget; expensive panel rendering can dominate interaction feel even when terminal rendering itself is healthy.
