use con_agent::{
    is_dangerous, AgentEvent, AgentProvider, Conversation, Message, SkillRegistry,
    TerminalContext, ToolApprovalDecision,
};
use con_terminal::Grid;
use crossbeam_channel::{Receiver, Sender};
use std::path::Path;
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
    /// Incremental text token (streaming mode only — currently unused)
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
}

impl AgentHarness {
    pub fn new(config: &Config) -> Self {
        let skills = SkillRegistry::new();
        let (event_tx, event_rx) = crossbeam_channel::unbounded();

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
        }
    }

    pub fn events(&self) -> &Receiver<HarnessEvent> {
        &self.event_rx
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

        let is_ssh = std::env::var("SSH_CONNECTION").is_ok();
        let is_tmux = std::env::var("TMUX").is_ok();

        let git_branch = cwd.as_ref().and_then(|dir| {
            std::process::Command::new("git")
                .args(["rev-parse", "--abbrev-ref", "HEAD"])
                .current_dir(dir)
                .output()
                .ok()
                .filter(|o| o.status.success())
                .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        });

        TerminalContext {
            cwd,
            recent_output,
            last_command: grid.last_command.clone(),
            last_exit_code: grid.last_exit_code,
            git_branch,
            is_ssh,
            is_tmux,
            agents_md,
            skills: self.skills.names(),
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

        let harness_tx = self.event_tx.clone();
        let agent_config = self.config.clone();
        let conversation = self.conversation.clone();

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
                .send(&conv_snapshot, &context, agent_tx, approval_rx)
                .await
            {
                Ok(assistant_msg) => {
                    conversation
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .add_message(assistant_msg);
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
