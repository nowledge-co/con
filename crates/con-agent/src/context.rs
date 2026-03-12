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
    /// Whether the user is in an SSH session
    pub is_ssh: bool,
    /// Whether the user is in tmux
    pub is_tmux: bool,
    /// Contents of AGENTS.md in the cwd (if present)
    pub agents_md: Option<String>,
    /// Available skills
    pub skills: Vec<String>,
}

impl TerminalContext {
    pub fn empty() -> Self {
        Self {
            cwd: None,
            recent_output: Vec::new(),
            last_command: None,
            last_exit_code: None,
            git_branch: None,
            is_ssh: false,
            is_tmux: false,
            agents_md: None,
            skills: Vec::new(),
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

        if self.is_ssh {
            parts.push("User is in an SSH session.".to_string());
        }

        if self.is_tmux {
            parts.push("User is in a tmux session.".to_string());
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
