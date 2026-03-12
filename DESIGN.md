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
│  │ (libghostty-vt  │  │ (rig + provider │               │
│  │  FFI + PTY)     │  │  adapters)      │               │
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
│   │       ├── main.rs
│   │       ├── app.rs         # GPUI Application bootstrap
│   │       ├── workspace.rs   # window, tabs, splits layout
│   │       ├── terminal_view.rs  # GPUI canvas rendering of terminal grid
│   │       ├── agent_panel.rs # side panel for AI chat / tool output
│   │       ├── command_bar.rs # command palette + inline AI prompt
│   │       ├── theme.rs       # theming (Ghostty config compat)
│   │       └── keybindings.rs
│   │
│   ├── con-core/              # shared logic, no UI dependency
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── terminal_manager.rs  # owns terminal instances
│   │       ├── agent_harness.rs     # orchestrates agent ↔ terminal
│   │       ├── session.rs           # persist/restore workspace state
│   │       ├── config.rs            # con config + ghostty config reader
│   │       └── notification.rs      # agent notification system (OSC hooks)
│   │
│   ├── con-terminal/          # terminal emulation layer
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── pty.rs         # cross-platform PTY (portable-pty)
│   │       ├── vt.rs          # libghostty-vt FFI bindings
│   │       ├── grid.rs        # terminal grid state (wraps ghostty Screen)
│   │       ├── input.rs       # keyboard/mouse → escape sequence encoding
│   │       └── renderer.rs    # grid → GPUI paint commands
│   │
│   ├── con-agent/             # AI agent harness
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── provider.rs    # rig-based multi-provider LLM client
│   │       ├── tools.rs       # agent tool definitions (shell, file, search)
│   │       ├── context.rs     # terminal context extraction for agent
│   │       ├── conversation.rs # conversation state + history
│   │       └── streaming.rs   # SSE/streaming token output
│   │
│   └── con-cli/               # headless CLI + socket client
│       └── src/
│           ├── main.rs
│           └── socket.rs      # IPC to running con instance
│
├── 3pp/                       # third-party sources (submodules)
│   ├── ghostty/               # libghostty-vt source
│   ├── gpui-ce/               # GPUI community edition
│   ├── cmux/                  # reference implementation
│   ├── create-gpui-app/
│   └── awesome-gpui/
│
├── build/                     # build scripts
│   ├── ghostty.rs             # build.rs helper: compile libghostty-vt
│   └── link.rs                # platform-specific linking
│
└── assets/
    ├── themes/                # built-in color schemes
    ├── fonts/                 # bundled fallback font (e.g. JetBrains Mono)
    └── icons/
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

### 2. Terminal Core: libghostty-vt via C FFI

**Why not rewrite VT parsing from scratch:**
- Ghostty's parser is production-grade, tested against thousands of terminal programs
- VT100/xterm compatibility is a multi-year effort — leverage it
- libghostty-vt exposes a clean C API specifically for embedding
- Kitty keyboard protocol, OSC 133 (semantic prompts), hyperlinks — all included

**Integration approach:**
- Build libghostty-vt from Zig source in `build.rs` (Zig compiles to C ABI)
- Generate Rust bindings via `bindgen` against `ghostty/vt.h`
- Wrap in safe Rust in `con-terminal` crate
- Use `portable-pty` crate for cross-platform PTY management (avoids reimplementing PTY for each OS)

### 3. AI Agent Harness: Rig

**Why [Rig](https://rig.rs/) over alternatives:**

| Option | Verdict |
|--------|---------|
| **Rig** | Most mature Rust AI agent framework. Multi-provider (OpenAI, Anthropic, Cohere, etc.). Tool-use, RAG, agent abstractions. 24.3% CPU — most efficient in benchmarks. |
| **rust-genai** | Multi-provider client but no agent abstractions (tools, chains). Just an API client. |
| **Embed Bun + ai-sdk** | Not feasible — Bun has no embedding API. Would need to spawn a subprocess. |
| **AutoAgents** | Multi-agent focused, heavier weight. Good for orchestration but overkill for terminal agent. |
| **Raw HTTP** | Maximum control but months of provider-specific work. |

**Rig gives us:**
- Unified provider interface (swap OpenAI ↔ Anthropic with config)
- Tool definitions in Rust (type-safe, fast)
- Streaming completion support
- Agent loop with tool calling
- Good enough to start; replaceable later if needed

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
- [ ] Side panel for AI chat (Cmd+L to toggle)
- [ ] Terminal context injection (last N lines, current command, cwd)
- [ ] Streaming response rendering in panel
- [ ] Tool execution: agent can run shell commands in terminal
- [ ] Multi-provider config (OpenAI, Anthropic, local via Ollama)
- [ ] Agent notification system (blue ring on tab when agent needs attention)

### Phase 3: Deep Integration
- [ ] Inline AI suggestions (ghost text below prompt, Tab to accept)
- [ ] Smart command blocks (detect command boundaries via OSC 133)
- [ ] Command block actions: copy, re-run, explain, share
- [ ] SSH-aware agent (knows when you're in a remote session)
- [ ] tmux-aware agent (understands pane topology)
- [ ] Agent tool: read/write files, search codebase, git operations
- [ ] Conversation history + search

### Phase 4: Polish
- [ ] Command palette (Cmd+Shift+P)
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
brew install zig rustup cmake

# Linux
sudo apt install zig rustup cmake libwayland-dev libxkbcommon-dev

# Windows
# Install zig, rustup, cmake via winget/scoop
```

### Build
```bash
cargo build                    # debug build
cargo build --release          # release build
cargo run -p con               # run the terminal
cargo test --workspace         # test everything
```

### Build Pipeline
1. `build.rs` in `con-terminal` invokes `zig build` on ghostty source to produce `libghostty_vt.a`
2. `bindgen` generates Rust FFI bindings from `ghostty/vt.h`
3. Cargo links everything together
4. GPUI handles platform-specific GPU setup at runtime

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
provider = "anthropic"             # or "openai", "ollama", "custom"
model = "claude-sonnet-4-20250514"
api-key-env = "ANTHROPIC_API_KEY"  # reads from env var
auto-context = true                # inject terminal context automatically
notification = true                # blue ring when agent needs attention

[agent.tools]
shell = true                       # agent can execute commands
file = true                        # agent can read/write files
search = true                      # agent can search codebase

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

### 1. Build: Zig from Cargo build.rs

**Decision:** Invoke `zig build lib-vt` from `con-terminal/build.rs`. This is the right long-term approach.

**Why not pre-compiled artifacts:**
- Zig cross-compiles natively — `zig build lib-vt -Dtarget=x86_64-linux-gnu` works out of the box
- Keeps the build hermetic: one `cargo build` does everything
- Ghostty's build.zig already has a clean `lib-vt` target that produces `libghostty_vt.a` + headers
- Zig 0.15.2+ required (check in build.rs, fail with clear message)

**CI strategy:** Cache the compiled `libghostty_vt.a` per platform in CI to avoid rebuilding on every push. Use content hash of ghostty source as cache key.

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
