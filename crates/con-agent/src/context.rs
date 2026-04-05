use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PaneMode {
    Shell,
    Multiplexer,
    Tui,
    Unknown,
}

impl PaneMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Shell => "shell",
            Self::Multiplexer => "multiplexer",
            Self::Tui => "tui",
            Self::Unknown => "unknown",
        }
    }
}

fn looks_like_tmux_command(command: &str) -> bool {
    let trimmed = command.trim_start();
    trimmed == "tmux"
        || trimmed.starts_with("tmux ")
        || trimmed == "tmate"
        || trimmed.starts_with("tmate ")
}

fn title_looks_like_tmux(title: &str) -> bool {
    let title_lower = title.to_ascii_lowercase();
    title_lower.contains("tmux") || title_lower.contains("tmate")
}

fn line_looks_like_tmux_status(line: &str) -> bool {
    let line = line.trim();
    if line.is_empty() {
        return false;
    }

    let tokens: Vec<&str> = line.split_whitespace().collect();
    let colon_windows = tokens
        .iter()
        .filter(|token| {
            token
                .split_once(':')
                .is_some_and(|(idx, _)| !idx.is_empty() && idx.chars().all(|c| c.is_ascii_digit()))
        })
        .count();
    if colon_windows >= 2 {
        return true;
    }

    let numbered_windows = tokens
        .windows(2)
        .filter(|pair| {
            pair[0]
                .chars()
                .all(|c| c.is_ascii_digit() || c == '*' || c == '-')
                && pair[1]
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.')
        })
        .count();
    numbered_windows >= 2
}

fn looks_like_dense_fullscreen_ui(recent_output: &[String]) -> bool {
    if recent_output.len() < 8 {
        return false;
    }

    let non_empty = recent_output
        .iter()
        .filter(|line| !line.trim().is_empty())
        .count();
    if non_empty * 10 < recent_output.len() * 7 {
        return false;
    }

    let has_box_drawing = recent_output.iter().any(|line| {
        line.chars().any(|c| {
            matches!(
                c,
                '│' | '─' | '┌' | '┐' | '└' | '┘' | '├' | '┤' | '┬' | '┴' | '┼'
            )
        })
    });

    has_box_drawing
        || recent_output
            .last()
            .is_some_and(|line| line_looks_like_tmux_status(line))
}

pub fn detect_tmux_session(
    title: Option<&str>,
    last_command: Option<&str>,
    recent_output: &[String],
) -> Option<String> {
    if let Some(command) = last_command {
        if looks_like_tmux_command(command) {
            let tokens: Vec<&str> = command.split_whitespace().collect();
            for window in tokens.windows(2) {
                if window[0] == "-t" {
                    return Some(window[1].trim_matches(&['"', '\''][..]).to_string());
                }
                if let Some(rest) = window[0].strip_prefix("-t") {
                    if !rest.is_empty() {
                        return Some(rest.trim_matches(&['"', '\''][..]).to_string());
                    }
                }
            }
            return Some("attached".to_string());
        }
    }

    if let Some(title) = title.filter(|t| title_looks_like_tmux(t)) {
        let trimmed = title.trim();
        if !trimmed.is_empty() {
            return Some(trimmed.to_string());
        }
    }

    if recent_output
        .last()
        .is_some_and(|line| line_looks_like_tmux_status(line))
    {
        return Some("visible".to_string());
    }

    None
}

pub fn infer_pane_mode(
    title: Option<&str>,
    recent_output: &[String],
    last_command: Option<&str>,
    has_shell_integration: bool,
    is_alt_screen: bool,
) -> PaneMode {
    if detect_tmux_session(title, last_command, recent_output).is_some() {
        return PaneMode::Multiplexer;
    }

    if is_alt_screen || looks_like_dense_fullscreen_ui(recent_output) {
        return PaneMode::Tui;
    }

    if has_shell_integration || last_command.is_some() {
        return PaneMode::Shell;
    }

    PaneMode::Unknown
}

pub fn shell_metadata_is_fresh(
    mode: PaneMode,
    has_shell_integration: bool,
    last_command: Option<&str>,
    cwd: Option<&str>,
) -> bool {
    if mode != PaneMode::Shell {
        return false;
    }

    has_shell_integration || last_command.is_some() || cwd.is_some()
}

