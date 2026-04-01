use con_agent::{
    is_dangerous, AgentEvent, AgentProvider, Conversation, Message, SkillRegistry,
    TerminalContext, TerminalExecRequest, ToolApprovalDecision,
};
use con_terminal::Grid;
use crossbeam_channel::{Receiver, Sender};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
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

/// The agent harness — orchestrates agent <-> terminal interaction.
///
/// Owns a single shared tokio runtime for all async agent work.
/// Each `send_message()` call spawns a task on this runtime.
///
/// ## Channel architecture
///
/// - `event_tx/rx`: Harness → UI. All events (tool calls, responses, errors).
/// - Per-request approval channels: Created fresh for each `send_message()`.
///   The sender is delivered inside `ToolApprovalNeeded` events. The receiver
///   is owned by the `ConHook` for that request. This prevents cross-request
///   interference — only one hook reads from each channel.
///
/// ## Conversation state
///
/// `Arc<Mutex<Conversation>>` is shared between the main thread and
/// spawned tasks. The mutex is held briefly (clone snapshot, add message).
/// The async agent work operates on a snapshot, not under the lock.
pub struct AgentHarness {
    config: con_agent::AgentConfig,
    conversation: Arc<Mutex<Conversation>>,
    skills: SkillRegistry,
    event_tx: Sender<HarnessEvent>,
    event_rx: Receiver<HarnessEvent>,
    runtime: Arc<Runtime>,
    /// Channel for visible terminal execution requests (agent → workspace).
    /// The sender is cloned into TerminalExecTool instances.
    terminal_exec_tx: Sender<TerminalExecRequest>,
    terminal_exec_rx: Receiver<TerminalExecRequest>,
    /// Cancellation flag for the current agent request.
    /// Set to true to stop streaming. Reset on each new send_message().
    cancel_flag: Arc<AtomicBool>,
}

impl AgentHarness {
    pub fn new(config: &Config) -> Self {
        let skills = SkillRegistry::new();
        let (event_tx, event_rx) = crossbeam_channel::unbounded();
        let (terminal_exec_tx, terminal_exec_rx) = crossbeam_channel::unbounded();

        let runtime = Arc::new(
            tokio::runtime::Builder::new_multi_thread()
                .worker_threads(2)
                .enable_all()
                .build()
                .expect("Failed to create tokio runtime for agent harness"),
        );

        Self {
            config: config.agent.clone(),
            conversation: Arc::new(Mutex::new(Conversation::new())),
            skills,
            event_tx,
            event_rx,
            runtime,
            terminal_exec_tx,
            terminal_exec_rx,
            cancel_flag: Arc::new(AtomicBool::new(false)),
        }
    }

    pub fn events(&self) -> &Receiver<HarnessEvent> {
        &self.event_rx
    }

    /// Create a SuggestionEngine that shares the harness tokio runtime.
    pub fn suggestion_engine(&self, debounce_ms: u64) -> crate::suggestions::SuggestionEngine {
        crate::suggestions::SuggestionEngine::new(
            self.config.clone(),
            self.runtime.clone(),
            debounce_ms,
        )
    }

    /// Channel for terminal exec requests — the workspace polls this to
    /// execute agent commands in the visible terminal.
    pub fn terminal_exec_requests(&self) -> &Receiver<TerminalExecRequest> {
        &self.terminal_exec_rx
    }

    /// Classify user input: NLP, command, or skill
    pub fn classify_input(&self, input: &str) -> InputKind {
        let trimmed = input.trim();

        if trimmed.starts_with('/') {
            let parts: Vec<&str> = trimmed[1..].splitn(2, ' ').collect();
            let skill_name = parts[0].to_string();
            let args = parts.get(1).map(|s| s.to_string());
            if self.skills.get(&skill_name).is_some() {
                return InputKind::SkillInvoke(skill_name, args);
            }
        }

        if looks_like_command(trimmed) {
            return InputKind::ShellCommand(trimmed.to_string());
        }

        InputKind::NaturalLanguage(trimmed.to_string())
    }

