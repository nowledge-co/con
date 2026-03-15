# 2026-03-15: Approval channel redesign — per-request isolation

## What happened

During the Phase 3 deep review, we discovered two critical issues in the initial tool approval implementation:

1. **Shared approval channel**: A single `approval_rx` was cloned across all agent requests. The `on_tool_call` hook looped over received messages matching by `call_id`, but in concurrent requests, one hook could consume another's approval decision — silently dropping it.

2. **No approval timeout**: If the UI crashed or the user never responded, `approval_rx.recv()` would block forever, hanging the agent task and the tokio worker thread it ran on.

3. **Misleading streaming state**: `AgentStatus::Streaming` and `complete_streaming()` implied active text streaming, but the non-streaming path (`prompt()`) never calls `on_text_delta`. Only `stream_prompt()` does — and we don't use that yet.

## Root causes

1. **Shared mutable state**: The original design shared one channel pair across the harness lifetime. This works if requests are strictly sequential, but the architecture allows concurrent `send_message()` calls (spawned as independent tokio tasks).

2. **Missing timeout**: The approval flow was modeled as synchronous UI interaction but lacked a safety net for when the UI half goes away.

3. **Incomplete Rig source analysis**: We assumed `on_text_delta` fired in all paths. Reading Rig's `prompt_request/mod.rs` (lines 297-601) revealed the `prompt()` path only calls `on_tool_call` and `on_tool_result`, never `on_text_delta`.

## Fixes applied

### Per-request approval channels
Each `send_message()` now creates a fresh `crossbeam::unbounded()` channel pair. The sender is delivered to the UI inside `HarnessEvent::ToolApprovalNeeded`. The receiver is owned by the `ConHook` instance for that request. No sharing, no races.

### 5-minute approval timeout
`ConHook::on_tool_call` uses `approval_rx.recv_timeout(Duration::from_secs(300))`. On timeout, the tool call is denied with "Tool approval timed out — denied for safety".

### Accurate naming
`AgentStatus::Streaming` renamed to `AgentStatus::Responding`. `complete_streaming()` renamed to `complete_response()`. Comments document that real streaming via `stream_prompt()` is a future phase.

### Safe string truncation
`truncate_str()` now checks `is_char_boundary()` before slicing, preventing panics on multi-byte UTF-8 content in tool call args/results.

## What we learned

- Per-request channels are the right pattern when tasks are independently spawned. Shared channels require coordination (locks, ID matching) that adds complexity without benefit.
- Always add timeouts to blocking operations that depend on another system (UI, network, user). The cost of a timeout (one constant, one match arm) is negligible compared to the cost of a hang.
- Read the framework source before assuming callback behavior. Rig's streaming and non-streaming paths have different hook coverage — this is by design, not a bug.
