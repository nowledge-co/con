# con: The Native Terminal Emulator with a Built-in AI Harness

## Vision

An open-source, cross-platform, GPU-accelerated terminal emulator that treats AI agents as first-class citizens. Think Warp's UX ambition meets Ghostty's terminal correctness meets a native agent harness — all in Rust.

**Why con exists:**
- Warp is closed-source and macOS-first
- Existing terminals bolt AI on as an afterthought
- Agent workflows (Claude Code, Codex, ssh, tmux) deserve deep terminal integration, not wrapper hacks
- The terminal is the last IDE-free surface that hasn't been reinvented for the AI era

---

## Architecture Overview

```
┌─────────────────────────────────────────────────────────┐
│                     con (binary)                        │
│                                                         │
│  ┌─────────────┐  ┌──────────────┐  ┌───────────────┐  │
│  │  GPUI Shell  │  │  Agent Panel │  │  Command Bar  │  │
│  │  (windows,   │  │  (chat, tool │  │  (palette,    │  │
│  │   tabs,      │  │   calls,     │  │   search,     │  │
│  │   splits)    │  │   context)   │  │   actions)    │  │
│  └──────┬───────┘  └──────┬───────┘  └──────┬────────┘  │
│         │                 │                  │           │
│  ┌──────┴─────────────────┴──────────────────┴────────┐  │
│  │                    con-core                        │  │
│  │  ┌────────────┐ ┌────────────┐ ┌────────────────┐  │  │
│  │  │ Terminal    │ │ Agent      │ │ Session        │  │  │
│  │  │ Manager    │ │ Harness    │ │ Manager        │  │  │
│  │  └─────┬──────┘ └─────┬──────┘ └────────────────┘  │  │
│  └────────┼──────────────┼────────────────────────────┘  │
│           │              │                               │
│  ┌────────┴────────┐  ┌──┴──────────────┐               │
│  │ con-terminal    │  │ con-agent       │               │
│  │ (vte parser     │  │ (rig 0.32,      │               │
│  │  + PTY)         │  │  multi-provider)     │               │
│  └─────────────────┘  └─────────────────┘               │
└─────────────────────────────────────────────────────────┘
```

---

## Crate Structure

```
kingston/
├── Cargo.toml                 # workspace root
├── crates/
│   ├── con/                   # main binary — GPUI app shell
│   │   └── src/
│   │       ├── main.rs         # Application bootstrap + keybindings
│   │       ├── workspace.rs    # window, tabs, splits layout
│   │       ├── terminal_view.rs # GPUI canvas rendering of terminal grid
│   │       ├── agent_panel.rs  # side panel for AI chat / tool output
│   │       ├── input_bar.rs    # smart input bar (NLP/shell/skill modes)
│   │       ├── settings_panel.rs # provider config UI (Cmd+,)
│   │       ├── sidebar.rs      # session sidebar
│   │       └── theme.rs        # Flexoki dark theme
│   │
│   ├── con-core/              # shared logic, no UI dependency
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── harness.rs     # orchestrates agent ↔ terminal
│   │       ├── session.rs     # persist/restore workspace state
│   │       └── config.rs      # con config (TOML)
│   │
│   ├── con-terminal/          # terminal emulation layer
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── pty.rs         # cross-platform PTY (portable-pty)
│   │       ├── grid.rs        # terminal grid + VTE Perform impl
│   │       └── input.rs       # keyboard → escape sequence encoding
│   │
│   ├── con-agent/             # AI agent harness (Rig 0.32)
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── provider.rs    # Multi-provider Rig Agent (13 providers)
│   │       ├── hook.rs        # ConHook — PromptHook lifecycle bridge
│   │       ├── tools.rs       # Rig Tool trait impls (shell, file, search)
│   │       ├── context.rs     # terminal context extraction for agent
│   │       ├── conversation.rs # conversation state + Rig Message conversion
│   │       └── skills.rs      # skill registry + AGENTS.md parser
│   │
│   └── con-cli/               # headless CLI + socket client (stub)
│       └── src/main.rs
│
├── postmortem/                # incident & integration postmortems
│
└── docs/
    ├── study/                 # technology study notes
    └── impl/                  # implementation notes
```

