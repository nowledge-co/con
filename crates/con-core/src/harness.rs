use con_agent::{
    is_dangerous, AgentEvent, AgentProvider, Conversation, Message, PaneRequest, SkillRegistry,
    TerminalContext, TerminalExecRequest, ToolApprovalDecision,
};
use con_terminal::Grid;
use crossbeam_channel::{Receiver, Sender};
use std::collections::HashSet;
use std::path::Path;
use parking_lot::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};
use tokio::runtime::Runtime;

use crate::config::Config;

/// Events from the harness to the UI.
///
/// These flow over a crossbeam channel polled by the GPUI workspace.
/// All variants are Clone so the UI can cheaply forward them.
#[derive(Debug, Clone)]
pub enum HarnessEvent {
    /// Agent is preparing to call the model
    Thinking,
    /// Incremental extended thinking/reasoning text from the model
    ThinkingDelta(String),
    /// Incremental text token from streaming
    Token(String),
    /// Reasoning step (e.g. "anthropic:claude-sonnet-4-0")
    Step(con_agent::conversation::AgentStep),
    /// A tool is about to execute (safe tools proceed immediately)
    ToolCallStart {
        call_id: String,
        tool_name: String,
        args: String,
    },
    /// A dangerous tool needs user approval before executing.
    /// The UI should present an approval dialog and call
    /// `respond_to_approval()` with the decision.
    ToolApprovalNeeded {
        call_id: String,
        tool_name: String,
        args: String,
        /// Sender to deliver the approval decision back to the hook.
        /// This is per-request: each agent invocation gets its own channel.
        approval_tx: Sender<ToolApprovalDecision>,
    },
    /// Tool finished executing
    ToolCallComplete {
        call_id: String,
        tool_name: String,
        result: String,
    },
    /// Agent produced a final response
    ResponseComplete(Message),
    /// An error occurred
    Error(String),
    /// Skill registry changed
    SkillsUpdated(Vec<String>),
}

/// Classifies user input
#[derive(Debug)]
pub enum InputKind {
    NaturalLanguage(String),
    ShellCommand(String),
    SkillInvoke(String, Option<String>),
}

// ---------------------------------------------------------------------------
// AgentSession — per-tab conversation state and channels
// ---------------------------------------------------------------------------

/// Per-tab agent session. Owns the conversation and channels for one tab's
/// agent interactions. Lightweight: no runtime, no config, no skills.
pub struct AgentSession {
    conversation: Arc<Mutex<Conversation>>,
    event_tx: Sender<HarnessEvent>,
    event_rx: Receiver<HarnessEvent>,
    terminal_exec_tx: Sender<TerminalExecRequest>,
    terminal_exec_rx: Receiver<TerminalExecRequest>,
    pane_tx: Sender<PaneRequest>,
    pane_rx: Receiver<PaneRequest>,
    cancel_flag: Arc<AtomicBool>,
}

