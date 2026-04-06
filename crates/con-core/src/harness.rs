use con_agent::{
    AgentEvent, AgentProvider, Conversation, Message, PaneRequest, SkillRegistry, TerminalContext,
    TerminalExecRequest, ToolApprovalDecision, is_dangerous,
};
use crossbeam_channel::{Receiver, Sender};
use parking_lot::Mutex;
use std::collections::HashSet;
use std::future::Future;
use std::path::Path;
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
    skills_config: crate::config::SkillsConfig,
    skills: SkillRegistry,
    /// Last cwd used for skill scanning, to avoid redundant rescans.
    last_skills_cwd: Option<String>,
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
            skills_config: config.skills.clone(),
            skills: SkillRegistry::new(),
            last_skills_cwd: None,
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

    pub fn spawn_detached<F>(&self, future: F)
    where
        F: Future<Output = ()> + Send + 'static,
    {
        self.runtime.spawn(future);
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

    /// Build agent context from Ghostty terminal state.
    pub fn build_context_from_snapshot(
        &self,
        focused_pane_index: usize,
        focused_observation: &con_agent::context::PaneObservationFrame,
        focused_runtime: &con_agent::context::PaneRuntimeState,
        other_panes: Vec<con_agent::context::PaneSummary>,
    ) -> TerminalContext {
        let cwd = focused_observation.cwd.clone();

        let agents_md = cwd.as_ref().and_then(|dir| {
            let agents_path = Path::new(dir).join("AGENTS.md");
            std::fs::read_to_string(&agents_path).ok()
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

            // Include stat summary (always) + diff content (capped at ~8K chars
            // to keep the system prompt lean — large diffs drown model attention).
            const MAX_DIFF_CHARS: usize = 8_000;
            let mut result = stat;
            result.push('\n');
            if diff.len() <= MAX_DIFF_CHARS {
                result.push_str(&diff);
            } else {
                // Truncate at a line boundary near the char limit
                let truncation_point = diff[..MAX_DIFF_CHARS]
                    .rfind('\n')
                    .unwrap_or(MAX_DIFF_CHARS);
                result.push_str(&diff[..truncation_point]);
                result.push_str(&format!(
                    "\n... (diff truncated — {:.0}K of {:.0}K chars shown, see `git diff` for full)",
                    truncation_point as f64 / 1000.0,
                    diff.len() as f64 / 1000.0,
                ));
            }
            Some(result)
        });

        let project_structure = cwd.as_ref().and_then(|dir| {
            let output = std::process::Command::new("git")
                .args(["ls-files", "--cached", "--others", "--exclude-standard"])
                .current_dir(dir)
                .output()
                .ok()
                .filter(|o| o.status.success())
                .map(|o| String::from_utf8_lossy(&o.stdout).to_string());

            let listing = output.or_else(|| {
                std::process::Command::new("find")
                    .args([
                        ".",
                        "-maxdepth",
                        "3",
                        "-type",
                        "f",
                        "-not",
                        "-path",
                        "./.git/*",
                    ])
                    .current_dir(dir)
                    .output()
                    .ok()
                    .filter(|o| o.status.success())
                    .map(|o| String::from_utf8_lossy(&o.stdout).to_string())
            })?;

            if listing.trim().is_empty() {
                return None;
            }

            let file_lines: Vec<&str> = listing.lines().collect();
            let total = file_lines.len();
            if total > 200 {
                let truncated = file_lines[..200].join("\n");
                Some(format!("{}\n... ({} more files)", truncated, total - 200))
            } else {
                Some(file_lines.join("\n"))
            }
        });

        let ssh_host = focused_runtime.remote_host.clone();
        let focused_control = con_agent::control::PaneControlState::from_runtime(focused_runtime);

        TerminalContext {
            focused_pane_index,
            focused_hostname: focused_runtime.remote_host.clone(),
            focused_hostname_confidence: focused_runtime.remote_host_confidence,
            focused_hostname_source: focused_runtime.remote_host_source,
            focused_title: focused_observation.title.clone(),
            focused_pane_mode: focused_runtime.mode,
            focused_has_shell_integration: focused_observation.has_shell_integration,
            focused_shell_metadata_fresh: focused_runtime.shell_metadata_fresh,
            focused_runtime_stack: focused_runtime.scope_stack.clone(),
            focused_runtime_warnings: focused_runtime.warnings.clone(),
            focused_control,
            cwd,
            recent_output: focused_observation.recent_output.clone(),
            last_command: focused_observation.last_command.clone(),
            last_exit_code: focused_observation.last_exit_code,
            last_command_duration_secs: focused_observation.last_command_duration_secs,
            git_branch,
            ssh_host,
            tmux_session: focused_runtime.tmux_session.clone(),
            agents_md,
            skills: self.skills.summaries(),
            command_history: Vec::new(), // ghostty doesn't track command blocks
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
            let conv_snapshot = conversation.lock().clone();
            let provider = AgentProvider::new(agent_config);
            let request_start = std::time::Instant::now();

            const MAX_RETRIES: u32 = 3;
            let mut attempt = 0u32;

            loop {
                if cancelled.load(Ordering::Relaxed) {
                    break;
                }

                let (agent_tx, agent_rx) = crossbeam_channel::unbounded();

                // Bridge AgentEvent → HarnessEvent on a blocking thread.
                // Recreated per attempt so each stream gets a fresh channel.
                let htx = harness_tx.clone();
                let per_request_approval_tx = approval_tx.clone();
                let bridge = tokio::task::spawn_blocking(move || {
                    let bridge_start = std::time::Instant::now();
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
                            AgentEvent::Done(mut m) => {
                                let duration_ms = bridge_start.elapsed().as_millis() as u64;
                                m.duration_ms = Some(duration_ms);
                                HarnessEvent::ResponseComplete(m)
                            }
                            AgentEvent::Error(e) => HarnessEvent::Error(e),
                        };
                        if htx.send(mapped).is_err() {
                            break;
                        }
                    }
                });

                log::info!("[harness] provider.send() attempt {}", attempt + 1);
                let result = provider
                    .send(
                        &conv_snapshot,
                        &context,
                        agent_tx,
                        approval_rx.clone(),
                        terminal_exec_tx.clone(),
                        pane_tx.clone(),
                        cancelled.clone(),
                    )
                    .await;

                // Wait for bridge to drain before inspecting the result
                let _ = bridge.await;

                match result {
                    Ok(mut assistant_msg) => {
                        let duration_ms = request_start.elapsed().as_millis() as u64;
                        assistant_msg.duration_ms = Some(duration_ms);
                        log::info!(
                            "[harness] provider.send() completed: {} chars in {}ms",
                            assistant_msg.content.len(),
                            duration_ms,
                        );
                        let mut conv = conversation.lock();
                        conv.add_message(assistant_msg);
                        if let Err(e) = conv.save() {
                            log::warn!("Failed to auto-save conversation: {}", e);
                        }
                        break;
                    }
                    Err(e) => {
                        let msg = e.to_string();
                        if attempt < MAX_RETRIES && is_transient_error(&msg) {
                            let delay_ms = 1000 * 2u64.pow(attempt); // 1s, 2s, 4s
                            log::warn!(
                                "[harness] Transient error (attempt {}/{}), retrying in {}ms: {}",
                                attempt + 1,
                                MAX_RETRIES + 1,
                                delay_ms,
                                msg,
                            );
                            let _ = harness_tx.send(HarnessEvent::Error(format!(
                                "Retrying ({}/{})… {}",
                                attempt + 1,
                                MAX_RETRIES,
                                short_error(&msg),
                            )));
                            tokio::time::sleep(std::time::Duration::from_millis(delay_ms)).await;
                            attempt += 1;
                            continue;
                        }
                        log::error!("[harness] provider.send() failed: {}", msg);
                        let user_msg = friendly_error(&msg);
                        let _ = harness_tx.send(HarnessEvent::Error(user_msg));
                        break;
                    }
                }
            }

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

    pub fn display_label_for_user_message(&self, content: &str) -> Option<String> {
        let mut skills = self.skills.list();
        skills.sort_by_key(|skill| std::cmp::Reverse(skill.prompt_template.len()));

        for skill in skills {
            if content == skill.prompt_template {
                return Some(format!("/{}", skill.name));
            }

            let prefix = format!("{}\n\nAdditional context: ", skill.prompt_template);
            if let Some(rest) = content.strip_prefix(&prefix) {
                let args = rest.trim();
                if args.is_empty() {
                    return Some(format!("/{}", skill.name));
                }
                return Some(format!("/{} {}", skill.name, args));
            }
        }

        None
    }

    /// Scan for skills from configured filesystem paths.
    /// Only rescans if the cwd has changed since the last scan.
    /// Returns true if skills were (re)loaded.
    pub fn scan_skills(&mut self, cwd: &str) -> bool {
        if self.last_skills_cwd.as_deref() == Some(cwd) {
            return false;
        }
        self.last_skills_cwd = Some(cwd.to_string());
        let cwd_path = Path::new(cwd);
        let global_dirs = self.skills_config.resolved_global_paths();
        let project_dirs = self.skills_config.resolved_project_paths(cwd_path);
        let count = self.skills.scan(&global_dirs, &project_dirs);
        log::info!("Skill scan for {}: {} skill(s) found", cwd, count);
        true
    }

    /// Update skills config (e.g. after settings change).
    pub fn update_skills_config(&mut self, config: crate::config::SkillsConfig) {
        self.skills_config = config;
        // Force rescan on next cwd check
        self.last_skills_cwd = None;
    }

    pub fn config(&self) -> &con_agent::AgentConfig {
        &self.config
    }

    pub fn update_config(&mut self, config: con_agent::AgentConfig) {
        self.config = config;
    }

    pub fn set_auto_approve(&mut self, enabled: bool) {
        self.config.auto_approve_tools = enabled;
    }

    /// Display name for the active model (e.g. "claude-sonnet-4-6").
    pub fn active_model_name(&self) -> String {
        self.config
            .effective_model(&self.config.provider)
            .to_string()
    }

    pub fn skill_names(&self) -> Vec<String> {
        self.skills.names()
    }

    pub fn skill_summaries(&self) -> Vec<(String, String)> {
        self.skills.summaries()
    }
}

