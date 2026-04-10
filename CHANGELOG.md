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
- Restored agent conversations now keep the assistant trace too, including thinking text, tool steps, model labels, and durations, instead of collapsing back to plain message bodies after restart.
- Command duration and exit code are now included in the agent's context. When you ask "what happened?", the agent can tell you a build took 12 seconds and failed with exit code 1 — not just show you the output.
- A first local automation control plane now ships with `con-cli`. External agents can list tabs and panes, read pane content, run visible shell commands, drive tmux through con's typed tmux adapter, and send prompts into a tab's built-in agent session over a local Unix socket.

### Improved

**AI Agent**
- The agent system prompt has been restructured for sharper tool usage. Questions are answered with minimal side effects; tasks are executed carefully with verification. Each tool now has explicit guidelines so the agent picks the right one the first time.
- con now depends directly on upstream Rig main for agent runtime behavior. The temporary fork used to carry the streaming multi-turn tool-call history fix has been removed now that the fix is merged upstream.
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
- con can now launch work inside tmux natively through the same typed control anchor. The agent can create a new tmux window or split pane and run a command there, which is the right path for fresh shells, Codex CLI, Claude Code, OpenCode, and long-running jobs inside tmux.
- tmux-native workflows now have higher-level helpers too. The agent can find likely tmux shell or agent-cli targets without hand-filtering every pane, and it can reuse or create a clean tmux shell target before remote file edits or command work.
- tmux-native agent CLI workflows now have a typed helper too. The agent can reuse or create a Codex CLI, Claude Code, or OpenCode tmux target directly instead of manually reconstructing that flow from `tmux_list_targets` and `tmux_run_command`.
- con can now resolve the best work target across a whole split layout. The agent has a typed helper to choose the right visible shell pane, tmux workspace, tmux shell target, or agent CLI target instead of re-deriving that decision from raw pane lists every turn.
- Multi-host SSH work now has a typed reuse-or-create helper too. The agent can ask con to reuse an existing SSH pane for a host, or create one with explicit split placement, instead of spawning duplicate remote panes on follow-up turns.
- con now distinguishes tmux-controlled agent CLIs from true native app attachments. Codex and OpenCode both have richer protocol surfaces, but con only treats them as tmux targets unless an explicit launch-integrated attachment is proven.
- When native tmux control is available, the agent now treats tmux-native target discovery and tmux-native key delivery as the default path. Raw outer-pane `send_keys` is now fallback-only for tmux instead of the first choice.
- con no longer teaches editor-specific keystroke playbooks in the main agent prompt. The built-in control surface now stays focused on shells, SSH, tmux, and agent CLIs, with generic TUI input only as a fallback.
- Sending the first agent message no longer performs local `git diff`, project listing, and `AGENTS.md` reads on the UI thread. Workspace enrichment now happens on the harness runtime, reducing visible app stalls when a request starts.
- Before the model runs, con now performs a deterministic non-intrusive fact pass on the focused pane. It no longer types a visible shell probe into the terminal before the first response. When a real tmux control anchor already exists, con still preloads tmux inventory because that query is silent and read-only.
- con now derives explicit weak observation hints from the current visible screen, such as prompt-like input near the bottom or htop-like output. These hints are labeled as observations, not facts, so the agent can describe what appears to be on screen without pretending it has backend proof.
- Session-state answers now explicitly synthesize proven facts, current-screen assessment, and unknowns/limits. When con already has visible-screen observations, the agent is guided to give the best bounded assessment instead of ending with a vague “inspect more closely” fallback.
- Multi-pane session-state answers now carry a deterministic pane-layout summary. When a tab has split panes, the agent is guided to mention the pane count and the materially different peer panes instead of collapsing the whole terminal state into only the focused pane.
- When multiple panes are open, the prompt now carries typed work-target hints too. The agent can see which pane is the best visible shell target and which pane is the best tmux workspace before it decides whether to call a resolver tool.
- SSH workspaces are now tracked across the whole tab, not just the focused pane. con keeps a typed inventory of proven remote hosts and con-managed SSH continuity, so follow-up requests can reuse the right host panes instead of recreating them.
- Follow-up remote work is now allowed to reuse con-created SSH panes even when fresh shell integration is not currently proven, as long as the pane still looks like a prompt and no tmux or TUI evidence contradicts that continuity.
- Pane tools now expose both `pane_index` and stable `pane_id`. `pane_index` remains the current visible layout position, while `pane_id` stays stable for the life of that pane inside the tab. Follow-up agent work now prefers `pane_id`, so adding or closing a split does not silently retarget later actions.
- Stale pane targeting is now explained more clearly too. If a pane was closed or the split layout changed, con returns a direct recovery message that tells the agent to re-run `list_panes` and continue with `pane_id` instead of quietly failing on an old index.
- con now summarizes the whole tab as typed workspaces. The agent can distinguish a ready remote shell, a tmux workspace, a disconnected SSH pane, or a pane that still needs inspection before it acts.
- Multi-host remote work now has a higher-level `remote_exec` path. The agent can reuse or create SSH workspaces for multiple hosts and run the same command across them in parallel without manually stitching pane creation and batch execution together.
- Current-screen SSH state is modeled more cleanly too. Login banners and closed-SSH screens are now captured as observations, which helps the agent tell the difference between a live remote shell and a pane whose remote session already ended.
- tmux-like screens are now captured as observations too. con can describe a pane as looking like tmux and route tmux-oriented questions toward that pane without pretending it already has native tmux control there.
- SSH workspace reuse is stricter now. A pane that visibly shows a closed SSH connection or a tmux-like screen is no longer silently reused as a plain remote shell target on follow-up turns.
- The agent panel now uses quieter run cards with calmer state labels, stronger alignment, and cleaner output blocks. Long tool results still expand inline, but the panel no longer relies on a loud “finished” banner to signal that a run is done.
- The agent panel now uses stronger Phosphor fill and duotone icons, clearer section labels, and more deliberate spacing so tool traces feel like a composed operator timeline instead of a generic debug inspector.
- The agent panel trace rows now expand as connected cards instead of detached header-and-output boxes, and model identity is shown as chips instead of awkward raw provider/model strings. That makes long tool traces and provider metadata read more like a composed operator surface and less like debug text.
- The live agent panel now shows human-readable provider/model labels instead of Rust-style `Thinking("provider:model")` debug text, expanded outputs use quieter inline toggles with a dedicated mono sub-surface, and the default accent path now follows the theme's blue primary token instead of an unintended cyan bias.
- Expanded agent trace output now reads as a clearer nested surface, with a stronger inner tone for code and log text so the card hierarchy stays legible in long tool runs.
- Agent trace cards now use distinct theme layers for run groups, tool rows, and inner output blocks. Light and dark themes both keep the title row, body, and nested code/log output visually separate instead of flattening into one tone.
- The terminal-agent benchmark now includes richer operator playbooks and profiles for local Codex dev loops, dual-host SSH maintenance, and remote tmux edit/run/reuse workflows. It can now execute those operator prompt sequences directly and record the transcript instead of only printing a playbook path.
- The terminal-agent benchmark now has stable scoring rubrics, a scoring tool, a trend-report generator, and a project-local improvement-loop skill so progress can be judged and compared over many iterations instead of living only in screenshots and memory.
- `con-cli agent ask` and operator benchmark steps can now be bounded with explicit timeouts, so a stuck agent turn fails cleanly instead of hanging the entire automation loop.
- The benchmark loop now writes a tracked improvement log entry and trend-chart report, so repeated iterations leave behind comparable notes and a durable progress trail in the repo.
- Operator benchmark profiles can now start from a fresh conversation and run deterministic visible-shell setup commands before the first prompt, which makes repeated local Codex and SSH/tmux evaluations more stable.
- The terminal-agent benchmark now ships an isolated batch runner, so multi-iteration evaluation can launch a fresh Con runtime per run instead of reusing polluted session state.

