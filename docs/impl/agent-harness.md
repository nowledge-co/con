# Agent Harness — Implementation Notes

## Overview

The agent harness orchestrates the AI agent lifecycle: user input → model call → tool execution → response delivery. It bridges three crates:

- **con-agent** — Rig integration, tools, hooks, conversation state
- **con-core** — Harness orchestration, event routing, config
- **con** — GPUI workspace, agent panel rendering, approval UI

## Architecture

The harness is split into two concerns: **shared infrastructure** (one per window) and **per-tab sessions** (one per tab).

```
Window (ConWorkspace)
  │
  ├─ AgentHarness (shared)
  │   ├── config: AgentConfig       ← provider, model, auth
  │   ├── skills: SkillRegistry     ← built-in + AGENTS.md
  │   └── runtime: Arc<Runtime>     ← tokio, 2 worker threads
  │
  └─ tabs: Vec<Tab>
      └── Tab
          ├── pane_tree: PaneTree
          ├── session: AgentSession         ← per-tab
          │   ├── conversation: Arc<Mutex<Conversation>>
          │   ├── event_tx / event_rx       ← HarnessEvent channel
          │   ├── terminal_exec_tx / rx     ← TerminalExecRequest channel
          │   ├── pane_tx / pane_rx         ← PaneRequest channel
          │   └── cancel_flag: Arc<AtomicBool>
          └── panel_state: PanelState       ← per-tab UI snapshot
              ├── messages: Vec<PanelMessage>
              ├── tool_calls: Vec<ToolCallEntry>
              ├── pending_approvals: Vec<PendingApproval>
              ├── streaming: bool
              └── status: AgentStatus
```

**Why this split:**
- Runtime and config are expensive to duplicate; skills are project-level, not tab-level.
- Conversations must be isolated: Tab 1's agent shouldn't see Tab 2's history.
- Terminal exec requests must route to the originating tab's pane tree, not the active tab.
- Background tabs keep running — switching away doesn't interrupt the agent.

## Request Flow

```
User Input (Tab N)
    │
    ▼
ConWorkspace::send_to_agent()
    │
    ├── Adds user message to AgentPanel
    ├── Snapshots Ghostty pane state → TerminalContext
    └── Calls harness.send_message(&tabs[N].session, content, context)
            │
            ▼
        AgentHarness::send_message(session, ...)
            │
            ├── Adds user message to session.conversation
            ├── Resets session.cancel_flag
            ├── Creates per-request approval channel
            └── Spawns tokio task on shared runtime:
                    │
                    ├── Spawns bridge thread (AgentEvent → HarnessEvent)
                    ├── Snapshots conversation (brief mutex lock)
                    ├── Creates AgentProvider from config
                    └── Calls provider.send(conv, ctx, event_tx, approval_rx)
                            │
                            ▼
                        AgentProvider (con-agent)
                            │
                            ├── Builds Rig Agent with tools
                            ├── Creates ConHook with event_tx + approval_rx
                            └── agent.prompt(msg).with_hook(hook).with_history(history)
                                    │
                                    ▼
                                Rig Agent Loop
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
```

## Event Polling & Tab Routing

The workspace polls **all** tabs' sessions every 4ms:

```
for tab_idx in 0..tabs.len() {
    // Agent events
    while let Ok(event) = tabs[tab_idx].session.events().try_recv() {
        if tab_idx == active_tab {
            → AgentPanel (with cx.notify → re-render)
        } else {
            → tabs[tab_idx].panel_state.apply_event(event)
            → tabs[tab_idx].needs_attention = true
        }
    }

    // Terminal exec — always routed to the originating tab
    while let Ok(req) = tabs[tab_idx].session.terminal_exec_requests().try_recv() {
        → handle_terminal_exec_request_for_tab(tab_idx, req)
    }

    // Pane queries — always routed to the originating tab
    while let Ok(req) = tabs[tab_idx].session.pane_requests().try_recv() {
        → handle_pane_request_for_tab(tab_idx, req)
    }
}
```

Key properties:
- Active tab events update the AgentPanel directly (live rendering).
- Background tab events are applied to `PanelState` (pure data, no GPUI dependency).
- Terminal exec routes to the **originating** tab, not the active tab.
- The `needs_attention` flag drives a blue dot indicator on the tab.

## Tab Switching (PanelState Swap)

When the user switches from Tab A to Tab B:

```rust
// 1. Take Tab B's cached state
let incoming = mem::replace(&mut tabs[B].panel_state, PanelState::new());

// 2. Swap into AgentPanel, get Tab A's live state back
let outgoing = agent_panel.swap_state(incoming);

// 3. Stash Tab A's state
tabs[A].panel_state = outgoing;
```

This avoids cloning — `mem::replace` moves ownership. The AgentPanel always displays exactly one tab's state.

## Channel Architecture

### Per-tab event channels (Session → UI)
- `crossbeam::unbounded::<HarnessEvent>()` — one per AgentSession
- Created when a tab is opened or restored
- Polled by the workspace's event loop

