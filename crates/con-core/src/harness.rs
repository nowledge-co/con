use con_agent::{
    AgentEvent, AgentProvider, Conversation, Message, SkillRegistry, TerminalContext,
};
use con_terminal::Grid;
use crossbeam_channel::{Receiver, Sender};
use std::path::Path;
use std::sync::Arc;
use tokio::runtime::Runtime;

use crate::config::Config;

/// Events from the harness to the UI
#[derive(Debug, Clone)]
pub enum HarnessEvent {
    Thinking,
    Token(String),
    Step(con_agent::conversation::AgentStep),
    ResponseComplete(Message),
    Error(String),
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
/// Owns a single shared tokio runtime for all async agent work.
pub struct AgentHarness {
    provider: AgentProvider,
    conversation: Conversation,
    skills: SkillRegistry,
    event_tx: Sender<HarnessEvent>,
    event_rx: Receiver<HarnessEvent>,
    runtime: Arc<Runtime>,
}

impl AgentHarness {
    pub fn new(config: &Config) -> Self {
        let provider = AgentProvider::new(config.agent.clone());
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
            provider,
            conversation: Conversation::new(),
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
            last_exit_code: None,
            git_branch,
            is_ssh,
            is_tmux,
            agents_md,
            skills: self.skills.names(),
        }
    }

    /// Send a natural language message to the agent.
    /// Spawns work on the shared tokio runtime instead of creating a new one per message.
    pub fn send_message(&mut self, content: String, context: TerminalContext) {
        let user_msg = Message::user(&content);
        self.conversation.add_message(user_msg);

        let harness_tx = self.event_tx.clone();
        let provider = AgentProvider::new(self.provider.config().clone());
        let conversation = self.conversation.clone();

        self.runtime.spawn(async move {
            let (agent_tx, agent_rx) = crossbeam_channel::unbounded();

            // Bridge AgentEvent -> HarnessEvent on a blocking task
            let htx = harness_tx.clone();
            let bridge = tokio::task::spawn_blocking(move || {
                while let Ok(event) = agent_rx.recv() {
                    let mapped = match event {
                        AgentEvent::Thinking => HarnessEvent::Thinking,
                        AgentEvent::Token(t) => HarnessEvent::Token(t),
                        AgentEvent::Step(s) => HarnessEvent::Step(s),
                        AgentEvent::Done(m) => HarnessEvent::ResponseComplete(m),
                        AgentEvent::Error(e) => HarnessEvent::Error(e),
                    };
                    if htx.send(mapped).is_err() {
                        break;
                    }
                }
            });

            match provider.send(&conversation, &context, agent_tx).await {
                Ok(_) => {}
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

    pub fn conversation(&self) -> &Conversation {
        &self.conversation
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
