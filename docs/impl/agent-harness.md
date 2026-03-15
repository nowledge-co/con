# Agent Harness — Implementation Notes

## Overview

The agent harness orchestrates the AI agent lifecycle: user input → model call → tool execution → response delivery. It bridges three crates:

- **con-agent** — Rig integration, tools, hooks, conversation state
- **con-core** — Harness orchestration, event routing, config
- **con** — GPUI workspace, agent panel rendering, approval UI

## Architecture

```
User Input
    │
    ▼
ConWorkspace::on_input_submit()
    │
    ├── Adds user message to AgentPanel
    ├── Snapshots terminal grid → TerminalContext
    └── Calls AgentHarness::send_message(content, context)
            │
            ▼
        AgentHarness (con-core)
            │
            ├── Adds user message to Arc<Mutex<Conversation>>
            ├── Creates per-request approval channel
            └── Spawns tokio task:
                    │
                    ├── Spawns bridge thread (AgentEvent → HarnessEvent)
                    ├── Snapshots conversation (brief lock)
                    ├── Creates AgentProvider
                    └── Calls provider.send(conv, ctx, event_tx, approval_rx)
                            │
                            ▼
                        AgentProvider (con-agent)
                            │
                            ├── Builds Rig Agent with 4 tools
                            ├── Creates ConHook with event_tx + approval_rx
                            └── agent.prompt(msg).with_hook(hook).with_history(history)
                                    │
                                    ▼
                                Rig Agent Loop (rig-core)
                                    │
                                    ├── Calls model API
                                    ├── on_tool_call → ConHook emits ToolCallStart
                                    │                  (blocks for approval if dangerous)
                                    ├── Executes tool
                                    ├── on_tool_result → ConHook emits ToolCallComplete
                                    └── Returns response text
                            │
                            ▼
                        Provider adds assistant message to conversation
                        Bridge emits HarnessEvent::ResponseComplete
                            │
                            ▼
                        ConWorkspace polls events_rx
                        Updates AgentPanel UI
```

## Channel Architecture

### Event channel (Harness → UI)
- `crossbeam::unbounded::<HarnessEvent>()` — created once at harness init
- Polled by GPUI workspace every 50ms via `cx.spawn(async move |this, cx| { loop { ... } })`
- All events flow through this single channel

### Per-request approval channel (UI → Hook)
- Fresh `crossbeam::unbounded::<ToolApprovalDecision>()` per `send_message()`
- Sender delivered to UI inside `HarnessEvent::ToolApprovalNeeded`
- Receiver owned by `ConHook` for that request
- Hook blocks with 5-minute timeout on `recv_timeout()`

### Bridge thread (AgentEvent → HarnessEvent)
- `tokio::task::spawn_blocking` — maps `AgentEvent` variants to `HarnessEvent`
- Intercepts `ToolCallStart` for dangerous tools → emits `ToolApprovalNeeded`
- Terminates when `agent_tx` is dropped (provider.send returns)

## Tool Danger Classification

| Tool | Classification | Behavior |
|------|---------------|----------|
| `file_read` | Safe | Executes immediately |
| `search` | Safe | Executes immediately |
| `shell_exec` | Dangerous | Requires approval (or auto_approve) |
| `file_write` | Dangerous | Requires approval (or auto_approve) |

Classification happens in two places:
1. `ConHook::on_tool_call` — blocks on approval for dangerous tools
2. Bridge thread in harness — emits `ToolApprovalNeeded` event for dangerous tools

## Conversation State

`Arc<Mutex<Conversation>>` shared between main thread and spawned tasks.

The mutex is held briefly for two operations:
1. **Snapshot** — clone conversation before agent call
2. **Add message** — insert assistant response after agent completes

Rig manages its own history via `with_history(&mut Vec<rig::message::Message>)`. Our `to_rig_history()` provides User/Assistant text pairs. Rig appends tool call/result messages during its turn loop.

## Streaming Status

The `on_text_delta` hook is implemented but **only fires when using `stream_prompt()`**. The current non-streaming path (`prompt()`) does not trigger it. The UI handles `Token` events correctly — when streaming is added, the agent panel will render incremental text without changes.

## Config

```toml
[agent]
auto_approve_tools = false  # skip approval for dangerous tools
max_turns = 10              # max tool-use turns per request
```

`auto_approve_tools = true` makes `ConHook` return `ToolCallHookAction::cont()` for all tools, bypassing the approval channel entirely.