    /// Build terminal context from the current grid state
    pub fn build_context(&self, grid: &Grid, cwd: Option<&str>) -> TerminalContext {
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
            git_diff,
            project_structure,
        }
    }

    /// Send a natural language message to the agent.
    ///
    /// Spawns an async task on the shared tokio runtime. The task:
    /// 1. Snapshots the conversation (brief lock)
    /// 2. Creates a per-request approval channel
    /// 3. Runs the agent with a ConHook that emits events
    /// 4. Adds the assistant response back to the conversation (brief lock)
    pub fn send_message(&mut self, content: String, context: TerminalContext) {
        let user_msg = Message::user(&content);
        self.conversation
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .add_message(user_msg);

        // Reset cancellation flag for new request
        self.cancel_flag.store(false, Ordering::Relaxed);

        let harness_tx = self.event_tx.clone();
        let agent_config = self.config.clone();
        let conversation = self.conversation.clone();
        let terminal_exec_tx = self.terminal_exec_tx.clone();
        let cancelled = self.cancel_flag.clone();

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

            let conv_snapshot = conversation
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .clone();

            let provider = AgentProvider::new(agent_config);

            match provider
                .send(&conv_snapshot, &context, agent_tx, approval_rx, terminal_exec_tx, cancelled)
                .await
            {
                Ok(assistant_msg) => {
                    let mut conv = conversation
                        .lock()
                        .unwrap_or_else(|e| e.into_inner());
                    conv.add_message(assistant_msg);
                    if let Err(e) = conv.save() {
                        log::warn!("Failed to auto-save conversation: {}", e);
                    }
                }
                Err(e) => {
                    let _ = harness_tx.send(HarnessEvent::Error(e.to_string()));
                }
            }

            let _ = bridge.await;
        });
    }

    /// Invoke a skill
    pub fn invoke_skill(
        &mut self,
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
        self.send_message(prompt, context);
        Some(skill.description.clone())
    }

    /// Load skills from AGENTS.md in the given directory
    pub fn load_agents_md(&mut self, dir: &Path) {
        let agents_path = dir.join("AGENTS.md");
        if agents_path.exists() {
            match self.skills.load_agents_md(&agents_path) {
                Ok(n) => {
                    log::info!("Loaded {} skills from AGENTS.md", n);
                    let _ = self
                        .event_tx
                        .send(HarnessEvent::SkillsUpdated(self.skills.names()));
                }
                Err(e) => {
                    log::warn!("Failed to load AGENTS.md: {}", e);
                }
            }
        }
    }

    /// Start a new conversation, saving the current one first
    pub fn new_conversation(&mut self) {
        let conv = self.conversation.lock().unwrap_or_else(|e| e.into_inner());
        if let Err(e) = conv.save() {
            log::warn!("Failed to save conversation: {}", e);
        }
        drop(conv);
        self.conversation = Arc::new(Mutex::new(Conversation::new()));
    }

    /// Restore a conversation by ID
    pub fn load_conversation(&mut self, id: &str) -> bool {
        match Conversation::load(id) {
            Ok(conv) => {
                // Save current first
                let current = self.conversation.lock().unwrap_or_else(|e| e.into_inner());
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

    /// Get the current conversation ID
    pub fn conversation_id(&self) -> String {
        self.conversation
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .id
            .clone()
    }

    /// Cancel the current agent request. The streaming loop will
    /// stop and return the partial response accumulated so far.
    pub fn cancel_current(&self) {
        self.cancel_flag.store(true, Ordering::Relaxed);
    }

    pub fn update_config(&mut self, config: con_agent::AgentConfig) {
        self.config = config;
    }

    pub fn conversation(&self) -> Arc<Mutex<Conversation>> {
        self.conversation.clone()
    }

    pub fn skill_names(&self) -> Vec<String> {
        self.skills.names()
    }
}

/// Heuristic to detect shell commands vs natural language
fn looks_like_command(input: &str) -> bool {
    let first_word = input.split_whitespace().next().unwrap_or("");

    const COMMANDS: &[&str] = &[
        "ls", "cd", "pwd", "cat", "echo", "grep", "find", "mkdir", "rm", "cp", "mv", "touch",
        "chmod", "chown", "curl", "wget", "git", "docker", "npm", "yarn", "pnpm", "cargo",
        "rustc", "python", "pip", "node", "go", "make", "cmake", "ssh", "scp", "rsync", "tar",
        "zip", "unzip", "brew", "apt", "yum", "pacman", "sudo", "kill", "ps", "top", "htop",
        "df", "du", "head", "tail", "less", "more", "sort", "uniq", "wc", "sed", "awk", "xargs",
        "env", "export", "which", "man", "vim", "nvim", "nano", "code", "open", "pbcopy",
        "pbpaste",
    ];

    if COMMANDS.contains(&first_word) {
        return true;
    }

    if first_word.starts_with("./")
        || first_word.starts_with('/')
        || first_word.starts_with("~/")
    {
        return true;
    }

    if input.contains(" | ")
        || input.contains(" > ")
        || input.contains(" >> ")
        || input.contains(" && ")
        || input.contains(" ; ")
    {
        return true;
    }

    false
}
