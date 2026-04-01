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
    /// Git diff output (from `git diff --stat` + `git diff`, truncated)
    pub git_diff: Option<String>,
    /// Project file structure (truncated directory listing)
    pub project_structure: Option<String>,
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
            git_diff: None,
            project_structure: None,
        }
    }

    /// Build a system prompt enriched with terminal context.
    ///
    /// Uses XML tags for structured context injection — models parse these
    /// more reliably than plain text blocks, and it prevents context from
    /// being confused with user instructions.
    pub fn to_system_prompt(&self) -> String {
        let mut prompt = String::with_capacity(4096);

        prompt.push_str(
            "You are con, a terminal AI assistant. You help users with their terminal tasks.\n\
             You can execute shell commands, read/write files, and reason about the user's environment.\n\n\
             When executing commands, prefer terminal_exec over shell_exec. terminal_exec runs commands \
             visibly in the user's terminal so they can see what you're doing. Use shell_exec only for \
             background operations or when you need to suppress output.\n\n",
        );

        prompt.push_str("<terminal_context>\n");

        if let Some(cwd) = &self.cwd {
            prompt.push_str(&format!("<cwd>{}</cwd>\n", cwd));
        }

        if let Some(branch) = &self.git_branch {
            prompt.push_str(&format!("<git_branch>{}</git_branch>\n", branch));
        }

        if let Some(host) = &self.ssh_host {
            prompt.push_str(&format!("<ssh_host>{}</ssh_host>\n", host));
        }

        if let Some(session) = &self.tmux_session {
            prompt.push_str(&format!("<tmux_session>{}</tmux_session>\n", session));
        }

        if let Some(cmd) = &self.last_command {
            match self.last_exit_code {
                Some(code) => prompt.push_str(&format!(
                    "<last_command exit_code=\"{}\">{}</last_command>\n",
                    code, cmd
                )),
                None => prompt.push_str(&format!("<last_command>{}</last_command>\n", cmd)),
            }
        }

        if !self.command_history.is_empty() {
            prompt.push_str("<command_history>\n");
            for block in &self.command_history {
                match block.exit_code {
                    Some(code) => {
                        prompt.push_str(&format!("$ {} (exit {})\n", block.command, code))
                    }
                    None => prompt.push_str(&format!("$ {}\n", block.command)),
                }
            }
            prompt.push_str("</command_history>\n");
        }

        if !self.recent_output.is_empty() {
            prompt.push_str("<terminal_output>\n");
            for line in &self.recent_output {
                prompt.push_str(line);
                prompt.push('\n');
            }
            prompt.push_str("</terminal_output>\n");
        }

        if let Some(diff) = &self.git_diff {
            prompt.push_str("<git_diff>\n");
            prompt.push_str(diff);
            if !diff.ends_with('\n') {
                prompt.push('\n');
            }
            prompt.push_str("</git_diff>\n");
        }

        if let Some(structure) = &self.project_structure {
            prompt.push_str("<project_structure>\n");
            prompt.push_str(structure);
            if !structure.ends_with('\n') {
                prompt.push('\n');
            }
            prompt.push_str("</project_structure>\n");
        }

        prompt.push_str("</terminal_context>\n");

        if let Some(agents_md) = &self.agents_md {
            prompt.push_str("\n<agents_md>\n");
            prompt.push_str(agents_md);
            if !agents_md.ends_with('\n') {
                prompt.push('\n');
            }
            prompt.push_str("</agents_md>\n");
        }

        if !self.skills.is_empty() {
            prompt.push_str(&format!(
                "\nAvailable skills: {}\n",
                self.skills.join(", ")
            ));
        }

        prompt
    }
}
