# Changelog

All notable changes to con are documented here.

con is still pre-release, so entries may group larger areas of work while the product shape is stabilizing.

## [Unreleased]

### Added

**Interface**
- A modeless `About con` window now shows the app icon, release version, build number, release channel, and repository URL.
- Appearance settings now include a configurable UI font size, separate from terminal font size.
- con now supports an optional system-wide summon/hide shortcut on macOS. It ships disabled by default so it does not conflict with launchers or existing global shortcuts.
- `cmd-n` now opens a real new window instead of reusing the current workspace.
- The bottom command bar now has a pane-scope picker that can target the focused pane, all panes, or an explicit subset of panes.
- The command bar now supports global command-history suggestions, local path completion for local panes, and an AI suggestion fallback that can be enabled independently.

**Terminal — Ghostty Backend**
- GPU-accelerated terminal rendering on macOS via Ghostty's Metal engine. Text rendering, scrollback, and compositing all run on the GPU for consistently smooth performance, even with high-throughput output.
- Full VT compliance out of the box — Kitty keyboard protocol, hyperlinks, sixel graphics, and OSC 133 shell integration are handled natively by Ghostty. No configuration needed.
- Instant command completion tracking — when a command finishes, con knows the exit code and exactly how long it took. The agent uses this to respond immediately instead of waiting on a timeout.
- Clipboard integration — copy and paste work natively between the terminal and the system clipboard, including programmatic clipboard access via OSC 52.
- Ghostty is now the only terminal runtime in con. The old in-app VTE/PTTY fallback path has been removed, so every pane uses the same terminal engine and the same behavior.
- Embedded Ghostty ticking on macOS now follows Ghostty's wakeup-driven runtime model instead of a host-side polling loop, and the hosted NSView stack now uses native live-resize redraw policies for smoother terminal window resizing.
- Ghostty panes no longer run their own per-surface 60fps GPUI polling loops. Surface event draining and deferred resize/retry housekeeping now flow through one workspace-level Ghostty wake pump, which removes redundant host-side churn during resize and other heavy terminal activity.
- The macOS window now adopts Ghostty-style cell-step resize increments from the active terminal surface, reducing pointless intermediate resize states during live drags and moving Con closer to Ghostty's own resize behavior.
- The normal workspace render path no longer forces fresh terminal observations just to populate pane metadata. Runtime facts are now cached from explicit observation paths and reused by the UI, which avoids reading terminal text during ordinary renders while a heavy TUI is active.
- The embedded macOS Ghostty host no longer synthesizes a fake scrollbar-at-top state before Ghostty emits real viewport data. The native scroll container now stays inert until Ghostty provides actual scrollbar state, which avoids forcing heavy TUIs through upper scrollback on startup or during reflow.

**AI Agent**
- Per-tab agent sessions — each tab has its own conversation, context, and approval state. Switch tabs freely while the agent works; background tabs keep running and accumulate responses. Your conversation stays with the tab it belongs to, and commands the agent runs always target the correct terminal.
- Agent conversations persist per-tab across restarts
- Restored agent conversations now keep the assistant trace too, including thinking text, tool steps, model labels, and durations, instead of collapsing back to plain message bodies after restart.
- Command duration and exit code are now included in the agent's context. When you ask "what happened?", the agent can tell you a build took 12 seconds and failed with exit code 1 — not just show you the output.
- A first local automation control plane now ships with `con-cli`. External agents can list tabs and panes, read pane content, run visible shell commands, drive tmux through con's typed tmux adapter, and send prompts into a tab's built-in agent session over a local Unix socket.

### Improved

**Interface**
- The agent panel can now expand with the window instead of stopping at a fixed width ceiling, while still preserving a minimum terminal area. Wide markdown content such as tables and code blocks now uses the panel's full available width instead of being forced through the prose column.
- macOS terminal profiling now has a dedicated runbook and opt-in host-path logs, making it easier to distinguish Ghostty core time from Con's embedding/composition cost when diagnosing heavy TUI resize issues.
- The macOS `xctrace` profiling helper now builds and launches the real `con` binary instead of profiling the `cargo` wrapper process, so resize traces reflect the app under test.