/// Shell builtins that won't appear as executables on $PATH.
/// POSIX + bash/zsh builtins that users commonly type as standalone commands.
const SHELL_BUILTINS: &[&str] = &[
    // POSIX required builtins
    "cd", "echo", "eval", "exec", "exit", "export", "readonly", "return", "set", "shift", "source",
    "test", "times", "trap", "type", "ulimit", "umask", "unset", "wait",
    // bash/zsh builtins commonly typed as commands
    "alias", "bg", "bind", "builtin", "caller", "command", "compgen", "complete", "declare", "dirs",
    "disown", "enable", "fc", "fg", "hash", "help", "history", "jobs", "let", "local", "logout",
    "popd", "pushd", "read", "shopt", "suspend", "typeset", "unalias",
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

/// Discover shell aliases by running `alias` in the user's login shell.
/// Cached on first call. Returns empty set on failure (no blocking, no panic).
fn shell_aliases() -> &'static HashSet<String> {
    static CACHE: OnceLock<HashSet<String>> = OnceLock::new();
    CACHE.get_or_init(|| {
        let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/bash".into());
        let output = std::process::Command::new(&shell)
            .args(["-ic", "alias"])
            .stderr(std::process::Stdio::null())
            .output()
            .ok();
        let mut aliases = HashSet::new();
        if let Some(out) = output {
            let text = String::from_utf8_lossy(&out.stdout);
            for line in text.lines() {
                // zsh: "name=value" or "name='value'"
                // bash: "alias name='value'"
                let line = line.trim();
                let name_part = if let Some(rest) = line.strip_prefix("alias ") {
                    rest
                } else {
                    line
                };
                if let Some((name, _)) = name_part.split_once('=') {
                    let name = name.trim();
                    if !name.is_empty() && is_command_shaped(name) {
                        aliases.insert(name.to_string());
                    }
                }
            }
        }
        log::info!("[classify] discovered {} shell aliases", aliases.len());
        aliases
    })
}