### Per-request approval channel (UI → Hook)
- Fresh `crossbeam::unbounded::<ToolApprovalDecision>()` per `send_message()` call
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
| `list_panes` | Safe | Executes immediately |
| `read_pane` | Safe | Executes immediately |
| `search_panes` | Safe | Executes immediately |
| `terminal_exec` | Dangerous | Requires approval (or auto_approve) |
| `shell_exec` | Dangerous | Requires approval (or auto_approve) |
| `file_write` | Dangerous | Requires approval (or auto_approve) |
| `edit_file` | Dangerous | Requires approval (or auto_approve) |
| `send_keys` | Dangerous | Requires approval (or auto_approve) |

Classification happens in two places:
1. `ConHook::on_tool_call` — blocks on approval for dangerous tools
2. Bridge thread in harness — emits `ToolApprovalNeeded` event for dangerous tools

## Conversation State

`Arc<Mutex<Conversation>>` per tab, shared between main thread and spawned tasks.

The mutex is held briefly for two operations:
1. **Snapshot** — clone conversation before agent call
2. **Add message** — insert assistant response after agent completes

Rig manages its own history via `with_history(&mut Vec<rig::message::Message>)`. Our `to_rig_history()` provides User/Assistant text pairs. Rig appends tool call/result messages during its turn loop.

## Session Persistence

Each tab saves its own `conversation_id` to `session.json`:

```json
{
  "tabs": [
    { "title": "Terminal 1", "conversation_id": "abc-123", ... },
    { "title": "Terminal 2", "conversation_id": "def-456", ... }
  ],
  "conversation_id": null
}
```

On restore, each tab loads its own conversation. For backward compatibility, if a tab has no `conversation_id` but the session-level field exists, the first tab inherits it.

## Config

```toml
[agent]
auto_approve_tools = false  # skip approval for dangerous tools
max_turns = 10              # max tool-use turns per request
```

`auto_approve_tools = true` makes `ConHook` return `ToolCallHookAction::cont()` for all tools, bypassing the approval channel entirely.

## Input Classification

`classify_input(input, is_remote)` determines whether user input is a skill invocation, a shell command, or a natural language query. The `is_remote` flag is derived from the focused pane's `detected_remote_host()`.

1. **Skill** — starts with `/` and matches a registered skill name
2. **Shell command** — structural analysis (see below)
3. **Natural language** — everything else → sent to the agent

### Command detection (`looks_like_command`)

Uses structural signals — no static word list:

| Signal | Example | How |
|--------|---------|-----|
| Shell builtin | `cd`, `export`, `alias` | `SHELL_BUILTINS` constant (POSIX + bash/zsh) |
| PATH executable | `hostname`, `git`, `docker` | `$PATH` scanned once, cached in `OnceLock<HashSet>` |
| Path invocation | `./script.sh`, `/usr/bin/env` | Prefix: `./`, `/`, `~/` |
| Env var assignment | `LANG=C sort file` | `VAR=value` pattern (uppercase name + `=`) |
| Shell operators | `cat foo \| grep bar` | ` \| `, ` > `, ` >> `, ` && `, ` ; ` |
| Subshell syntax | `echo $(date)` | `$(` or backticks |
| Flag arguments | `free -g`, `docker --version` | Any token starts with `-` + command-shaped first word |
| Remote commands | `systemctl status` (via SSH) | When `is_remote=true`, command-shaped first word is accepted unless NL signals are present |

### Remote-aware classification

When the focused pane is an SSH session, remote executables aren't on the local `$PATH`. The classifier accepts any command-shaped first word (lowercase, alphanumeric, hyphens) as a shell command — unless natural-language signals are detected (question words, articles, pronouns). This covers remote commands like `free`, `systemctl`, `apt` without maintaining a word list.

### Multi-pane agent context

The system prompt is built from a live pane snapshot, not process-wide environment variables. For the focused pane we derive host, title, pane mode (`shell`, `multiplexer`, `tui`, `unknown`), and whether shell metadata is fresh enough to trust for the visible app.

When multiple panes are open, the system prompt includes a `<panes>` block listing every pane with its index, hostname, cwd, mode, and shell-metadata freshness. This lets the agent target the right pane(s) immediately — using `terminal_exec` with `pane_index` or `batch_exec` for parallel execution — without needing to call `list_panes` first.

This matters for SSH, tmux, and full-screen TUIs:

- `ssh_host` comes from pane-local evidence only. con prefers a detected host from the pane itself and now falls back to title/screen-structure hints when OSC 7 does not carry a usable remote hostname.
- `tmux_session` is inferred from the pane itself (command/title/screen hints), not from `TMUX` in the parent process.
- When remote identity is unknown, the prompt says `unknown`, not `local`.
- When the pane mode is not `shell`, or shell metadata is stale, the prompt explicitly tells the model to inspect the live pane with `list_panes`, `read_pane`, and `send_keys` before making claims about cwd, hostname, or the running app.

This is still a transitional architecture. The next layer is a dedicated pane runtime observer that keeps evidence and models nested scopes such as `ssh -> tmux -> shell -> Codex CLI`. See `docs/impl/pane-runtime-observer.md`.