**Release and Packaging**
- The macOS app bundle, updater, and release pipeline are now documented and exercised as real product surfaces. Local updater testing now runs from a bundled beta app instead of `cargo run`, and version metadata is visible in both Settings and the About window.
- Workspace dependencies no longer rely on local `3pp/` checkouts at build time. GPUI, gpui-component, Rig, and Ghostty now resolve from upstream git sources or scripted fetches instead of local path dependencies.

**AI Agent**
- The agent system prompt has been restructured for sharper tool usage. Questions are answered with minimal side effects; tasks are executed carefully with verification. Each tool now has explicit guidelines so the agent picks the right one the first time.
- `con` currently pins `rig-core` to a maintained fork revision while provider work settles upstream. The build no longer depends on a local source checkout for Rig.
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
- tmux native control can now also come from fresh shell continuity plus recent con-caused tmux setup. If con just created or targeted a tmux session from a still-fresh shell prompt, that shell can immediately become a tmux control anchor instead of falling back to raw tmux keystrokes first.
- con can now launch work inside tmux natively through the same typed control anchor. The agent can create a new tmux window or split pane and run a command there, which is the right path for fresh shells, Codex CLI, Claude Code, OpenCode, and long-running jobs inside tmux.
- tmux-native workflows now have higher-level helpers too. The agent can find likely tmux shell or agent-cli targets without hand-filtering every pane, and it can reuse or create a clean tmux shell target before remote file edits or command work.
- tmux shell work now has a typed follow-up tool too. Once a clean tmux shell target exists, the agent can run one deterministic shell command there, wait for that shell target to settle, and return a fresh capture instead of manually composing tmux key delivery and capture steps.
- tmux-native agent CLI workflows now have a typed helper too. The agent can reuse or create a Codex CLI, Claude Code, or OpenCode tmux target directly instead of manually reconstructing that flow from `tmux_list_targets` and `tmux_run_command`.
- Remote `ssh -> tmux -> shell-work` bootstrap now has a typed one-step helper too. con can reuse or create the SSH pane, ensure the tmux session, and then reuse or create the clean tmux shell target for file work without forcing the agent to reconstruct that path by hand.
- tmux-native control can now survive one more honest boundary: if con recently prepared tmux from a pane and the current visible screen still looks like a prompt inside that tmux workspace, con can retain the same-session tmux shell anchor long enough to keep tmux-native tools available without pretending the foreground app is fully proven.
- con can now resolve the best work target across a whole split layout. The agent has a typed helper to choose the right visible shell pane, tmux workspace, tmux shell target, or agent CLI target instead of re-deriving that decision from raw pane lists every turn.
- Multi-host SSH work now has a typed reuse-or-create helper too. The agent can ask con to reuse an existing SSH pane for a host, or create one with explicit split placement, instead of spawning duplicate remote panes on follow-up turns.
- Disconnected SSH workspaces now stay visible to typed target resolution too. When one host pane has dropped, con can surface it as a selective-recovery target and point the agent at `ensure_remote_shell_target` for that host instead of pretending no routing information remains.
- Local coding-agent workflows now have a typed paired-shell helper too. The agent can keep Codex, Claude Code, or OpenCode in one local pane while reusing or creating a separate local shell pane for file edits, test runs, and other shell work.
- Local coding-agent workflows now have a typed agent-target helper too. con can reuse or launch a local Codex, Claude Code, or OpenCode pane explicitly instead of leaving that choice implicit in model reasoning.
- Local coding-agent workflows now also have a typed paired-workspace helper. con can reuse or prepare the whole local coding pair for a project path at once: one interactive Codex / Claude Code / OpenCode pane plus one separate shell pane for file/test/git work.
- Visible agent-cli follow-up turns now wait for output change-and-settle instead of shell-idle semantics, so Codex / Claude Code / OpenCode panes are treated like interactive foreground apps instead of shell prompts with stale shell integration under them.
- The local paired-coding rule is stricter now: deterministic file writes, test runs, and other direct shell work should stay in the shell lane by default, while the interactive Codex / Claude Code / OpenCode lane is reserved for explicit CLI interaction or clearing a blocking trust/continue prompt.
- Local coding workspaces are now summarized at the tab level too. The agent can see reusable local shell and agent-cli workspaces, including project-path hints and recent local continuity, instead of rebuilding that routing decision from raw panes every turn.
- Local coding target reuse is stricter now. A pane that was recently launched for Codex / Claude Code / OpenCode is no longer silently reused as a generic local shell target on follow-up turns.
- Local coding workspace preparation now normalizes `~` project paths before startup shell quoting, so new local agent and shell panes no longer fail with literal `cd '~/project'` commands.
- Local coding workspace preparation can now create the requested project directory as part of its own bootstrap, so “prepare a workspace in X” no longer burns the first turn just recovering from a missing local path.
- Interactive Codex / Claude Code / OpenCode follow-up turns now have a typed tool too. con can send one prompt into an existing local or tmux agent-cli target, wait for that target to settle, and return a fresh snapshot instead of forcing the model to improvise raw `send_keys` plus timing guesses.
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
- The agent panel trace hierarchy is quieter now. Run groups use a very light outer surface, tool rows use the main secondary surface, and expanded code/log output sits on a deeper nested surface so the panel stays layered without looking boxed-in.
- The agent panel trace stack is quieter and more borderless now. The outer run group no longer reads as a heavy card, while step rows and nested output keep a clear neutral hierarchy without muddy tint stacking.
- Expanded tool traces now use a calmer nested rhythm: a light step shell, a distinct detail surface when unfolded, and a deeper mono result block inside that detail surface. The structure stays connected, but each layer now reads as a different depth.
- Run traces now keep a clearer section scope too. The group itself has a faint outer shell again, the section header sits on its own quiet surface, and the inner spacing is looser so expanded steps no longer feel visually jammed together.
- Step cards and live tool cards now separate header and body more clearly. The title row sits on its own header surface, while expanded detail stays nested below it, so rows like `Read Pane` no longer flatten into the page background.
- Step headers are now inset within their card shells, so the first layer reads as a true card header instead of a flat strip on the page. Expanded detail keeps its own quieter body surface below that header.
- Trace cards now use structural shell padding instead of margin tricks. That keeps the header plane visibly inside the card, makes collapsed rows read as real cards, and fixes the awkward hover geometry around titles like `Read Pane`.
- Trace card layers now use the real theme surface ladder again instead of ultra-low-opacity neutrals. In light mode especially, rows like `Read Pane` should now read as visible cards with a distinct header plane instead of flattening into the page.
- Run trace groups now sit on their own visible section surface, and structured tool output like `key: value` blocks renders as aligned rows instead of a flat monospace dump. That gives expanded steps a clearer hierarchy and a more legible body layout.
- Agent traces now use a simpler, more deliberate surface ladder: one quiet section shell, unified step cards, and a single inset content box for expanded output. Header rows, unfolded content, and mono result blocks now align to the same inset instead of drifting into separate mini-cards.
- Agent trace shells now sit more clearly above the chat background, step cards use rounder geometry, structured shell output enforces monospace styling, and expanded content aligns closer to its step header instead of drifting too far right.
- Agent panel trace polish now uses calmer custom model chips, tighter radius scaling, stronger monospace treatment for shell-style output, and better text contrast across step headers, details, and expanded result blocks.
- The unfolded step body now aligns to the card’s icon/title grid instead of drifting inward, and the chat/trace typography uses a cleaner shared scale for prose, captions, chips, and monospace output.
- The terminal-agent benchmark now includes richer operator playbooks and profiles for local Codex dev loops, dual-host SSH maintenance, and remote tmux edit/run/reuse workflows. It can now execute those operator prompt sequences directly and record the transcript instead of only printing a playbook path.
- The terminal-agent benchmark now covers local Claude Code and OpenCode dev loops too, so Con can compare the same project-prep, test-run, and follow-up-repair workflow across all three supported local coding CLIs instead of only Codex.
- The terminal-agent benchmark now also covers richer local git-backed workflows and local interactive session-resume stories for Codex, Claude Code, and OpenCode. Those tracks measure paired shell-plus-CLI continuity across git commits, diff review, and later return-to-the-same-pane follow-up turns.
- The terminal-agent benchmark now also includes a dual-host SSH recovery track, so Con can be scored on recovering only the disconnected host workspace instead of recreating every remote pane after a failure.
- The terminal-agent benchmark now has stable scoring rubrics, a scoring tool, a trend-report generator, and a project-local improvement-loop skill so progress can be judged and compared over many iterations instead of living only in screenshots and memory.
- `con-cli agent ask` and operator benchmark steps can now be bounded with explicit timeouts, so a stuck agent turn fails cleanly instead of hanging the entire automation loop.
- Control-plane `agent ask` timeouts are now request-scoped. A finished `con-cli agent ask` can no longer leave behind a stale timer that aborts the next request on the same tab.
- Closing or resetting a tab now clears or reindexes any pending control-plane `agent.ask` state for that tab, so a killed benchmark or closed tab cannot leave behind a stale “tab is busy” entry that poisons the next fresh tab at the same index.
- The benchmark loop now writes a tracked improvement log entry and trend-chart report, so repeated iterations leave behind comparable notes and a durable progress trail in the repo.
- Operator benchmark profiles can now start from a fresh conversation and run deterministic visible-shell setup commands before the first prompt, which makes repeated local Codex and SSH/tmux evaluations more stable.
- Operator benchmark profiles can now also run deterministic local shell setup commands before the first prompt. The built-in operator profiles use that for dependency-free local Python setup and clean tmux-session hygiene on remote hosts.
- The terminal-agent benchmark now ships an isolated batch runner, so multi-iteration evaluation can launch a fresh Con runtime per run instead of reusing polluted session state.
- The terminal-agent benchmark now isolates restored session state on macOS too. Fresh iterations use dedicated session and conversation paths, and repeated Ghostty bootstrap launch failures are classified as blocked environment runs instead of misleading scored regressions.