/// Returns true if a token looks like a valid command name:
/// lowercase alphanumeric, hyphens, underscores, dots — no spaces.
fn is_command_shaped(word: &str) -> bool {
    !word.is_empty()
        && word.len() <= 40
        && word.bytes().all(|b| {
            b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'_' || b == b'.'
        })
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

    // Shell builtins (these are never NL — "cd", "export", etc.)
    if SHELL_BUILTINS.contains(&first_word) {
        return true;
    }

    // Shell aliases (ll, la, gs, gst, etc.) — discovered from user's shell config
    if shell_aliases().contains(first_word) {
        return true;
    }

    // Executable on local $PATH — but only if the surrounding text
    // doesn't read like natural language. Many short English words
    // (`what`, `test`, `time`, `sort`, `make`) are also executables.
    if path_executables().contains(first_word) && !has_natural_language_signals(input) {
        return true;
    }

    // Explicit path invocation
    if first_word.starts_with("./") || first_word.starts_with('/') || first_word.starts_with("~/") {
        return true;
    }

    // Env var assignment: VAR=value or VAR=value command
    if first_word.contains('=') {
        let (name, _) = first_word.split_once('=').unwrap();
        if !name.is_empty()
            && name
                .bytes()
                .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'_')
        {
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

/// Detects natural-language syntax using structural signals only.
///
/// Uses three purely syntactic checks — no word lists:
/// 1. Sentence-ending punctuation: `?` or `.` at the end (shell commands never end this way)
/// 2. First word starts with uppercase (shell executables are lowercase)
/// 3. Contains clause-separating commas (`, ` — distinct from shell comma usage)
/// 4. Multi-word input with no shell argument structure (no flags, paths, or operators)
fn has_natural_language_signals(input: &str) -> bool {
    let trimmed = input.trim();

    // 1. Ends with sentence punctuation — commands never end with ? or .
    //    (note: shell `!` is history expansion, so we skip it)
    if trimmed.ends_with('?') || trimmed.ends_with('.') {
        return true;
    }

    // 2. First character is uppercase — shell commands are lowercase.
    //    Env var assignments (VAR=val) are caught earlier in the pipeline.
    if trimmed.starts_with(|c: char| c.is_ascii_uppercase()) {
        return true;
    }

    // 3. Contains clause-separating comma+space pattern.
    //    Shell uses commas in brace expansion {a,b} but not ", " between words.
    if trimmed.contains(", ") {
        return true;
    }

    // 4. Multi-word input where no token looks like a shell argument.
    //    Shell arguments are: flags (-x, --foo), paths (/foo, ./bar),
    //    env vars ($FOO), globs (*.txt), or quoted strings.
    //    If none of the tokens after the first have these patterns, it's likely prose.
    let words: Vec<&str> = trimmed.split_whitespace().collect();
    if words.len() >= 3 {
        let has_shell_args = words[1..].iter().any(|w| {
            w.starts_with('-')       // flag
                || w.starts_with('/')    // absolute path
                || w.starts_with("./")   // relative path
                || w.starts_with("~/")   // home path
                || w.starts_with('$')    // variable
                || w.contains('*')       // glob
                || w.contains('=')       // assignment
                || w.starts_with('"')    // quoted string
                || w.starts_with('\'') // quoted string
        });
        if !has_shell_args {
            return true;
        }
    }

    false
}

/// Classify errors as transient (worth retrying) vs permanent.
/// Produce a user-friendly error message from a verbose provider error.
/// Detects known failure patterns and translates them into actionable guidance.
fn friendly_error(msg: &str) -> String {
    // Empty tool_call_id: the streaming parser failed to parse the model's
    // tool call (missing name/id). This is a provider compatibility issue.
    if msg.contains("tool_call_id") && msg.contains("invalid") {
        return format!(
            "The model returned a malformed tool call (empty name or call_id). \
             This is a known compatibility issue with some providers' streaming format. \
             Try a different model, or retry the request.\n\nRaw error: {}",
            short_error_detail(msg),
        );
    }
    // ToolNotFoundError with empty name — same root cause
    if msg.contains("ToolNotFoundError") && msg.contains("\"\"") {
        return "The model returned a tool call with an empty name. \
                This usually means the provider's streaming format is incompatible. \
                Try a different model."
            .to_string();
    }
    msg.to_string()
}

/// Extract the JSON error body from a verbose provider error, if present.
fn short_error_detail(msg: &str) -> &str {
    // Look for the JSON portion: {"type":"error",...}
    if let Some(start) = msg.find("{\"type\":") {
        &msg[start..]
    } else {
        msg
    }
}

/// Rig surfaces HTTP errors as stringified messages, so we match on substrings.
fn is_transient_error(msg: &str) -> bool {
    // HTTP 429 rate limit
    msg.contains("429") || msg.contains("rate_limit") || msg.contains("Rate limit")
    // Server-side transient
    || msg.contains("500") || msg.contains("502") || msg.contains("503")
    || msg.contains("Internal Server Error") || msg.contains("Bad Gateway")
    || msg.contains("Service Unavailable") || msg.contains("overloaded")
    // Network transient
    || msg.contains("connection reset") || msg.contains("timed out")
    || msg.contains("Connection reset")
}

/// Extract a short, user-facing error summary from a verbose provider error.
fn short_error(msg: &str) -> &str {
    if msg.contains("429") || msg.contains("rate_limit") {
        "rate limited"
    } else if msg.contains("500") || msg.contains("Internal Server Error") {
        "server error (500)"
    } else if msg.contains("502") || msg.contains("Bad Gateway") {
        "bad gateway (502)"
    } else if msg.contains("503") || msg.contains("Service Unavailable") {
        "service unavailable (503)"
    } else if msg.contains("overloaded") {
        "provider overloaded"
    } else if msg.contains("timed out") {
        "request timed out"
    } else if msg.contains("connection reset") || msg.contains("Connection reset") {
        "connection reset"
    } else {
        "transient error"
    }
}