impl AgentSession {
    /// Create a fresh session with a new conversation.
    pub fn new() -> Self {
        let (event_tx, event_rx) = crossbeam_channel::unbounded();
        let (terminal_exec_tx, terminal_exec_rx) = crossbeam_channel::unbounded();
        let (pane_tx, pane_rx) = crossbeam_channel::unbounded();
        Self {
            conversation: Arc::new(Mutex::new(Conversation::new())),
            event_tx,
            event_rx,
            terminal_exec_tx,
            terminal_exec_rx,
            pane_tx,
            pane_rx,
            cancel_flag: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Create a session from a loaded conversation.
    pub fn with_conversation(conv: Conversation) -> Self {
        let mut s = Self::new();
        s.conversation = Arc::new(Mutex::new(conv));
        s
    }

    pub fn events(&self) -> &Receiver<HarnessEvent> {
        &self.event_rx
    }

    pub fn terminal_exec_requests(&self) -> &Receiver<TerminalExecRequest> {
        &self.terminal_exec_rx
    }

    pub fn pane_requests(&self) -> &Receiver<PaneRequest> {
        &self.pane_rx
    }

    pub fn conversation_id(&self) -> String {
        self.conversation.lock().id.clone()
    }

    pub fn cancel_current(&self) {
        self.cancel_flag.store(true, Ordering::Relaxed);
    }

    pub fn conversation(&self) -> Arc<Mutex<Conversation>> {
        self.conversation.clone()
    }

    /// Start a new conversation, saving the current one first.
    pub fn new_conversation(&mut self) {
        let conv = self.conversation.lock();
        if let Err(e) = conv.save() {
            log::warn!("Failed to save conversation: {}", e);
        }
        drop(conv);
        self.conversation = Arc::new(Mutex::new(Conversation::new()));
    }

    /// Restore a conversation by ID, saving the current one first.
    pub fn load_conversation(&mut self, id: &str) -> bool {
        match Conversation::load(id) {
            Ok(conv) => {
                let current = self.conversation.lock();
                let _ = current.save();
                drop(current);
                self.conversation = Arc::new(Mutex::new(conv));
                true
            }
            Err(e) => {
                log::warn!("Failed to load conversation {}: {}", id, e);
                false
            }
        }
    }
}

// ---------------------------------------------------------------------------
// AgentHarness — shared infrastructure (1 per window)
// ---------------------------------------------------------------------------

/// Shared agent infrastructure. Owns the tokio runtime, config, and skills.
/// Does NOT own conversation or channels — those are per-tab in AgentSession.
pub struct AgentHarness {
    config: con_agent::AgentConfig,
    skills: SkillRegistry,
    runtime: Arc<Runtime>,
}

impl AgentHarness {
    pub fn new(config: &Config) -> anyhow::Result<Self> {
        let runtime = Arc::new(
            tokio::runtime::Builder::new_multi_thread()
                .worker_threads(2)
                .enable_all()
                .build()?,
        );

        Ok(Self {
            config: config.agent.clone(),
            skills: SkillRegistry::new(),
            runtime,
        })
    }

    /// Create a SuggestionEngine that shares the harness tokio runtime.
    pub fn suggestion_engine(&self, debounce_ms: u64) -> crate::suggestions::SuggestionEngine {
        crate::suggestions::SuggestionEngine::new(
            self.config.suggestion_agent_config(),
            self.runtime.clone(),
            debounce_ms,
        )
    }

    /// Classify user input: NLP, command, or skill.
    ///
    /// `is_remote` should be true when the focused pane is an SSH session,
    /// enabling more permissive command detection for remote executables
    /// that aren't on the local $PATH.
    pub fn classify_input(&self, input: &str, is_remote: bool) -> InputKind {
        let trimmed = input.trim();

        if trimmed.starts_with('/') {
            let parts: Vec<&str> = trimmed[1..].splitn(2, ' ').collect();
            let skill_name = parts[0].to_string();
            let args = parts.get(1).map(|s| s.to_string());
            if self.skills.get(&skill_name).is_some() {
                return InputKind::SkillInvoke(skill_name, args);
            }
        }

        if looks_like_command(trimmed, is_remote) {
            return InputKind::ShellCommand(trimmed.to_string());
        }

        InputKind::NaturalLanguage(trimmed.to_string())
    }

    /// Build terminal context from the current grid state.
    /// `other_panes` contains summaries of non-focused panes in the same tab.
    pub fn build_context(
        &self,
        grid: &Grid,
        cwd: Option<&str>,
        focused_pane_index: usize,
        focused_hostname: Option<String>,
        other_panes: Vec<con_agent::context::PaneSummary>,
    ) -> TerminalContext {
        let recent_output = grid.content_lines(50);
        let cwd = cwd
            .map(|s| s.to_string())
            .or_else(|| grid.current_dir.clone());

        let agents_md = cwd.as_ref().and_then(|dir| {
            let agents_path = Path::new(dir).join("AGENTS.md");
            std::fs::read_to_string(&agents_path).ok()
        });

        // Parse SSH_CONNECTION for remote host IP (format: "client_ip client_port server_ip server_port")
        let ssh_host = std::env::var("SSH_CONNECTION")
            .ok()
            .and_then(|val| val.split_whitespace().next().map(|s| s.to_string()));

        // Parse TMUX for session name (format: "/tmp/tmux-uid/default,pid,index")
        let tmux_session = std::env::var("TMUX").ok().and_then(|val| {
            // Get the session name via tmux command, falling back to socket path parsing
            std::process::Command::new("tmux")
                .args(["display-message", "-p", "#S"])
                .output()
                .ok()
                .filter(|o| o.status.success())
                .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
                .or_else(|| {
                    // Fallback: extract from socket path
                    val.split(',').next().and_then(|path| {
                        std::path::Path::new(path)
                            .file_name()
                            .map(|n| n.to_string_lossy().to_string())
                    })
                })
        });

        let git_branch = cwd.as_ref().and_then(|dir| {
            std::process::Command::new("git")
                .args(["rev-parse", "--abbrev-ref", "HEAD"])
                .current_dir(dir)
                .output()
                .ok()
                .filter(|o| o.status.success())
                .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        });

        let command_history: Vec<con_agent::context::CommandBlockInfo> = grid
            .command_blocks
            .iter()
            .rev()
            .take(10)
            .rev()
            .map(|block| con_agent::context::CommandBlockInfo {
                command: block.command.clone(),
                exit_code: block.exit_code,
            })
            .collect();

        // Gather git diff (stat + diff, truncated to avoid bloating context)
        let git_diff = cwd.as_ref().and_then(|dir| {
            let stat = std::process::Command::new("git")
                .args(["diff", "--stat"])
                .current_dir(dir)
                .output()
                .ok()
                .filter(|o| o.status.success())
                .map(|o| String::from_utf8_lossy(&o.stdout).to_string())
                .unwrap_or_default();

            if stat.trim().is_empty() {
                return None;
            }

            let diff = std::process::Command::new("git")
                .args(["diff"])
                .current_dir(dir)
                .output()
                .ok()
                .filter(|o| o.status.success())
                .map(|o| String::from_utf8_lossy(&o.stdout).to_string())
                .unwrap_or_default();

            // Truncate diff to ~2000 lines to keep context manageable
            let diff_lines: Vec<&str> = diff.lines().collect();
            let was_truncated = diff_lines.len() > 2000;

            let mut result = stat;
            result.push('\n');
            result.push_str(&diff_lines[..diff_lines.len().min(2000)].join("\n"));
            if was_truncated {
                result.push_str("\n... (truncated)");
            }

            Some(result)
        });

        // Gather project file structure (shallow directory listing)
        let project_structure = cwd.as_ref().and_then(|dir| {
            // Use `git ls-files` if in a repo (respects .gitignore), fall back to find
            let output = std::process::Command::new("git")
                .args(["ls-files", "--cached", "--others", "--exclude-standard"])
                .current_dir(dir)
                .output()
                .ok()
                .filter(|o| o.status.success())
                .map(|o| String::from_utf8_lossy(&o.stdout).to_string());

            let listing = output.or_else(|| {
                std::process::Command::new("find")
                    .args([".", "-maxdepth", "3", "-type", "f", "-not", "-path", "./.git/*"])
                    .current_dir(dir)
                    .output()
                    .ok()
                    .filter(|o| o.status.success())
                    .map(|o| String::from_utf8_lossy(&o.stdout).to_string())
            })?;

            if listing.trim().is_empty() {
                return None;
            }

            // Truncate to 200 files
            let file_lines: Vec<&str> = listing.lines().collect();
            let total = file_lines.len();

            if total > 200 {
                let truncated = file_lines[..200].join("\n");
                Some(format!("{}\n... ({} more files)", truncated, total - 200))
            } else {
                Some(file_lines.join("\n"))
            }
        });

        TerminalContext {
            focused_pane_index,
            focused_hostname,
            cwd,
            recent_output,
            last_command: grid.last_command.clone(),
            last_exit_code: grid.last_exit_code,
            git_branch,
            ssh_host,
            tmux_session,
            agents_md,
            skills: self.skills.names(),
            command_history,
            other_panes,
            git_diff,
            project_structure,
        }
    }

    /// Send a natural language message to the agent using a specific tab's session.
    ///
    /// The session provides the conversation and channels;
    /// the harness provides runtime and config.
    pub fn send_message(&self, session: &AgentSession, content: String, context: TerminalContext) {
        let user_msg = Message::user(&content);
        session.conversation.lock().add_message(user_msg);

        // Reset cancellation flag for new request
        session.cancel_flag.store(false, Ordering::Relaxed);

        let harness_tx = session.event_tx.clone();
        let agent_config = self.config.clone();
        let conversation = session.conversation.clone();
        let terminal_exec_tx = session.terminal_exec_tx.clone();
        let pane_tx = session.pane_tx.clone();
        let cancelled = session.cancel_flag.clone();

        // Per-request approval channel: the sender goes to the UI via
        // ToolApprovalNeeded events, the receiver goes to the ConHook.
        // This ensures no cross-request interference.
        let (approval_tx, approval_rx) = crossbeam_channel::unbounded();

        self.runtime.spawn(async move {
            let (agent_tx, agent_rx) = crossbeam_channel::unbounded();

            // Bridge AgentEvent → HarnessEvent on a blocking thread.
            // The bridge terminates when agent_tx is dropped (provider.send returns).
            let htx = harness_tx.clone();
            let per_request_approval_tx = approval_tx.clone();
            let bridge = tokio::task::spawn_blocking(move || {
                while let Ok(event) = agent_rx.recv() {
                    let mapped = match event {
                        AgentEvent::Thinking => HarnessEvent::Thinking,
                        AgentEvent::ThinkingDelta(t) => HarnessEvent::ThinkingDelta(t),
                        AgentEvent::Token(t) => HarnessEvent::Token(t),
                        AgentEvent::Step(s) => HarnessEvent::Step(s),
                        AgentEvent::ToolCallStart {
                            call_id,
                            tool_name,
                            args,
                        } => {
                            if is_dangerous(&tool_name) {
                                let _ = htx.send(HarnessEvent::ToolApprovalNeeded {
                                    call_id: call_id.clone(),
                                    tool_name: tool_name.clone(),
                                    args: args.clone(),
                                    approval_tx: per_request_approval_tx.clone(),
                                });
                            }
                            HarnessEvent::ToolCallStart {
                                call_id,
                                tool_name,
                                args,
                            }
                        }
                        AgentEvent::ToolCallComplete {
                            call_id,
                            tool_name,
                            result,
                        } => HarnessEvent::ToolCallComplete {
                            call_id,
                            tool_name,
                            result,
                        },
                        AgentEvent::Done(m) => HarnessEvent::ResponseComplete(m),
                        AgentEvent::Error(e) => HarnessEvent::Error(e),
                    };
                    if htx.send(mapped).is_err() {
                        break;
                    }
                }
            });

            let conv_snapshot = conversation.lock().clone();

            let provider = AgentProvider::new(agent_config);

            log::info!("[harness] Calling provider.send()");
            match provider
                .send(&conv_snapshot, &context, agent_tx, approval_rx, terminal_exec_tx, pane_tx, cancelled)
                .await
            {
                Ok(assistant_msg) => {
                    log::info!(
                        "[harness] provider.send() completed: {} chars",
                        assistant_msg.content.len()
                    );
                    let mut conv = conversation.lock();
                    conv.add_message(assistant_msg);
                    if let Err(e) = conv.save() {
                        log::warn!("Failed to auto-save conversation: {}", e);
                    }
                }
                Err(e) => {
                    log::error!("[harness] provider.send() failed: {}", e);
                    let _ = harness_tx.send(HarnessEvent::Error(e.to_string()));
                }
            }

            log::info!("[harness] Waiting for bridge thread to finish");
            let _ = bridge.await;
            log::info!("[harness] Request complete");
        });
    }

    /// Invoke a skill using the active tab's session.
    pub fn invoke_skill(
        &self,
        session: &AgentSession,
        skill_name: &str,
        args: Option<&str>,
        context: TerminalContext,
    ) -> Option<String> {
        let skill = self.skills.get(skill_name)?.clone();
        let prompt = if let Some(args) = args {
            format!("{}\n\nAdditional context: {}", skill.prompt_template, args)
        } else {
            skill.prompt_template.clone()
        };
        self.send_message(session, prompt, context);
        Some(skill.description.clone())
    }

    /// Load skills from AGENTS.md in the given directory.
    /// Returns the updated skill names list if loading succeeded.
    pub fn load_agents_md(&mut self, dir: &Path) -> Option<Vec<String>> {
        let agents_path = dir.join("AGENTS.md");
        if agents_path.exists() {
            match self.skills.load_agents_md(&agents_path) {
                Ok(n) => {
                    log::info!("Loaded {} skills from AGENTS.md", n);
                    Some(self.skills.names())
                }
                Err(e) => {
                    log::warn!("Failed to load AGENTS.md: {}", e);
                    None
                }
            }
        } else {
            None
        }
    }

    pub fn update_config(&mut self, config: con_agent::AgentConfig) {
        self.config = config;
    }

    pub fn set_auto_approve(&mut self, enabled: bool) {
        self.config.auto_approve_tools = enabled;
    }

    pub fn skill_names(&self) -> Vec<String> {
        self.skills.names()
    }
}

/// Shell builtins that won't appear as executables on $PATH.
/// POSIX + bash/zsh builtins that users commonly type as standalone commands.
const SHELL_BUILTINS: &[&str] = &[
    // POSIX required builtins
    "cd", "echo", "eval", "exec", "exit", "export", "readonly", "return", "set", "shift",
    "source", "test", "times", "trap", "type", "ulimit", "umask", "unset", "wait",
    // bash/zsh builtins commonly typed as commands
    "alias", "bg", "bind", "builtin", "caller", "command", "compgen", "complete", "declare",
    "dirs", "disown", "enable", "fc", "fg", "hash", "help", "history", "jobs", "let", "local",
    "logout", "popd", "pushd", "read", "shopt", "suspend", "typeset", "unalias",
];

/// Scan $PATH once and cache the set of executable names.
fn path_executables() -> &'static HashSet<String> {
    static CACHE: OnceLock<HashSet<String>> = OnceLock::new();
    CACHE.get_or_init(|| {
        let path_var = std::env::var("PATH").unwrap_or_default();
        let mut exes = HashSet::with_capacity(2048);
        for dir in std::env::split_paths(&path_var) {
            if let Ok(entries) = std::fs::read_dir(&dir) {
                for entry in entries.filter_map(|e| e.ok()) {
                    if let Some(name) = entry.file_name().to_str() {
                        exes.insert(name.to_string());
                    }
                }
            }
        }
        exes
    })
}

/// Returns true if a token looks like a valid command name:
/// lowercase alphanumeric, hyphens, underscores, dots — no spaces.
fn is_command_shaped(word: &str) -> bool {
    !word.is_empty()
        && word.len() <= 40
        && word
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'_' || b == b'.')
}

