# con

A terminal for people who live in the terminal.

GPU-accelerated. AI-native. Open source. Written in Rust.

## Why con?

The terminal hasn't changed much in forty years. The AI era bolted chat interfaces onto the side, but the terminal itself stayed the same — a passive rectangle waiting for keystrokes.

con treats the terminal and the agent as one thing. The agent lives inside the terminal. It can see what you see, read what's on screen, run commands where you run them. No context-switching. No copy-pasting output into a chat window. No explaining to the AI what just happened — it was right there.

We were inspired by [Warp](https://www.warp.dev/) and [cmux](https://github.com/nickthecook/cmux). Warp proved that the terminal could be rethought as a modern product. cmux showed that multiplexing and scripting could be unified. con takes a different path: instead of reimagining the terminal as a notebook or a script runner, we kept it as a terminal — and gave it a brain.

## What you get

- **Full terminal emulation** — Truecolor, scrollback, mouse, clipboard, alternate screen, bracketed paste, DEC private modes. Ghostty's Metal-rendered backend on macOS, a pure-Rust VTE fallback everywhere else.
- **Built-in AI agent** — 13+ providers (Anthropic, OpenAI, DeepSeek, Ollama, and more). The agent reads your terminal, knows your working directory and git branch, and runs commands with your approval.
- **One input bar** — Type a shell command or ask the AI a question. con figures out which one you meant. Or switch modes explicitly with Tab.
- **Tool transparency** — Every tool call is visible. Execution time, arguments, output — nothing hidden. Dangerous operations require explicit approval.
- **Skills** — `/explain`, `/fix`, `/commit`, `/test`, `/review`. Extend with your own via `AGENTS.md`.
- **Per-tab sessions** — Each tab owns its conversation and context. Switch tabs while the agent works.
- **12 built-in themes** — Flexoki, Catppuccin, Tokyo Night, Dracula, Nord, Rose Pine, Gruvbox, Solarized, One Half Dark, Kanagawa Wave, Everforest. Sourced from [iTerm2-Color-Schemes](https://github.com/mbadolato/iTerm2-Color-Schemes) via [ghostty.style](https://ghostty.style).

## Getting started

### Prerequisites

- Rust (stable, edition 2024)
- cmake

### Build and run

```bash
git clone https://github.com/nickthecook/kingston.git
cd kingston
cargo run -p con
```

### Configure

`~/.config/con/config.toml`. Or hit **Cmd+,**.

```toml
[terminal]
font-size = 14
theme = "flexoki-dark"

[agent]
provider = "anthropic"
api_key_env = "ANTHROPIC_API_KEY"
```

## Keyboard shortcuts

| Shortcut | Action |
|----------|--------|
| Cmd+T | New tab |
| Cmd+W | Close tab |
| Cmd+1-9 | Switch to tab |
| Cmd+Shift+[ / ] | Previous / next tab |
| Cmd+L | Toggle agent panel |
| Cmd+, | Settings |
| Cmd+K | Clear terminal |
| Cmd+C / Cmd+V | Copy / paste |
| Tab (in input bar) | Cycle input mode |

## Architecture

Four crates, clear boundaries:

| Crate | Responsibility |
|-------|---------------|
| **con** | GPUI app shell — window, tabs, panels, input bar |
| **con-core** | Shared glue — agent harness, config, session management |
| **con-terminal** | Terminal emulation — VT parser, PTY, grid, keyboard encoding |
| **con-agent** | AI harness — Rig, tool definitions, conversation, skills |

The agent harness runs on a shared tokio runtime with per-tab sessions. Events flow from agent to UI via crossbeam channels. Tool calls go through a PromptHook lifecycle that emits events at every step — the UI is never in the dark about what the agent is doing.

## Standing on the shoulders of giants

con wouldn't exist without these projects:

- **[Ghostty](https://ghostty.org/)** / **[libghostty](https://github.com/ghostty-org/ghostty)** — Terminal emulation on macOS via the Ghostty C API with Metal rendering
- **[GPUI](https://github.com/longbridgeapp/gpui-component)** — GPU-accelerated UI framework, community edition of the engine behind [Zed](https://zed.dev)
- **[Rig](https://github.com/0xPlaygrounds/rig)** — Rust AI framework powering the agent harness across 13+ LLM providers
- **[vte](https://github.com/alacritty/vte)** — VT parser from Alacritty, used in the cross-platform fallback backend
- **[portable-pty](https://github.com/wez/wezterm/tree/main/pty)** — Cross-platform PTY layer from WezTerm
- **[Phosphor Icons](https://phosphoricons.com/)** — Icon set used throughout the UI
- **[Flexoki](https://stephango.com/flexoki)** — Default color theme by Steph Ango
- **[ghostty.style](https://ghostty.style)** — Theme preview inspiration and color schemes from [iTerm2-Color-Schemes](https://github.com/mbadolato/iTerm2-Color-Schemes)

## License

MIT
