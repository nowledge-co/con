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
- Ghostty is now the only terminal runtime in con. The old in-app VTE/PTTY fallback path has been removed, so every pane uses the same terminal engine and the same behavior.

**AI Agent**
- Per-tab agent sessions — each tab has its own conversation, context, and approval state. Switch tabs freely while the agent works; background tabs keep running and accumulate responses. Your conversation stays with the tab it belongs to, and commands the agent runs always target the correct terminal.
- Agent conversations persist per-tab across restarts
- Command duration and exit code are now included in the agent's context. When you ask "what happened?", the agent can tell you a build took 12 seconds and failed with exit code 1 — not just show you the output.

### Improved

**AI Agent**
- The agent system prompt has been restructured for sharper tool usage. Questions are answered with minimal side effects; tasks are executed carefully with verification. Each tool now has explicit guidelines so the agent picks the right one the first time.
- Busy/idle detection works on Ghostty panes — the agent waits for a running command to finish before sending another.
- Pane-aware context is stricter and more honest. con no longer guesses SSH hosts, tmux sessions, or agent CLIs from pane titles or status-line patterns. When the foreground runtime is not proven, it stays `unknown`.
- Visible shell execution now depends on real Ghostty command boundaries instead of stale cwd or title clues. After any unconfirmed input, con stops trusting shell metadata until shell integration proves a fresh prompt again.
- con now refuses `terminal_exec` and `batch_exec` on panes that are not proven plain-shell targets. This prevents the built-in agent from typing shell commands into tmux+nvim or other visible TUIs.
- Pane control state is now typed and shared across the prompt, `list_panes`, and execution guards. The agent now sees each pane's address space, visible target, explicit control attachments, control channels, capabilities, and control notes instead of relying on flat pane heuristics.
- Pane metadata now also exposes backend observability limits directly. If embedded Ghostty cannot prove foreground command text, alternate-screen state, or remote-host identity for a pane, con says so instead of guessing.
- The control plane can represent nested targets explicitly, and it now uses `unknown` for unproven foreground targets instead of pretending every ambiguous pane is a TUI.
- con now exposes a read-only `probe_shell_context` tool on panes with a proven fresh shell prompt. This gives the agent a typed way to ask the live shell for hostname, SSH env, tmux env, tmux session/window/pane ids, and Neovim socket hints instead of guessing from screen text.
- Pane runtime state is now reducer-backed instead of snapshot-only. con tracks each pane's recent actions, typed shell-context snapshots, and freshness rules, so the agent can reuse truthful causal history without confusing it for the current foreground target.
- Pane runtime now separates the current verified foreground stack from the last verified shell frame. If con cannot prove what is visible now, it keeps the live target `unknown` and shows the last verified shell context separately instead of pretending history is current state.
- con now exposes the first native tmux control layer through a proven same-session shell anchor. When a fresh shell probe confirms tmux, the agent can list tmux targets, capture a specific tmux pane, and send tmux-native keys to a chosen tmux pane instead of typing blindly into the outer terminal surface.

**Terminal**
- New Ghostty panes now inherit the requested working directory and font size at creation time, which keeps restored tabs and newly opened panes aligned with the workspace state.
- Font size changes now apply to existing Ghostty panes immediately, so terminal text updates in place when you save Settings.
- Terminal theme changes now apply to live Ghostty panes immediately instead of only updating the surrounding con interface.
- Terminal settings are simpler and more honest. con no longer exposes backend switching or fake scrollback tuning for features that are owned by Ghostty itself.

**Smart Input**
- Command detection now scans your `$PATH` at startup instead of using a static word list. Any installed program — `hostname`, `terraform`, `kubectl`, or a custom script in `/usr/local/bin` — is correctly recognized as a shell command without manual configuration.
- Commands with flags (`free -g`, `docker --version`) are now recognized by their syntax — even when the executable isn't on your local PATH.
- Remote-sensitive classification now only activates when remote identity is proven. When con cannot prove a pane is remote, it stays conservative instead of guessing.

**AI Agent**
- The agent now sees pane indexes, directories when available, busy status, control notes, and backend limits directly in its context. This reduces pane-targeting mistakes without pretending SSH or tmux identity is known when it is not.
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
- Cmd+K now clears the current Ghostty screen and scrollback using Ghostty's native action path
- Fixed a Ghostty theme sync regression where the Settings panel could update con's chrome but leave the terminal on an old palette after a later runtime config update
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
- Session sidebar showing your open tabs
- Four built-in terminal color themes — Flexoki Dark, Flexoki Light, Catppuccin Mocha, and Tokyo Night. Switch instantly from Settings, or set your default in config.toml.