/// Detect whether input is a shell command or natural language.
///
/// Uses structural signals — no static word list:
/// 1. First token is an executable on $PATH or a shell builtin
/// 2. First token is a path (`./`, `/`, `~/`)
/// 3. Input contains shell operators (`|`, `>`, `>>`, `&&`, `;`)
/// 4. Input contains flag-like arguments (`-x`, `--foo`) with a command-shaped first word
/// 5. First token is an env var assignment (`VAR=value`)
/// 6. Input contains subshell/expansion syntax (`$(...)`, backticks)
///
/// When `is_remote` is true (focused pane is an SSH session), the classification
/// is more permissive: a command-shaped first word alone is enough, since remote
/// executables aren't on the local $PATH. Natural-language signals (question words,
/// articles, pronouns) override this to prevent false positives.
fn looks_like_command(input: &str, is_remote: bool) -> bool {
    let mut words = input.split_whitespace();
    let first_word = match words.next() {
        Some(w) => w,
        None => return false,
    };

    // --- Definitive structural signals (always apply) ---

    // Shell builtins
    if SHELL_BUILTINS.contains(&first_word) {
        return true;
    }

    // Executable on local $PATH
    if path_executables().contains(first_word) {
        return true;
    }

    // Explicit path invocation
    if first_word.starts_with("./")
        || first_word.starts_with('/')
        || first_word.starts_with("~/")
    {
        return true;
    }

    // Env var assignment: VAR=value or VAR=value command
    if first_word.contains('=') {
        let (name, _) = first_word.split_once('=').unwrap();
        if !name.is_empty() && name.bytes().all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'_') {
            return true;
        }
    }

    // Shell operators indicate a pipeline/compound command
    if input.contains(" | ")
        || input.contains(" > ")
        || input.contains(" >> ")
        || input.contains(" && ")
        || input.contains(" ; ")
    {
        return true;
    }

    // Subshell / expansion syntax
    if input.contains("$(") || input.contains('`') {
        return true;
    }

    // Flag arguments with a command-shaped first word:
    // "free -g", "docker --version" — NL never uses -flags
    if is_command_shaped(first_word) {
        let has_flags = words.clone().any(|w| w.starts_with('-'));
        if has_flags {
            return true;
        }
    }

    // --- Remote-aware classification ---
    // On SSH sessions, remote executables aren't on local $PATH.
    // A command-shaped first word is likely a remote command — unless
    // the input reads like natural language.
    if is_remote && is_command_shaped(first_word) {
        if !has_natural_language_signals(input) {
            return true;
        }
    }

    false
}

/// Detects natural-language signals that distinguish NL from short commands.
/// Used as a negative signal: if these are present, the input is likely NL
/// even if the first word looks command-shaped.
fn has_natural_language_signals(input: &str) -> bool {
    let lower = input.to_ascii_lowercase();
    let words: Vec<&str> = lower.split_whitespace().collect();

    // Question patterns (first word or second word after a command-like word)
    const QUESTION_WORDS: &[&str] = &[
        "what", "how", "why", "when", "where", "which", "who", "is", "are", "can",
        "could", "would", "should", "does", "do", "will", "explain", "describe",
        "tell", "show", "help", "please",
    ];
    if let Some(first) = words.first() {
        if QUESTION_WORDS.contains(first) {
            return true;
        }
    }

    // Articles and pronouns — strong NL signal when present anywhere
    const NL_MARKERS: &[&str] = &[
        "the", "a", "an", "this", "that", "these", "those",
        "i", "me", "my", "you", "your", "we", "our",
        "about", "with", "from", "into",
    ];
    if words.iter().any(|w| NL_MARKERS.contains(w)) {
        return true;
    }

    false
}