**Terminal**
- New Ghostty panes now inherit the requested working directory and font size at creation time, which keeps restored tabs and newly opened panes aligned with the workspace state.
- Split requests now flow through Ghostty's native split action path before con creates the new surface, so pane insertion follows Ghostty split direction semantics instead of treating every split as a purely external GPUI command.
- Font size changes now apply to existing Ghostty panes immediately, so terminal text updates in place when you save Settings.
- Terminal theme changes now apply to live Ghostty panes immediately instead of only updating the surrounding con interface.
- Control-created panes now bootstrap as real Ghostty surfaces immediately instead of staying as uninitialized placeholders until a later render pass. `panes.create` now returns `surface_ready`, `is_alive`, and `has_shell_integration`, which makes CLI- and agent-driven pane setup much more truthful.
- Split rails now keep a visible resting presence instead of appearing only on hover, so sparse or newly created panes still read as separate terminal workspaces at a glance.
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
- Fixed a Unicode logging crash where long tool-result previews could be truncated at an invalid UTF-8 byte boundary

**Terminal**
- Full terminal emulation with 256-color and truecolor support
- Split panes — divide your workspace horizontally (Cmd+D) or vertically (Cmd+Shift+D), with drag-to-resize dividers
- Mouse text selection with click-drag, double-click for words, triple-click for lines, and Cmd+A to select all
- Scrollback buffer with smooth scroll and a floating indicator showing how far back you are
- Clipboard integration with Cmd+C / Cmd+V, including bracketed paste mode for safe pasting into editors
- Cmd+K now clears the current Ghostty screen and scrollback using Ghostty's native action path
- Fixed a Ghostty theme sync regression where the Settings panel could update con's chrome but leave the terminal on an old palette after a later runtime config update
- Fixed a shutdown crash in the workspace mouse/resize path when the active tab index outlived the tab list during window teardown
- Fixed another shutdown-time panic where late workspace renders or actions could still assume an active pane after the last tab was already gone
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
- The agent panel now keeps live activity visible near the top while a run is in progress, shows a labeled Stop action in the header, and lets you expand long tool results instead of forcing every step into the same short preview.
- Settings panel (Cmd+,) to configure your provider, model, and preferences
- Command palette (Cmd+Shift+P) with fuzzy search for every action
- Session sidebar showing your open tabs
- Four built-in terminal color themes — Flexoki Dark, Flexoki Light, Catppuccin Mocha, and Tokyo Night. Switch instantly from Settings, or set your default in config.toml.