/// Terminal context extracted for the AI agent.
/// This is what makes con's agent smarter than a generic chatbot —
/// it always knows what the user is doing in their terminal.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TerminalContext {
    /// 1-based index of the focused pane
    pub focused_pane_index: usize,
    /// Remote hostname of the focused pane (None if local)
    pub focused_hostname: Option<String>,
    /// Focused pane title if available.
    pub focused_title: Option<String>,
    /// Whether the focused pane looks like a shell, multiplexer, or TUI.
    pub focused_pane_mode: PaneMode,
    /// Whether shell integration has been observed for the focused pane.
    pub focused_has_shell_integration: bool,
    /// Whether shell-derived metadata such as cwd and last_command should be treated as fresh.
    pub focused_shell_metadata_fresh: bool,
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
    /// Available skills: (name, description) pairs
    pub skills: Vec<(String, String)>,
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
    pub title: Option<String>,
    pub mode: PaneMode,
    pub has_shell_integration: bool,
    pub shell_metadata_fresh: bool,
    pub tmux_session: Option<String>,
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
            focused_title: None,
            focused_pane_mode: PaneMode::Unknown,
            focused_has_shell_integration: false,
            focused_shell_metadata_fresh: false,
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
             - When pane mode is not `shell`, or shell metadata is marked stale, do NOT assume cwd/hostname/last_command describe the visible app.\n\
             - For tmux, vim, htop, dashboards, and other TUIs: inspect the pane with read_pane/list_panes/send_keys before making claims.\n\
             - If a command fails (exit_code != 0), diagnose the error before retrying.\n\
             - When editing files: always read first, ensure old_text is unique, verify the edit succeeded.\n\
             </safety>\n\n",
        );

        prompt.push_str("<terminal_context>\n");
        prompt.push_str(&format!(
            "<focused_pane index=\"{}\" mode=\"{}\" shell_integration=\"{}\" shell_metadata_fresh=\"{}\"",
            self.focused_pane_index,
            self.focused_pane_mode.as_str(),
            self.focused_has_shell_integration,
            self.focused_shell_metadata_fresh,
        ));
        if let Some(host) = &self.focused_hostname {
            prompt.push_str(&format!(" host=\"{}\"", host));
        }
        if let Some(title) = &self.focused_title {
            prompt.push_str(&format!(" title=\"{}\"", title));
        }
        if let Some(cwd) = &self.cwd {
            prompt.push_str(&format!(" cwd=\"{}\"", cwd));
        }
        prompt.push_str("/>\n");
        if !self.focused_shell_metadata_fresh {
            prompt.push_str(
                "<metadata_warning>Shell-derived metadata may be stale for the visible app in this pane. Prefer live pane inspection.</metadata_warning>\n",
            );
        }

        let total_panes = 1 + self.other_panes.len();

        // When multiple panes are open, embed a full pane layout so the agent
        // can target the right pane(s) without needing to call list_panes first.
        if total_panes > 1 {
            prompt.push_str("<panes>\n");
            // Focused pane
            let host_label = self.focused_hostname.as_deref().unwrap_or("local");
            let cwd_label = self.cwd.as_deref().unwrap_or("?");
            prompt.push_str(&format!(
                "  <pane index=\"{}\" focused=\"true\" host=\"{}\" cwd=\"{}\" mode=\"{}\" shell_metadata_fresh=\"{}\"/>\n",
                self.focused_pane_index,
                host_label,
                cwd_label,
                self.focused_pane_mode.as_str(),
                self.focused_shell_metadata_fresh
            ));
            // Other panes
            for pane in &self.other_panes {
                let host = pane.hostname.as_deref().unwrap_or("local");
                let cwd = pane.cwd.as_deref().unwrap_or("?");
                let busy = if pane.is_busy { " busy=\"true\"" } else { "" };
                let stale = if pane.shell_metadata_fresh {
                    " shell_metadata_fresh=\"true\""
                } else {
                    " shell_metadata_fresh=\"false\""
                };
                let tmux = pane
                    .tmux_session
                    .as_deref()
                    .map(|session| format!(" tmux=\"{}\"", session))
                    .unwrap_or_default();
                prompt.push_str(&format!(
                    "  <pane index=\"{}\" host=\"{}\" cwd=\"{}\" mode=\"{}\"{}{}{}/>\n",
                    pane.pane_index,
                    host,
                    cwd,
                    pane.mode.as_str(),
                    busy,
                    stale,
                    tmux
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
            prompt.push_str("\n<skills>\nThe user can invoke these skills with /name. When a skill is invoked, follow its intent:\n");
            for (name, desc) in &self.skills {
                prompt.push_str(&format!("  /{name} — {desc}\n"));
            }
            prompt.push_str("</skills>\n");
        }

        prompt
    }
}

#[cfg(test)]
mod tests {
    use super::{PaneMode, detect_tmux_session, infer_pane_mode, shell_metadata_is_fresh};

    #[test]
    fn detects_tmux_from_command_target() {
        let session = detect_tmux_session(None, Some("tmux attach -t model-serving"), &Vec::new());
        assert_eq!(session.as_deref(), Some("model-serving"));
    }

    #[test]
    fn infers_tmux_from_status_line() {
        let recent = vec![
            "nvtop 1.4.1".to_string(),
            "0* 63d 1m 45m 1 model-serving 2 ops 3 nekomaster".to_string(),
        ];
        assert_eq!(
            infer_pane_mode(None, &recent, None, false, false),
            PaneMode::Multiplexer
        );
    }

    #[test]
    fn shell_metadata_is_stale_inside_tui() {
        assert!(!shell_metadata_is_fresh(
            PaneMode::Tui,
            true,
            Some("top"),
            Some("/tmp"),
        ));
    }
}