---

## Key Technical Decisions

### 1. UI Framework: GPUI

**Why GPUI over alternatives (Tauri, Iced, egui, Slint):**

| Criteria | GPUI | Tauri | Iced | egui |
|----------|------|-------|------|------|
| GPU rendering | Metal + Blade (OpenGL) | WebView | wgpu | wgpu |
| Text quality | Production (Zed-grade) | Web fonts | Good | Basic |
| Cross-platform | macOS/Linux/Windows | All | All | All |
| Maturity | Zed production + 23+ apps | Very mature | Growing | Mature |
| Terminal canvas | canvas() API | DOM hacks | Custom widget | Custom paint |
| Rust-native | Yes | Hybrid JS/Rust | Yes | Yes |

GPUI gives us Zed-level text rendering quality (critical for a terminal) and a proven canvas API for custom grid drawing. The `termy` project in awesome-gpui proves terminal embedding works.

### 2. Terminal Core: vte + portable-pty

**Current approach (v0.1):**
- **vte** (v0.15) — pure Rust VT100 parser, implements `Perform` trait for dispatch
- Our `Grid` struct implements `vte::Perform` directly — handles print, CSI dispatch, SGR, OSC, ESC, alternate screen, scroll regions, cursor shapes
- **portable-pty** — cross-platform PTY management (macOS/Linux/Windows)
- 256-color + truecolor rendering, scrollback buffer

**Future (post-MVP):** Evaluate migrating to libghostty-vt for production-grade VT compliance (Kitty keyboard protocol, hyperlinks, full sixel support). The vte-based approach is sufficient for the current milestone and avoids the Zig build dependency.

### 3. AI Agent Harness: Rig

