# con

A GPU-accelerated terminal emulator with a built-in AI agent. Open source, cross-platform, written in Rust.

## What is con?

con is a terminal that treats AI as a first-class feature, not an afterthought. It combines a fast, correct terminal emulator with a native agent harness that can see your terminal, understand your context, and take action — all without leaving your workflow.

**Key capabilities:**

- **Full terminal emulation** — 256-color and truecolor, scrollback, mouse selection, clipboard, cursor blink, alternate screen, bracketed paste, DEC private modes
- **Built-in AI agent** — 13 providers (Anthropic, OpenAI, DeepSeek, Ollama, and more) via the Rig framework. The agent sees your terminal output, knows your working directory and git branch, and can run commands with your approval
- **Smart input bar** — Type naturally. con auto-detects whether you're entering a shell command or asking the AI a question. Or switch to explicit Shell/Agent mode
- **Tool transparency** — When the agent runs a command or writes a file, you see exactly what it's doing. Dangerous tools require explicit approval
- **Skills** — Built-in actions like `/explain`, `/fix`, `/commit`, `/test`, `/review`. Extend with your own via `AGENTS.md`
- **Session persistence** — Tabs, layout, and agent panel state are saved and restored automatically

## Getting Started

### Prerequisites

- Rust (stable, edition 2024)
- cmake

### Build and Run

```bash
git clone https://github.com/nickthecook/kingston.git
cd kingston
cargo run -p con
```

### Configure

Settings are stored in `~/.config/con/config.toml`. Open the settings panel with **Cmd+,** to configure your AI provider, model, and terminal preferences.

```toml
[terminal]
font-size = 14
scrollback-lines = 10000

[agent]
provider = "anthropic"
api_key_env = "ANTHROPIC_API_KEY"
auto_approve_tools = false
```

## Keyboard Shortcuts

| Shortcut | Action |
|----------|--------|
| Cmd+T | New tab |
| Cmd+W | Close tab |
| Cmd+1-9 | Switch to tab |
| Cmd+Shift+[ / ] | Previous / next tab |
| Cmd+L | Toggle agent panel |
| Cmd+, | Settings |
| Cmd+Shift+P | Command palette |
| Cmd+K | Clear terminal |
| Cmd+A | Select all |
| Cmd+C | Copy selection |
| Cmd+V | Paste |
| Tab (in input bar) | Cycle input mode |

## Architecture

con is structured as a Rust workspace with clear crate boundaries:

- **con** — GPUI app shell (window, tabs, panels, input bar)
- **con-core** — Shared logic (agent harness, config, session management)
- **con-terminal** — Terminal emulation (VT parser, PTY, grid, keyboard encoding)
- **con-agent** — AI harness (Rig 0.34, tool definitions, conversation, skills)

The agent harness runs on a shared tokio runtime. Events flow from the agent to the UI via crossbeam channels. Tool calls go through a PromptHook lifecycle that emits events for every step — the UI is never in the dark about what the agent is doing.

## License

MIT
