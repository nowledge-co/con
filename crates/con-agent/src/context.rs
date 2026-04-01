use serde::{Deserialize, Serialize};

/// Terminal context extracted for the AI agent.
/// This is what makes con's agent smarter than a generic chatbot —
/// it always knows what the user is doing in their terminal.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TerminalContext {
    /// Current working directory (from OSC 7 or manual detection)
    pub cwd: Option<String>,
    /// Last N lines of terminal output
    pub recent_output: Vec<String>,
    /// Last command executed (from OSC 133 or heuristic)
    pub last_command: Option<String>,
    /// Last exit code
    pub last_exit_code: Option<i32>,
    /// Git branch if in a repo
    pub git_branch: Option<String>,
    /// SSH remote host (parsed from SSH_CONNECTION), if in an SSH session
    pub ssh_host: Option<String>,
    /// tmux session name, if inside tmux
    pub tmux_session: Option<String>,
    /// Contents of AGENTS.md in the cwd (if present)
    pub agents_md: Option<String>,
    /// Available skills
    pub skills: Vec<String>,
    /// Recent command blocks from OSC 133 shell integration
    pub command_history: Vec<CommandBlockInfo>,
}

/// A completed command block from OSC 133 shell integration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommandBlockInfo {
    pub command: String,
    pub exit_code: Option<i32>,
}

impl TerminalContext {
    pub fn empty() -> Self {
        Self {
            cwd: None,
            recent_output: Vec::new(),
            last_command: None,
            last_exit_code: None,
            git_branch: None,
            ssh_host: None,
            tmux_session: None,
            agents_md: None,
            skills: Vec::new(),
            command_history: Vec::new(),
        }
    }

    /// Build a system prompt enriched with terminal context
    pub fn to_system_prompt(&self) -> String {
        let mut parts = vec![
            "You are con, a terminal AI assistant. You help users with their terminal tasks.".to_string(),
            "You can execute shell commands, read/write files, and reason about the user's environment.".to_string(),
        ];

        if let Some(cwd) = &self.cwd {
            parts.push(format!("Current directory: {}", cwd));
        }

        if let Some(branch) = &self.git_branch {
            parts.push(format!("Git branch: {}", branch));
        }

        if let Some(host) = &self.ssh_host {
            parts.push(format!("Connected via SSH to {}", host));
        }

        if let Some(session) = &self.tmux_session {
            parts.push(format!("Inside tmux session '{}'", session));
        }

        if let Some(agents_md) = &self.agents_md {
            parts.push(format!(
                "The project has an AGENTS.md with these instructions:\n{}",
                agents_md
            ));
        }

        if !self.skills.is_empty() {
            parts.push(format!("Available skills: {}", self.skills.join(", ")));
        }

        if !self.command_history.is_empty() {
            let history: Vec<String> = self
                .command_history
                .iter()
                .map(|block| {
                    match block.exit_code {
                        Some(code) => format!("$ {} (exit {})", block.command, code),
                        None => format!("$ {}", block.command),
                    }
                })
                .collect();
            parts.push(format!(
                "Recent command history:\n{}",
                history.join("\n")
            ));
        }

        if !self.recent_output.is_empty() {
            let output = self.recent_output.join("\n");
            parts.push(format!(
                "Recent terminal output:\n```\n{}\n```",
                output
            ));
        }

        parts.join("\n\n")
    }
}