**Why [Rig](https://rig.rs/) over alternatives:**

| Option | Verdict |
|--------|---------|
| **Rig** | Most mature Rust AI agent framework. Multi-provider (OpenAI, Anthropic, Cohere, etc.). Tool-use, RAG, agent abstractions. 24.3% CPU — most efficient in benchmarks. |
| **rust-genai** | Multi-provider client but no agent abstractions (tools, chains). Just an API client. |
| **Embed Bun + ai-sdk** | Not feasible — Bun has no embedding API. Would need to spawn a subprocess. |
| **AutoAgents** | Multi-agent focused, heavier weight. Good for orchestration but overkill for terminal agent. |
| **Raw HTTP** | Maximum control but months of provider-specific work. |

**Rig v0.32 gives us:**
- `CompletionClient::agent()` builder — preamble + tools + model config in one chain
- `Tool` trait — type-safe tool definitions with `Args` (Deserialize), `Output` (Serialize), `Error`
- `Chat` trait — multi-turn conversation with `Vec<Message>` history
- `PromptHook` trait — lifecycle callbacks (on_text_delta, on_tool_call, on_tool_result) for streaming UI
- `anthropic::Client::new(api_key)` — direct Anthropic provider with model constants (`CLAUDE_4_SONNET`)
- Agent loop with automatic tool calling (up to N turns)

**Current integration:** con-agent implements 4 tools as Rig `Tool` impls (shell_exec, file_read, file_write, search), supports 13 providers (Anthropic, OpenAI, OpenAI-compatible, DeepSeek, Groq, Gemini, Ollama, OpenRouter, Mistral, Together, Cohere, Perplexity, xAI) with custom base_url for proxy endpoints. The harness runs on a shared tokio runtime. Provider settings are configurable via Cmd+, settings panel.

**Agent lifecycle (PromptHook):** Each agent request uses `agent.prompt().with_hook(hook).with_history(history)` instead of `agent.chat()`. The `ConHook` struct implements Rig's `PromptHook<M>` trait, emitting `AgentEvent`s for every tool call start/result and text delta. For dangerous tools (`shell_exec`, `file_write`), the hook blocks on a per-request approval channel — the UI must explicitly allow or deny execution before the agent proceeds. Safe tools (`file_read`, `search`) execute immediately. A 5-minute timeout prevents indefinite hangs if the UI becomes unresponsive.

**Per-request approval channels:** Each `send_message()` creates a fresh crossbeam channel pair. The sender is delivered to the UI via `ToolApprovalNeeded` events. The receiver is owned by the `ConHook` for that request. This eliminates race conditions between concurrent agent requests — only one hook instance reads from each channel.

**Conversation round-trip:** The conversation state (`Arc<Mutex<Conversation>>`) is shared between the main thread and spawned tasks. After the agent completes, the assistant message is added back to the conversation, enabling multi-turn context.

**Escape hatch:** If we later need ai-sdk features, we can spawn a Bun/Node sidecar process and communicate over IPC. This is a pragmatic fallback, not a primary architecture.

### 4. Cross-Platform Strategy

| Platform | GPU | Font | PTY | Window |
|----------|-----|------|-----|--------|
| macOS | Metal (GPUI native) | Core Text | posix_openpt | GPUI/AppKit |
| Linux | Blade/OpenGL (GPUI native) | fontconfig + freetype | openpty | GPUI/X11 or Wayland |
| Windows | Blade/D3D (GPUI native) | DirectWrite | ConPTY | GPUI/Win32 |

GPUI handles all three platforms. `portable-pty` handles PTY differences. libghostty-vt is pure computation (no platform deps).

---

## Core Features (MVP → v0.1)

### Phase 1: Terminal That Works
- [ ] GPUI window with single terminal pane
- [ ] libghostty-vt parsing → grid state → GPUI canvas render loop
- [ ] Keyboard input → PTY write (including Kitty protocol)
- [ ] Mouse support (selection, scroll, click)
- [ ] 256-color + truecolor
- [ ] Ghostty config file compatibility (themes, fonts)
- [ ] Basic tabs (Cmd+T / Ctrl+T)
- [ ] Split panes (horizontal + vertical)

### Phase 2: Agent Harness
- [x] Side panel for AI chat (Cmd+L to toggle)
- [x] Terminal context injection (last N lines, current command, cwd)
- [x] Tool execution: agent can run shell commands in terminal
- [x] Multi-provider config (13 providers via Rig 0.32)
- [x] Settings panel (Cmd+,) with provider selector and model config
- [x] Smart input bar (shell/agent/smart modes)
- [ ] Agent notification system (blue ring on tab when agent needs attention)

### Phase 3: Agent Lifecycle & Tool Transparency
- [x] PromptHook integration — tool calls visible in agent panel
- [x] Tool danger classification (safe: file_read, search; dangerous: shell_exec, file_write)
- [x] Per-request approval channels (no cross-request interference)
- [x] Approval timeout (5 minutes, denies on expiry)
- [x] Conversation round-trip (assistant messages added back for multi-turn)
- [x] Agent status indicators (Idle/Thinking/Responding)
- [x] Tool call rendering (in-progress dots, completed results)
- [x] Approval dialog UI (inline approval cards with Allow/Deny)
- [x] Streaming via `stream_prompt()` with real-time token rendering

### Phase 4: Agent Chat Polish
- [x] Structured tool call cards (icon, name, formatted args, result)
- [x] Inline approval cards for dangerous tools (Allow/Deny buttons)
- [x] Scrollable agent panel
- [x] Collapsible step timeline (click to expand/collapse)
- [x] Auto-approve toggle in settings UI (switch with live config propagation)
- [x] Streaming text rendering via `stream_prompt()`

### Phase 5: Deep Integration
- [x] OSC 133 command block tracking (prompt/command/exit code detection)
- [x] Command palette (Cmd+Shift+P) with fuzzy search and keyboard nav
- [ ] Inline AI suggestions (ghost text below prompt, Tab to accept)
- [ ] Command block actions: copy, re-run, explain, share
- [ ] SSH-aware agent (knows when you're in a remote session)
- [ ] tmux-aware agent (understands pane topology)
- [ ] Conversation history + search

### Phase 6: Polish
- [ ] Session persistence and restore
- [ ] Configurable keybindings
- [ ] Plugin system (Lua or WASM)
- [ ] Auto-update (Sparkle on macOS, appimage on Linux)
- [ ] CLI tool (`con` command for scripting)

---

## Agent ↔ Terminal Integration Design

This is what makes con different from "terminal + chatbot sidebar."

### Terminal Context Awareness

```
Terminal Output
     │
     ▼
┌─────────────┐     ┌──────────────┐
│ OSC 133     │────▶│ Command      │
│ Detection   │     │ Block Parser │
└─────────────┘     └──────┬───────┘
                           │
                    ┌──────▼───────┐
                    │ Context      │
                    │ Extractor    │
                    │ - cwd        │
                    │ - last cmd   │
                    │ - exit code  │
                    │ - output     │
                    │ - git branch │
                    └──────┬───────┘
                           │
                    ┌──────▼───────┐
                    │ Agent        │
                    │ Harness      │
                    │ (Rig)        │
                    └──────────────┘
```

The agent always knows:
- What directory the user is in
- What command they just ran and its output
- Whether they're in SSH/tmux/docker
- Git status if in a repo

### Agent Tool Execution

When the agent needs to run a command:
1. Agent produces a `ShellExec` tool call via Rig
2. con-core creates a new (or reuses) terminal pane
3. Command is written to PTY
4. Output is captured via grid state + OSC 133 boundaries
5. Result is fed back to agent

This means the user **sees** what the agent does — no hidden subprocess. Full transparency.

### Compatibility with Existing Agents

For tools like Claude Code, Codex CLI, or OpenCode that run *inside* the terminal:
- con detects these agents via process name / OSC sequences
- Provides enhanced UX: notification rings, focus management
- Does NOT try to "take over" — con is the host, not the agent
- Optional: pipe agent's stderr/notifications to con's notification system

---

## Build & Development

### Prerequisites
```bash
# macOS
brew install rustup cmake

# Linux
sudo apt install rustup cmake libwayland-dev libxkbcommon-dev

# Windows
# Install rustup, cmake via winget/scoop
```

### Build
```bash
cargo build                    # debug build
cargo build --release          # release build
cargo run -p con               # run the terminal
cargo test --workspace         # test everything
```

### Build Pipeline
1. Cargo resolves workspace deps from crates.io (gpui-ce 0.3, rig-core 0.32)
2. GPUI-CE compiles Metal shaders at runtime (`runtime_shaders` feature — no Xcode.app needed for dev)
3. `cargo build` produces the `con` binary with all crates linked

---

## Config

```toml
# ~/.config/con/config.toml

[terminal]
font-family = "JetBrains Mono"
font-size = 14
theme = "catppuccin-mocha"        # or any ghostty theme
scrollback-lines = 10000
cursor-style = "block"

[agent]
provider = "anthropic"             # anthropic, openai, openai-compatible, deepseek,
                                   # groq, gemini, ollama, openrouter, mistral,
                                   # together, cohere, perplexity, xai
model = "claude-sonnet-4-0"        # leave empty for provider default
api_key_env = "ANTHROPIC_API_KEY"  # reads from env var
base_url = ""                      # optional: custom/proxy endpoint
max_tokens = 4096
max_turns = 10
auto_context = true                # inject terminal context automatically
auto_approve_tools = false         # require approval for shell_exec, file_write

[keybindings]
toggle-agent = "cmd+l"
command-palette = "cmd+shift+p"
new-tab = "cmd+t"
split-right = "cmd+d"
split-down = "cmd+shift+d"
```

---

## Why This Stack Wins

1. **Ghostty's terminal correctness** — years of VT compat work, free
2. **GPUI's rendering quality** — Zed proves it handles text beautifully at scale
3. **Rig's agent abstractions** — swap providers, define tools in Rust, stream tokens
4. **Rust's performance** — sub-millisecond input latency, <100MB RAM
5. **Cross-platform from day 1** — GPUI + portable-pty + libghostty-vt all support macOS/Linux/Windows
6. **Open source** — fill the gap Warp left

---

## Resolved Decisions

### 1. Terminal Parsing: vte (Pure Rust)

**Decision:** Use `vte` crate (v0.15) for VT parsing instead of libghostty-vt FFI.

**Rationale:**
- Zero build complexity — no Zig toolchain, no FFI, no bindgen
- Pure Rust `Perform` trait maps cleanly to our Grid implementation
- Sufficient for MVP: handles all common VT100/xterm sequences
- Future migration to libghostty-vt remains possible if we need Kitty keyboard protocol or sixel graphics

### 2. GPUI IME: Production-Ready

GPUI-CE implements the full `InputHandler` trait (modeled after `NSTextInputClient`):
- `marked_text_range()` / `replace_and_mark_text_in_range()` for IME composition
- `bounds_for_range()` for candidate window positioning
- Platform implementations: macOS (AppKit), Linux X11 (xim crate), Windows (WM_IME_*)
- CJK input works. No blocker.

### 3. GPU Fallback: Not Needed

Ghostty has no software renderer — GPU-only. Same for con.

**Rationale:** A desktop terminal emulator always has a GPU. The scenarios where it doesn't:
- **Headless servers** — users SSH in, they use the remote machine's terminal, not con
- **X11 forwarding** — OpenGL forwarding works via Blade; this is the Linux path already
- **Containers** — con doesn't run inside Docker; it runs on the host

If we ever need headless testing, we add a test-only software rasterizer. Not a user feature.

### 4. Plugin System: Node.js + Python via Sidecar IPC

**Decision:** Plugins run as external processes. con communicates via JSON-RPC over Unix domain sockets (like cmux's socket API).

**Why Node.js + Python:**
- Largest developer ecosystems — lowest friction for plugin authors
- Runtimes installed by the user (system Node/Python, nvm, pyenv — their choice)
- No embedded runtime = no binary bloat, no version conflicts
- Security: plugins run in separate processes with explicit capability grants

**How it works:**
```
con (Rust)  ─── Unix socket (JSON-RPC) ───  plugin process (Node/Python/any)
```

- con exposes a socket API (inspired by cmux V2): `notification.create`, `terminal.write`, `context.get`, etc.
- Plugin manifest declares: name, runtime, entry point, requested capabilities
- con spawns the plugin process, passes socket path via env var
- Plugin SDK: thin npm package (`@con/sdk`) and pip package (`con-sdk`) wrapping the JSON-RPC protocol

**Phase 4 deliverable.** Socket API comes free from the cmux-inspired external agent support in Phase 2.

### 5. Licensing: MIT

- **GPUI-CE**: Apache 2.0 (compatible with MIT, allows sublicensing)
- **Ghostty libghostty-vt**: MIT
- **Rig**: MIT
- **portable-pty**: MIT
- **con**: MIT (already in LICENSE)

All clear. No copyleft, no GPL contamination.

---

## Key Insight from cmux

cmux doesn't embed an LLM. It provides a **socket control API** that external agents (Claude Code, Codex) use to interact with the terminal. This is the right pattern.

**con should have both:**
1. **Built-in agent** (via Rig) — for users who want native AI without installing anything else
2. **Socket API** (cmux-inspired) — for external agents and plugins to control con

The socket API is the foundation. The built-in agent is just the first client of that API.
