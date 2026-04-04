# Changelog

All notable changes to con are documented here.

con is still pre-release, so entries may group larger areas of work while the product shape is stabilizing.

## [Unreleased]

### Added

**Terminal — Ghostty Backend**
- GPU-accelerated terminal rendering on macOS via Ghostty's Metal engine. Text rendering, scrollback, and compositing all run on the GPU for consistently smooth performance, even with high-throughput output.
- Full VT compliance out of the box — Kitty keyboard protocol, hyperlinks, sixel graphics, and OSC 133 shell integration are handled natively by Ghostty. No configuration needed.
- Instant command completion tracking — when a command finishes, con knows the exit code and exactly how long it took. The agent uses this to respond immediately instead of waiting on a timeout.
- Clipboard integration — copy and paste work natively between the terminal and the system clipboard, including programmatic clipboard access via OSC 52.

**AI Agent**
- Per-tab agent sessions — each tab has its own conversation, context, and approval state. Switch tabs freely while the agent works; background tabs keep running and accumulate responses. Your conversation stays with the tab it belongs to, and commands the agent runs always target the correct terminal.
- Agent conversations persist per-tab across restarts
- Command duration and exit code are now included in the agent's context. When you ask "what happened?", the agent can tell you a build took 12 seconds and failed with exit code 1 — not just show you the output.

### Improved

**AI Agent**
- The agent system prompt has been restructured for sharper tool usage. Questions are answered with minimal side effects; tasks are executed carefully with verification. Each tool now has explicit guidelines so the agent picks the right one the first time.
- Remote host detection works on Ghostty panes — the agent correctly identifies SSH sessions and targets the right host.
- Busy/idle detection works on Ghostty panes — the agent waits for a running command to finish before sending another.

**Smart Input**
- Command detection now scans your `$PATH` at startup instead of using a static word list. Any installed program — `hostname`, `terraform`, `kubectl`, or a custom script in `/usr/local/bin` — is correctly recognized as a shell command without manual configuration.
- Commands with flags (`free -g`, `docker --version`) are now recognized by their syntax — even when the executable isn't on your local PATH.
- SSH-aware classification — when you're in a remote session, commands like `systemctl`, `apt`, or `free` are correctly routed to the terminal instead of the AI agent.

**AI Agent**
- The agent now sees your full pane layout (hostname, directory, busy status) directly in its context. When you have multiple panes open — especially SSH sessions to different machines — the agent targets the right pane without extra steps.
- Ghostty panes now report `has_shell_integration: true` in the agent's pane list, enabling the agent to use command tracking features.

### Fixed

**AI Agent**
- Fixed agent hanging after receiving a final response from certain providers
- Fixed empty agent responses appearing as stuck/hanging when providers don't emit text items during streaming

**Terminal**
- Full terminal emulation with 256-color and truecolor support
- Split panes — divide your workspace horizontally (Cmd+D) or vertically (Cmd+Shift+D), with drag-to-resize dividers
- Mouse text selection with click-drag, double-click for words, triple-click for lines, and Cmd+A to select all
- Scrollback buffer with smooth scroll and a floating indicator showing how far back you are
- Clipboard integration with Cmd+C / Cmd+V, including bracketed paste mode for safe pasting into editors
- Cmd+K to clear your scrollback history
- Tab management — Cmd+T to open, Cmd+W to close, Cmd+1–9 to switch, Cmd+Shift+[/] to cycle
- Session restore — your tabs, layout, and panel state are preserved when you relaunch
- Full compatibility with terminal applications like vim, htop, and tmux (alternate screen, application cursor keys, DEC private modes, Kitty keyboard protocol)

**AI Agent**
- Built-in AI assistant that works with 13 providers — Anthropic, OpenAI, DeepSeek, Groq, Gemini, Ollama, OpenRouter, Mistral, Together, Cohere, Perplexity, xAI, and any OpenAI-compatible endpoint. Each provider uses its native Rig client for correct API routing and auth.
- Transparent execution — when the agent runs a command, it executes right in your terminal. You see every keystroke, every output, in real time. No hidden processes.
- Deep context awareness — the agent sees your working directory, recent output, command history, git branch, uncommitted changes, and project structure. It reasons about what you're actually doing.
- Seven tools at the agent's disposal: run commands (visibly or in the background), read files, write files, surgically edit specific sections of a file, list project files, and search your codebase
- Streaming responses you can cancel mid-flight — hit Stop and the partial response is preserved
- Extended thinking — when the model reasons before responding, you can expand a collapsible section to see its thought process
- Approval workflow — the agent asks before running commands or modifying files. You stay in control. (Or toggle auto-approve for trusted sessions.)
- Multi-turn conversations with full context carried across messages
- Built-in skills: type /explain, /fix, /commit, /test, or /review to trigger purpose-built agent workflows
- Custom skills via AGENTS.md files in your project
- Temperature control — set `temperature` in config.toml or the Settings panel to tune model creativity
- Separate suggestion model — configure `[agent.suggestion_model]` to use a fast, cheap model for inline completions while keeping a powerful model for agent chat

**Interface**
- Smart input bar that auto-detects intent — shell commands go to the terminal, questions go to the agent, /skills invoke workflows
- Agent panel (Cmd+L) with structured tool call cards, inline approval dialogs, code block rendering, and a resizable width you can drag to adjust
- Settings panel (Cmd+,) to configure your provider, model, and preferences
- Command palette (Cmd+Shift+P) with fuzzy search for every action
- Inline suggestions — type at a shell prompt and ghost text appears with AI-powered completions. Tab to accept, keep typing to dismiss.
- Session sidebar showing your open tabs
- Four built-in terminal color themes — Flexoki Dark, Flexoki Light, Catppuccin Mocha, and Tokyo Night. Switch instantly from Settings, or set your default in config.toml.
