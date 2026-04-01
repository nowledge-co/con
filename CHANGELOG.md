# Changelog

All notable changes to con are documented here.

## [Unreleased]

### Added

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
- Built-in AI assistant that works with 13 providers — Anthropic, OpenAI, DeepSeek, Groq, Gemini, Ollama, OpenRouter, Mistral, Together, Cohere, Perplexity, xAI, and any OpenAI-compatible endpoint
- Transparent execution — when the agent runs a command, it executes right in your terminal. You see every keystroke, every output, in real time. No hidden processes.
- Deep context awareness — the agent sees your working directory, recent output, command history, git branch, uncommitted changes, and project structure. It reasons about what you're actually doing.
- Seven tools at the agent's disposal: run commands (visibly or in the background), read files, write files, surgically edit specific sections of a file, list project files, and search your codebase
- Streaming responses you can cancel mid-flight — hit Stop and the partial response is preserved
- Extended thinking — when the model reasons before responding, you can expand a collapsible section to see its thought process
- Approval workflow — the agent asks before running commands or modifying files. You stay in control. (Or toggle auto-approve for trusted sessions.)
- Multi-turn conversations with full context carried across messages
- Built-in skills: type /explain, /fix, /commit, /test, or /review to trigger purpose-built agent workflows
- Custom skills via AGENTS.md files in your project

**Interface**
- Smart input bar that auto-detects intent — shell commands go to the terminal, questions go to the agent, /skills invoke workflows
- Agent panel (Cmd+L) with structured tool call cards, inline approval dialogs, code block rendering, and a resizable width you can drag to adjust
- Settings panel (Cmd+,) to configure your provider, model, and preferences
- Command palette (Cmd+Shift+P) with fuzzy search for every action
- Session sidebar showing your open tabs
- Flexoki dark theme with carefully matched ANSI colors
