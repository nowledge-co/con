# Changelog

All notable changes to con are documented here. This project follows [Keep a Changelog](https://keepachangelog.com/) conventions.

## [Unreleased]

### Added

**Terminal**
- Full terminal emulation with 256-color and truecolor support
- Mouse text selection: click-drag, double-click word select, triple-click line select, Cmd+A select all
- Cursor blink with 500ms cycle, resets on keypress
- Scrollback buffer with mouse wheel navigation and floating scroll indicator
- Clipboard support: Cmd+C copy, Cmd+V paste with bracketed paste mode
- Cmd+K to clear terminal scrollback
- Dynamic resize — terminal fills available window space
- Alternate screen, DEC private modes, application cursor keys
- OSC 7 working directory tracking, OSC 133 command block detection
- Tab management: Cmd+T new, Cmd+W close, Cmd+1-9 switch, Cmd+Shift+[/] cycle
- Session persistence — tabs, active tab, and panel state restored on launch

**AI Agent**
- Built-in AI agent with 13 provider support (Anthropic, OpenAI, DeepSeek, Groq, Gemini, Ollama, OpenRouter, Mistral, Together, Cohere, Perplexity, xAI, and OpenAI-compatible endpoints)
- Terminal context injection — the agent sees your recent output, working directory, git branch, and command history
- Streaming responses with real-time token rendering
- Tool transparency — tool calls appear in the agent panel with arguments and results
- Tool approval — dangerous tools (shell_exec, file_write) require explicit Allow/Deny before execution
- Auto-approve toggle in settings for trusted workflows
- Multi-turn conversation with context preserved across messages
- Built-in skills: /explain, /fix, /commit, /test, /review
- Custom skills via AGENTS.md files in your project directory

**Interface**
- Smart input bar with three modes: Smart (auto-detect), Shell, Agent
- Smart mode classifies input using known command detection — shell commands go to the terminal, questions go to the agent, /skills invoke agent skills
- Agent panel (Cmd+L) with structured tool call cards, approval dialogs, and code block rendering
- Settings panel (Cmd+,) with provider selector, model configuration, terminal settings, and auto-approve toggle
- Command palette (Cmd+Shift+P) with fuzzy search — access all actions from the keyboard
- Session sidebar with tab list and new session button
- Flexoki dark theme with matched terminal ANSI colors
