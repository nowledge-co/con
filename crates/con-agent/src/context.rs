use serde::{Deserialize, Serialize};

/// Terminal context extracted for the AI agent.
/// This is what makes con's agent smarter than a generic chatbot —
/// it always knows what the user is doing in their terminal.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TerminalContext {
    /// 1-based index of the focused pane
    pub focused_pane_index: usize,
    /// Remote hostname of the focused pane (None if local)
    pub focused_hostname: Option<String>,
    /// Current working directory (from OSC 7 or manual detection)
    pub cwd: Option<String>,
    /// Last N lines of terminal output
    pub recent_output: Vec<String>,
    /// Last command executed (from OSC 133 or heuristic)
    pub last_command: Option<String>,
    /// Last exit code
    pub last_exit_code: Option<i32>,
    /// Last command duration in seconds (from ghostty COMMAND_FINISHED)
    pub last_command_duration_secs: Option<f64>,
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
    /// Other (non-focused) panes in the current tab.
    /// Empty when there is only one pane.
    pub other_panes: Vec<PaneSummary>,
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

/// Summary of a non-focused terminal pane's state.
/// Kept intentionally small to avoid bloating the system prompt.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaneSummary {
    pub pane_index: usize,
    /// Remote hostname if this is an SSH session, None if local.
    pub hostname: Option<String>,
    pub cwd: Option<String>,
    pub last_command: Option<String>,
    pub last_exit_code: Option<i32>,
    pub is_busy: bool,
    /// Last ~10 lines of visible output
    pub recent_output: Vec<String>,
}

impl TerminalContext {
    pub fn empty() -> Self {
        Self {
            focused_pane_index: 1,
            focused_hostname: None,
            cwd: None,
            recent_output: Vec::new(),
            last_command: None,
            last_exit_code: None,
            last_command_duration_secs: None,
            git_branch: None,
            ssh_host: None,
            tmux_session: None,
            agents_md: None,
            skills: Vec::new(),
            command_history: Vec::new(),
            other_panes: Vec::new(),
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
            "You are con, a terminal AI assistant with full access to the user's terminal environment.\n\n\
             ## Decision framework\n\
             For QUESTIONS about code, errors, or terminal state: prefer reading files and panes. Minimize side effects.\n\
             For TASKS that modify state: verify context first, explain what you will do, then execute carefully.\n\n\
             ## Tools\n\n\
             <tools>\n\
             - terminal_exec: Run a command visibly in any pane. Use pane_index to target a specific pane.\n\
               ALWAYS use absolute paths for executables when possible.\n\
               Check exit_code in the response — 0 means success, non-zero means failure.\n\
               If exit_code is null, shell integration may be absent or the command is still running.\n\n\
             - batch_exec: Run commands on MULTIPLE panes in PARALLEL. Fastest for multi-pane tasks\n\
               (e.g., \"check uptime on all servers\"). Returns results for each pane independently.\n\n\
             - shell_exec: Run a command in a hidden subprocess. Output is NOT shown to the user.\n\
               Prefer over terminal_exec for: git status, file searches, package lookups, background ops.\n\n\
             - list_panes: List all open panes with index, title, cwd, dimensions, hostname, shell integration status.\n\n\
             - read_pane: Read last N lines from any pane (includes scrollback). Use to inspect output.\n\n\
             - send_keys: Send raw keystrokes to any pane. For TUI interaction: Ctrl-C (\\x03), arrows, Enter.\n\n\
             - search_panes: Search scrollback across all panes by regex. Find previous errors, output, etc.\n\n\
             - file_read: Read a file. Supports line ranges (start_line, end_line). Read before editing.\n\
             - file_write: Write a file. Creates parent directories. Read the file first if it exists.\n\
             - edit_file: Surgical text replacement. old_text must match EXACTLY and be UNIQUE in the file.\n\n\
             - list_files: List files in a directory. Respects .gitignore. Max 500 entries.\n\
             - search: Search file contents by regex pattern. Returns file:line:match triples.\n\
             </tools>\n\n\
             ## Multi-pane awareness\n\
             You have access to ALL terminal panes, not just the focused one.\n\
             The <panes> section shows every open pane with its index, hostname, and cwd.\n\
             Use batch_exec to cover multiple relevant panes in parallel.\n\n\
             <safety>\n\
             - NEVER execute rm -rf, DROP TABLE, or destructive commands without explicit user confirmation.\n\
             - Check is_alive before executing — false means PTY exited, commands will fail.\n\
             - If is_busy is true, a command is already running — wait or use a different pane.\n\
             - On SSH panes (hostname != null): commands execute on the REMOTE host, not locally.\n\
             - If a command fails (exit_code != 0), diagnose the error before retrying.\n\
             - When editing files: always read first, ensure old_text is unique, verify the edit succeeded.\n\
             </safety>\n\n",
        );

        prompt.push_str("<terminal_context>\n");

        let total_panes = 1 + self.other_panes.len();

        // When multiple panes are open, embed a full pane layout so the agent
        // can target the right pane(s) without needing to call list_panes first.
        if total_panes > 1 {
            prompt.push_str("<panes>\n");
            // Focused pane
            let host_label = self
                .focused_hostname
                .as_deref()
                .unwrap_or("local");
            let cwd_label = self.cwd.as_deref().unwrap_or("?");
            prompt.push_str(&format!(
                "  <pane index=\"{}\" focused=\"true\" host=\"{}\" cwd=\"{}\"/>\n",
                self.focused_pane_index, host_label, cwd_label
            ));
            // Other panes
            for pane in &self.other_panes {
                let host = pane.hostname.as_deref().unwrap_or("local");
                let cwd = pane.cwd.as_deref().unwrap_or("?");
                let busy = if pane.is_busy { " busy=\"true\"" } else { "" };
                prompt.push_str(&format!(
                    "  <pane index=\"{}\" host=\"{}\" cwd=\"{}\"{}/>\n",
                    pane.pane_index, host, cwd, busy
                ));
            }
            prompt.push_str("</panes>\n");
        } else if let Some(cwd) = &self.cwd {
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
            let mut attrs = String::new();
            if let Some(code) = self.last_exit_code {
                attrs.push_str(&format!(" exit_code=\"{}\"", code));
            }
            if let Some(dur) = self.last_command_duration_secs {
                attrs.push_str(&format!(" duration=\"{:.1}s\"", dur));
            }
            prompt.push_str(&format!("<last_command{}>{}</last_command>\n", attrs, cmd));
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