**Terminal**
- New Ghostty panes now inherit the requested working directory and font size at creation time, which keeps restored tabs and newly opened panes aligned with the workspace state.
- Split-pane creation and new-tab activation are now off the synchronous session-save path, which reduces visible latency during interactive workspace changes.
- Split requests now flow through Ghostty's native split action path before con creates the new surface, so pane insertion follows Ghostty split direction semantics instead of treating every split as a purely external GPUI command.
- Font size changes now apply to existing Ghostty panes immediately, so terminal text updates in place when you save Settings.
- Terminal theme changes now apply to live Ghostty panes immediately instead of only updating the surrounding con interface.
- Control-created panes now bootstrap as real Ghostty surfaces immediately instead of staying as uninitialized placeholders until a later render pass. `panes.create` now returns `surface_ready`, `is_alive`, and `has_shell_integration`, which makes CLI- and agent-driven pane setup much more truthful.
- Control-created tabs now bootstrap their first Ghostty pane through the same visibility and surface-ensure path as normal tab activation, so `tabs.new` no longer leaves automation on a dead first pane.
- The control plane can now create and close tabs through `con-cli`, and workspace bootstrap now reasserts new or newly focused terminals until Ghostty either comes up or the environment proves it cannot.
- Split rails now keep a visible resting presence instead of appearing only on hover, so sparse or newly created panes still read as separate terminal workspaces at a glance.
- Split panes no longer blank out when they lose focus. Embedded Ghostty surfaces are no longer dimmed with GPUI opacity, which restores correct rendering for unfocused split panes while keeping the layout borderless.
- Con now reasserts visibility for every active-tab Ghostty pane during normal renders, which hardens split layouts against stale hidden-surface state after split churn or overlay transitions.
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
- Fixed inline `<think>...</think>` reasoning from some models so it now renders in the collapsible reasoning section instead of leaking raw tags into chat output.
- Fixed ChatGPT Subscription model discovery so it now inherits the full OpenAI `models.dev` catalog instead of falling back to a much shorter local list.
- Fixed MiniMax and Z.AI provider settings so Anthropic API mode, transport toggles, and matching endpoint presets survive Settings reopen instead of silently falling back.
- Fixed configured-provider filtering in the agent panel so the session provider picker only shows providers you have actually set up.
- Fixed conversation history title truncation for CJK text so opening history no longer panics on multibyte characters.
- Fixed tmux native control helper quoting so tmux list/capture/exec commands no longer leak into the target shell as broken `quote>` input when a quoted tmux target is present
- Fixed a visible-panel performance regression where assistant messages were reparsed as markdown on every render.
- Fixed a second visible-panel performance regression where fenced code blocks rebuilt syntax-highlight runs on every render.
- Aligned embedded Ghostty viewport hosting with standalone Ghostty on macOS by consuming Ghostty scrollbar actions and wrapping each surface in a native scroll container. This keeps bottom anchoring stable during heavy TUI resize instead of briefly jumping into upper scrollback.
- Removed Con-specific live-resize coalescing from the embedded Ghostty surface path on macOS. Surface resizes now follow Ghostty's own immediate backing-size update model instead of deferring commits behind host-side heuristics.
- Fixed Ghostty resource packaging and dev-run bootstrap on macOS. Con now embeds Ghostty's `Resources/ghostty` payload into app bundles and points debug runs at the built Ghostty resources so child processes get `xterm-ghostty`, `TERMINFO`, and Ghostty shell integration instead of silently falling back to `xterm-256color`.

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
- Fixed last-window close teardown for embedded Ghostty surfaces. Closing the final tab or window now shuts down Ghostty surfaces explicitly before window removal instead of relying on late destructor cleanup.
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
- The single-tab state now uses a compact titlebar instead of a full tab strip, which recovers terminal space without hiding core window controls.
- Agent panel (Cmd+L) with structured tool call cards, inline approval dialogs, code block rendering, and a resizable width you can drag to adjust
- Fixed the agent-panel provider and model menus so long provider/model catalogs open in a bounded scrollable popup instead of getting stuck in an unscrollable header dropdown.
- The agent panel now keeps live activity visible near the top while a run is in progress, shows a labeled Stop action in the header, and lets you expand long tool results instead of forcing every step into the same short preview.
- Settings panel (Cmd+,) to configure your provider, model, and preferences
- Appearance settings now expose terminal blur separately from terminal opacity, so Ghostty glass and blur can be tuned independently.
- Fixed terminal blur on macOS so changing the blur setting now reapplies Ghostty's window blur to existing terminal windows, instead of only updating the stored config.
- Fixed shifted punctuation in embedded terminal key handling, so `:` and related keys work correctly in Vim and other TUI apps.
- Fixed Chinese and other IME text input in embedded terminal panes by routing committed composition text through GPUI's native text-input handler.
- Clicking Con's Dock icon now reopens a window when the app is still running with zero open windows, instead of forcing `cmd-n`.
- Added standard macOS app-menu commands for Hide con, Hide Others, and Show All, including their native system shortcuts.
- Reduced terminal flashing when hiding or showing the bottom input bar and agent panel by removing the full-terminal transition matte from those interactions.
- Reduced tab transition flashing by removing the full-terminal matte from tab creation/switching and deferring closed-tab surface teardown until the replacement tab is visible.
- Fixed Appearance slider layout so Terminal Glass and Window Chrome use the same fixed-width control lane.
- Added a macOS 12 compatibility guard for terminal glass. On Monterey and older, Con now forces the embedded terminal to solid/no-blur to avoid transparent blank windows caused by old WindowServer + embedded Metal composition behavior.
- Fixed bottom input history navigation across Smart, Shell, and Agent modes. Up/Down now recall submitted input directly, while completion popups still keep their own selection behavior.
- Fixed terminal IME/cursor polish: printable keys now follow the terminal-safe IME path, IME candidate placement uses Ghostty's real cursor anchor, and Con's cursor style setting now applies to embedded Ghostty panes.
- Fixed duplicate ASCII input from the terminal IME bridge and added a Cursor Style selector in Appearance settings.
- Polished Appearance settings by moving Cursor Style into its own Cursor section instead of mixing terminal behavior with font sizing.
- Restored terminal cursor blinking and tied Ghostty cursor focus visuals to Con's pane scope, including broadcast and custom multi-pane targets.
- Fixed Chinese IME composition leaving pinyin/preedit text behind in terminal panes.
- Fixed typing ASCII while a Chinese IME is in English mode by consuming terminal key events explicitly instead of dropping ASCII IME commits.
- Fixed the side agent-panel composer focus boundary so terminal key forwarding no longer steals typing from the inline agent input after the panel opens.
- Fixed Up/Down recall so the command bar and side-panel composer recover history even when the underlying input component consumes arrow keys. Render-time shell suggestion sync no longer overwrites persisted global input history.
- Restored history ghost suggestions from persisted submitted input when per-pane shell command history is incomplete, while keeping AI suggestions separate.
- Fixed fresh windows and Dock/global-hotkey reopen so they start with persisted command history instead of an empty command bar history.
- Moved app-wide command history into its own persistence file and hydrated input components during workspace construction, so a fresh app launch does not depend on the first render or a healthy layout session to restore history.
- Polished Agent-mode command bar text alignment and foreground styling so it lines up with Smart and Shell modes.
- Polished Settings and chrome controls: Skills rows now have clearer hierarchy and hover feedback, AI Routing rows are aligned with the card grid, Keyboard Shortcuts use separate keycaps per key, and titlebar controls now show shortcut-aware tooltips.
- Refined the Settings keycap typography and pane-scope picker, removed the misleading "local" pane label, and added configurable Control-Tab / Control-Shift-Tab tab cycling with Cmd-Shift-[ / ] fallbacks.
- Polished provider setup and pane scope controls for the beta. The provider list now shows provider icons with clear configured/unconfigured state, and the pane-scope picker uses matched frame geometry between the mode switcher and minimap.
- Fixed command palette dismissal focus. Pressing Escape now returns focus to the active terminal so Cmd-Shift-P can reopen the palette immediately without clicking the terminal first.
- Fixed the side agent-panel composer empty-state layout so the input keeps the same full-width shell even when no LLM provider is configured.
- Fixed right-panel composer history navigation so multiline drafts keep normal Up/Down cursor movement, while single-line prompts still support submitted-history recall.
- Improved markdown table rendering for complex prose tables in the agent panel by using a stable minimum table width with horizontal scrolling instead of collapsing all columns into a narrow wrapped stack.
- Fixed bottom command-bar history so it only recalls single-line entries. Multiline agent prompts no longer get injected into the single-line command bar and crash GPUI on Up/Down recall.
- Command palette (Cmd+Shift+P) with fuzzy search for every action
- Session sidebar showing your open tabs
- Four built-in terminal color themes — Flexoki Dark, Flexoki Light, Catppuccin Mocha, and Tokyo Night. Switch instantly from Settings, or set your default in config.toml.
