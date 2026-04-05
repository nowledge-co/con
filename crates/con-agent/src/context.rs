use std::sync::OnceLock;

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

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PaneScopeKind {
    Shell,
    RemoteShell,
    Multiplexer,
    InteractiveApp,
    AgentCli,
}

impl PaneScopeKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Shell => "shell",
            Self::RemoteShell => "remote_shell",
            Self::Multiplexer => "multiplexer",
            Self::InteractiveApp => "interactive_app",
            Self::AgentCli => "agent_cli",
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PaneEvidenceSource {
    ShellIntegration,
    Osc7,
    CommandLine,
    Title,
    ScreenStructure,
}

impl PaneEvidenceSource {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ShellIntegration => "shell_integration",
            Self::Osc7 => "osc7",
            Self::CommandLine => "command_line",
            Self::Title => "title",
            Self::ScreenStructure => "screen_structure",
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PaneConfidence {
    Strong,
    Advisory,
}

impl PaneConfidence {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Strong => "strong",
            Self::Advisory => "advisory",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PaneRuntimeScope {
    pub kind: PaneScopeKind,
    pub label: Option<String>,
    pub host: Option<String>,
    pub confidence: PaneConfidence,
    pub evidence_source: PaneEvidenceSource,
}

impl PaneRuntimeScope {
    pub fn summary(&self) -> String {
        match (self.kind, self.label.as_deref(), self.host.as_deref()) {
            (PaneScopeKind::RemoteShell, _, Some(host)) => format!("remote_shell({host})"),
            (_, Some(label), _) => format!("{}({label})", self.kind.as_str()),
            _ => self.kind.as_str().to_string(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaneObservationFrame {
    pub title: Option<String>,
    pub cwd: Option<String>,
    pub recent_output: Vec<String>,
    pub last_command: Option<String>,
    pub last_exit_code: Option<i32>,
    pub last_command_duration_secs: Option<f64>,
    pub detected_remote_host: Option<String>,
    pub has_shell_integration: bool,
    pub is_alt_screen: bool,
    pub is_busy: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaneRuntimeState {
    pub mode: PaneMode,
    pub shell_metadata_fresh: bool,
    pub remote_host: Option<String>,
    pub remote_host_confidence: Option<PaneConfidence>,
    pub remote_host_source: Option<PaneEvidenceSource>,
    pub tmux_session: Option<String>,
    pub scope_stack: Vec<PaneRuntimeScope>,
    pub warnings: Vec<String>,
}

impl PaneRuntimeState {
    pub fn from_observation(observation: &PaneObservationFrame) -> Self {
        let remote_host_hint = detect_remote_host_hint(
            observation.title.as_deref(),
            observation.detected_remote_host.as_deref(),
            &observation.recent_output,
        );
        let tmux_scope = detect_tmux_scope(
            observation.title.as_deref(),
            observation.last_command.as_deref(),
            &observation.recent_output,
        );
        let tmux_session = detect_tmux_session(
            observation.title.as_deref(),
            observation.last_command.as_deref(),
            &observation.recent_output,
        );
        let mode = infer_pane_mode(
            observation.title.as_deref(),
            &observation.recent_output,
            observation.last_command.as_deref(),
            observation.has_shell_integration,
            observation.is_alt_screen,
        );
        let shell_metadata_fresh = shell_metadata_is_fresh(
            mode,
            observation.has_shell_integration,
            observation.last_command.as_deref(),
            observation.cwd.as_deref(),
        );

        let mut scope_stack = Vec::new();
        if observation.has_shell_integration
            || observation.cwd.is_some()
            || observation.last_command.is_some()
        {
            scope_stack.push(PaneRuntimeScope {
                kind: PaneScopeKind::Shell,
                label: None,
                host: None,
                confidence: PaneConfidence::Strong,
                evidence_source: PaneEvidenceSource::ShellIntegration,
            });
        }

        if let Some((host, confidence, evidence_source)) = remote_host_hint.clone() {
            scope_stack.push(PaneRuntimeScope {
                kind: PaneScopeKind::RemoteShell,
                label: None,
                host: Some(host),
                confidence,
                evidence_source,
            });
        }

        if let Some(scope) = tmux_scope {
            scope_stack.push(scope);
        }

        if let Some(scope) =
            detect_agent_cli_scope(observation.title.as_deref(), &observation.recent_output)
        {
            scope_stack.push(scope);
        } else if mode == PaneMode::Tui {
            scope_stack.push(PaneRuntimeScope {
                kind: PaneScopeKind::InteractiveApp,
                label: observation
                    .title
                    .clone()
                    .filter(|title| !title.trim().is_empty()),
                host: None,
                confidence: PaneConfidence::Advisory,
                evidence_source: if observation.is_alt_screen {
                    PaneEvidenceSource::ShellIntegration
                } else {
                    PaneEvidenceSource::ScreenStructure
                },
            });
        }

        let mut warnings = Vec::new();
        if !shell_metadata_fresh {
            warnings.push(
                "Shell-derived metadata may not describe the currently visible app.".to_string(),
            );
        }
        if mode == PaneMode::Multiplexer {
            warnings.push(
                "Inspect the active multiplexer pane before trusting cwd or command metadata."
                    .to_string(),
            );
        }
        if remote_host_hint.is_some() {
            warnings.push(
                "Remote host identity is inferred from terminal metadata and should be verified before destructive actions.".to_string(),
            );
        }

        let (remote_host, remote_host_confidence, remote_host_source) = match remote_host_hint {
            Some((host, confidence, source)) => (Some(host), Some(confidence), Some(source)),
            None => (None, None, None),
        };

        Self {
            mode,
            shell_metadata_fresh,
            remote_host,
            remote_host_confidence,
            remote_host_source,
            tmux_session,
            scope_stack,
            warnings,
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

fn local_hostname_aliases() -> &'static [String] {
    static LOCAL_HOSTNAME_ALIASES: OnceLock<Vec<String>> = OnceLock::new();
    LOCAL_HOSTNAME_ALIASES.get_or_init(|| {
        let raw = gethostname::gethostname()
            .to_string_lossy()
            .trim()
            .to_ascii_lowercase();
        let mut aliases = Vec::new();
        if !raw.is_empty() {
            aliases.push(raw.clone());
            if let Some(short) = raw.split('.').next() {
                if !short.is_empty() {
                    aliases.push(short.to_string());
                }
            }
        }
        aliases.sort();
        aliases.dedup();
        aliases
    })
}

fn normalize_host_candidate(raw: &str) -> Option<String> {
    let candidate = raw
        .trim()
        .trim_matches(|c: char| {
            matches!(
                c,
                '"' | '\'' | '`' | '[' | ']' | '(' | ')' | '{' | '}' | '<' | '>' | ',' | ';'
            )
        })
        .trim_matches('.');

    if candidate.is_empty() || candidate.contains('/') || candidate.contains('\\') {
        return None;
    }

    if candidate.starts_with('-')
        || !candidate
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'))
    {
        return None;
    }

    let lower = candidate.to_ascii_lowercase();
    if matches!(
        lower.as_str(),
        "localhost"
            | "local"
            | "terminal"
            | "tmux"
            | "tmate"
            | "zellij"
            | "vim"
            | "nvim"
            | "top"
            | "htop"
            | "nvtop"
            | "claude"
            | "codex"
            | "opencode"
    ) {
        return None;
    }

    Some(candidate.to_string())
}

fn is_local_hostname(candidate: &str) -> bool {
    let lower = candidate.to_ascii_lowercase();
    local_hostname_aliases().iter().any(|alias| alias == &lower)
}

fn host_candidate_from_user_at_host(text: &str) -> Option<String> {
    text.split_whitespace().find_map(|token| {
        let (_, host) = token.rsplit_once('@')?;
        let host = normalize_host_candidate(host)?;
        (!is_local_hostname(&host)).then_some(host)
    })
}

fn host_candidate_from_tmux_status(line: &str) -> Option<String> {
    if !line_looks_like_tmux_status(line) {
        return None;
    }

    line.split_whitespace().rev().find_map(|token| {
        if token.contains(':')
            || token
                .chars()
                .all(|c| c.is_ascii_digit() || matches!(c, '*' | '-' | '.'))
        {
            return None;
        }
        let candidate = normalize_host_candidate(token)?;
        (!is_local_hostname(&candidate)).then_some(candidate)
    })
}

fn host_candidate_from_title(title: &str) -> Option<String> {
    if let Some(host) = host_candidate_from_user_at_host(title) {
        return Some(host);
    }

    if title.contains(char::is_whitespace) {
        return None;
    }

    let candidate = normalize_host_candidate(title)?;
    (!is_local_hostname(&candidate)).then_some(candidate)
}

fn detect_remote_host_hint(
    title: Option<&str>,
    osc7_host: Option<&str>,
    recent_output: &[String],
) -> Option<(String, PaneConfidence, PaneEvidenceSource)> {
    if let Some(host) = osc7_host
        .and_then(normalize_host_candidate)
        .filter(|host| !is_local_hostname(host))
    {
        return Some((host, PaneConfidence::Advisory, PaneEvidenceSource::Osc7));
    }

    if let Some(host) = title.and_then(host_candidate_from_user_at_host) {
        return Some((host, PaneConfidence::Advisory, PaneEvidenceSource::Title));
    }

    if let Some(host) = recent_output.iter().rev().find_map(|line| {
        host_candidate_from_user_at_host(line).or_else(|| host_candidate_from_tmux_status(line))
    }) {
        return Some((
            host,
            PaneConfidence::Advisory,
            PaneEvidenceSource::ScreenStructure,
        ));
    }

    if let Some(host) = title.and_then(host_candidate_from_title) {
        return Some((host, PaneConfidence::Advisory, PaneEvidenceSource::Title));
    }

    None
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

fn parse_tmux_target(command: &str) -> Option<String> {
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
    None
}

fn detect_tmux_scope(
    title: Option<&str>,
    last_command: Option<&str>,
    recent_output: &[String],
) -> Option<PaneRuntimeScope> {
    if let Some(command) = last_command {
        if looks_like_tmux_command(command) {
            let label = parse_tmux_target(command).or_else(|| {
                if command.trim_start().starts_with("tmate") {
                    Some("tmate".to_string())
                } else {
                    Some("tmux".to_string())
                }
            });
            return Some(PaneRuntimeScope {
                kind: PaneScopeKind::Multiplexer,
                label,
                host: None,
                confidence: PaneConfidence::Strong,
                evidence_source: PaneEvidenceSource::CommandLine,
            });
        }
    }

    if title.is_some_and(title_looks_like_tmux) {
        return Some(PaneRuntimeScope {
            kind: PaneScopeKind::Multiplexer,
            label: Some("tmux".to_string()),
            host: None,
            confidence: PaneConfidence::Advisory,
            evidence_source: PaneEvidenceSource::Title,
        });
    }

    if recent_output
        .last()
        .is_some_and(|line| line_looks_like_tmux_status(line))
    {
        return Some(PaneRuntimeScope {
            kind: PaneScopeKind::Multiplexer,
            label: Some("tmux".to_string()),
            host: None,
            confidence: PaneConfidence::Advisory,
            evidence_source: PaneEvidenceSource::ScreenStructure,
        });
    }

    None
}

pub fn detect_tmux_session(
    _title: Option<&str>,
    last_command: Option<&str>,
    _recent_output: &[String],
) -> Option<String> {
    last_command
        .filter(|command| looks_like_tmux_command(command))
        .and_then(parse_tmux_target)
}

fn detect_agent_cli_scope(
    title: Option<&str>,
    recent_output: &[String],
) -> Option<PaneRuntimeScope> {
    let title_lower = title.map(str::to_ascii_lowercase);
    let recent_lower: Vec<String> = recent_output
        .iter()
        .map(|line| line.to_ascii_lowercase())
        .collect();

    for (needle, label) in [
        ("claude code", "claude_code"),
        ("opencode", "opencode"),
        ("codex", "codex"),
    ] {
        if title_lower
            .as_deref()
            .is_some_and(|value| value.contains(needle))
        {
            return Some(PaneRuntimeScope {
                kind: PaneScopeKind::AgentCli,
                label: Some(label.to_string()),
                host: None,
                confidence: PaneConfidence::Advisory,
                evidence_source: PaneEvidenceSource::Title,
            });
        }

        if recent_lower
            .iter()
            .filter(|line| line.contains(needle))
            .count()
            >= 2
        {
            return Some(PaneRuntimeScope {
                kind: PaneScopeKind::AgentCli,
                label: Some(label.to_string()),
                host: None,
                confidence: PaneConfidence::Advisory,
                evidence_source: PaneEvidenceSource::ScreenStructure,
            });
        }
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
    if detect_tmux_scope(title, last_command, recent_output).is_some() {
        return PaneMode::Multiplexer;
    }

    if detect_agent_cli_scope(title, recent_output).is_some()
        || is_alt_screen
        || looks_like_dense_fullscreen_ui(recent_output)
    {
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
    /// Effective remote hostname of the focused pane when detected.
    pub focused_hostname: Option<String>,
    /// Confidence for the focused remote hostname, when known.
    pub focused_hostname_confidence: Option<PaneConfidence>,
    /// Evidence source for the focused remote hostname, when known.
    pub focused_hostname_source: Option<PaneEvidenceSource>,
    /// Focused pane title if available.
    pub focused_title: Option<String>,
    /// Whether the focused pane looks like a shell, multiplexer, or TUI.
    pub focused_pane_mode: PaneMode,
    /// Whether shell integration has been observed for the focused pane.
    pub focused_has_shell_integration: bool,
    /// Whether shell-derived metadata such as cwd and last_command should be treated as fresh.
    pub focused_shell_metadata_fresh: bool,
    /// Structured runtime scopes derived from the focused pane's current evidence.
    pub focused_runtime_stack: Vec<PaneRuntimeScope>,
    /// Warnings that should constrain interpretation of the focused pane.
    pub focused_runtime_warnings: Vec<String>,
    /// Current working directory (from OSC 7 or manual detection)
    pub cwd: Option<String>,
    /// Last N lines of terminal output
    pub recent_output: Vec<String>,
    /// Most recent command text when the backend can prove it.
    pub last_command: Option<String>,
    /// Last exit code
    pub last_exit_code: Option<i32>,
    /// Last command duration in seconds (from ghostty COMMAND_FINISHED)
    pub last_command_duration_secs: Option<f64>,
    /// Git branch if in a repo
    pub git_branch: Option<String>,
    /// Effective remote host for the focused pane, if detected.
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
    /// Effective remote hostname when detected.
    pub hostname: Option<String>,
    pub hostname_confidence: Option<PaneConfidence>,
    pub hostname_source: Option<PaneEvidenceSource>,
    pub title: Option<String>,
    pub mode: PaneMode,
    pub has_shell_integration: bool,
    pub shell_metadata_fresh: bool,
    pub runtime_stack: Vec<PaneRuntimeScope>,
    pub runtime_warnings: Vec<String>,
    pub tmux_session: Option<String>,
    pub cwd: Option<String>,
    pub last_command: Option<String>,
    pub last_exit_code: Option<i32>,
    pub is_busy: bool,
    /// Last ~10 lines of visible output
    pub recent_output: Vec<String>,
}

fn format_runtime_stack(scopes: &[PaneRuntimeScope]) -> String {
    if scopes.is_empty() {
        "unknown".to_string()
    } else {
        scopes
            .iter()
            .map(PaneRuntimeScope::summary)
            .collect::<Vec<_>>()
            .join(" > ")
    }
}

fn xml_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

impl TerminalContext {
    pub fn empty() -> Self {
        Self {
            focused_pane_index: 1,
            focused_hostname: None,
            focused_hostname_confidence: None,
            focused_hostname_source: None,
            focused_title: None,
            focused_pane_mode: PaneMode::Unknown,
            focused_has_shell_integration: false,
            focused_shell_metadata_fresh: false,
            focused_runtime_stack: Vec::new(),
            focused_runtime_warnings: Vec::new(),
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
            prompt.push_str(&format!(" host=\"{}\"", xml_escape(host)));
            if let Some(confidence) = self.focused_hostname_confidence {
                prompt.push_str(&format!(" host_confidence=\"{}\"", confidence.as_str()));
            }
            if let Some(source) = self.focused_hostname_source {
                prompt.push_str(&format!(" host_source=\"{}\"", source.as_str()));
            }
        }
        if let Some(title) = &self.focused_title {
            prompt.push_str(&format!(" title=\"{}\"", xml_escape(title)));
        }
        if let Some(cwd) = &self.cwd {
            prompt.push_str(&format!(" cwd=\"{}\"", xml_escape(cwd)));
        }
        prompt.push_str("/>\n");
        if !self.focused_runtime_stack.is_empty() {
            prompt.push_str("<runtime_stack>\n");
            for scope in &self.focused_runtime_stack {
                prompt.push_str(&format!(
                    "  <scope kind=\"{}\" confidence=\"{}\" source=\"{}\"",
                    scope.kind.as_str(),
                    scope.confidence.as_str(),
                    scope.evidence_source.as_str(),
                ));
                if let Some(label) = &scope.label {
                    prompt.push_str(&format!(" label=\"{}\"", xml_escape(label)));
                }
                if let Some(host) = &scope.host {
                    prompt.push_str(&format!(" host=\"{}\"", xml_escape(host)));
                }
                prompt.push_str("/>\n");
            }
            prompt.push_str("</runtime_stack>\n");
        }
        for warning in &self.focused_runtime_warnings {
            prompt.push_str(&format!(
                "<metadata_warning>{}</metadata_warning>\n",
                xml_escape(warning)
            ));
        }

        let total_panes = 1 + self.other_panes.len();

        // When multiple panes are open, embed a full pane layout so the agent
        // can target the right pane(s) without needing to call list_panes first.
        if total_panes > 1 {
            prompt.push_str("<panes>\n");
            // Focused pane
            let cwd_label = self.cwd.as_deref().unwrap_or("?");
            prompt.push_str(&format!(
                "  <pane index=\"{}\" focused=\"true\" cwd=\"{}\" mode=\"{}\" shell_metadata_fresh=\"{}\" runtime=\"{}\"",
                self.focused_pane_index,
                xml_escape(cwd_label),
                self.focused_pane_mode.as_str(),
                self.focused_shell_metadata_fresh,
                xml_escape(&format_runtime_stack(&self.focused_runtime_stack))
            ));
            if let Some(host) = &self.focused_hostname {
                prompt.push_str(&format!(" host=\"{}\"", xml_escape(host)));
                if let Some(confidence) = self.focused_hostname_confidence {
                    prompt.push_str(&format!(" host_confidence=\"{}\"", confidence.as_str()));
                }
            } else {
                prompt.push_str(" host=\"unknown\"");
            }
            prompt.push_str("/>\n");
            // Other panes
            for pane in &self.other_panes {
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
                    .map(|session| format!(" tmux=\"{}\"", xml_escape(session)))
                    .unwrap_or_default();
                let host = pane
                    .hostname
                    .as_deref()
                    .map(|host| format!(" host=\"{}\"", xml_escape(host)))
                    .unwrap_or_else(|| " host=\"unknown\"".to_string());
                let host_confidence = pane
                    .hostname_confidence
                    .map(|confidence| format!(" host_confidence=\"{}\"", confidence.as_str()))
                    .unwrap_or_default();
                prompt.push_str(&format!(
                    "  <pane index=\"{}\" cwd=\"{}\" mode=\"{}\" runtime=\"{}\"{}{}{}{}{}/>\n",
                    pane.pane_index,
                    xml_escape(cwd),
                    pane.mode.as_str(),
                    xml_escape(&format_runtime_stack(&pane.runtime_stack)),
                    host,
                    host_confidence,
                    busy,
                    stale,
                    tmux
                ));
            }
            prompt.push_str("</panes>\n");
        } else if let Some(cwd) = &self.cwd {
            prompt.push_str(&format!("<cwd>{}</cwd>\n", xml_escape(cwd)));
        }

        if let Some(branch) = &self.git_branch {
            prompt.push_str(&format!(
                "<git_branch>{}</git_branch>\n",
                xml_escape(branch)
            ));
        }

        if let Some(host) = &self.ssh_host {
            prompt.push_str(&format!("<ssh_host>{}</ssh_host>\n", xml_escape(host)));
        }

        if let Some(session) = &self.tmux_session {
            prompt.push_str(&format!(
                "<tmux_session>{}</tmux_session>\n",
                xml_escape(session)
            ));
        }

        if let Some(cmd) = &self.last_command {
            let mut attrs = String::new();
            if let Some(code) = self.last_exit_code {
                attrs.push_str(&format!(" exit_code=\"{}\"", code));
            }
            if let Some(dur) = self.last_command_duration_secs {
                attrs.push_str(&format!(" duration=\"{:.1}s\"", dur));
            }
            prompt.push_str(&format!(
                "<last_command{}>{}</last_command>\n",
                attrs,
                xml_escape(cmd)
            ));
        }

        if !self.command_history.is_empty() {
            prompt.push_str("<command_history>\n");
            for block in &self.command_history {
                match block.exit_code {
                    Some(code) => prompt.push_str(&format!(
                        "$ {} (exit {})\n",
                        xml_escape(&block.command),
                        code
                    )),
                    None => prompt.push_str(&format!("$ {}\n", xml_escape(&block.command))),
                }
            }
            prompt.push_str("</command_history>\n");
        }

        if !self.recent_output.is_empty() {
            prompt.push_str("<terminal_output>\n");
            for line in &self.recent_output {
                prompt.push_str(&xml_escape(line));
                prompt.push('\n');
            }
            prompt.push_str("</terminal_output>\n");
        }

        if let Some(diff) = &self.git_diff {
            prompt.push_str("<git_diff>\n");
            prompt.push_str(&xml_escape(diff));
            if !diff.ends_with('\n') {
                prompt.push('\n');
            }
            prompt.push_str("</git_diff>\n");
        }

        if let Some(structure) = &self.project_structure {
            prompt.push_str("<project_structure>\n");
            prompt.push_str(&xml_escape(structure));
            if !structure.ends_with('\n') {
                prompt.push('\n');
            }
            prompt.push_str("</project_structure>\n");
        }

        prompt.push_str("</terminal_context>\n");

        if let Some(agents_md) = &self.agents_md {
            prompt.push_str("\n<agents_md>\n");
            prompt.push_str(&xml_escape(agents_md));
            if !agents_md.ends_with('\n') {
                prompt.push('\n');
            }
            prompt.push_str("</agents_md>\n");
        }

        if !self.skills.is_empty() {
            prompt.push_str("\n<skills>\nThe user can invoke these skills with /name. When a skill is invoked, follow its intent:\n");
            for (name, desc) in &self.skills {
                prompt.push_str(&format!("  /{} — {}\n", xml_escape(name), xml_escape(desc)));
            }
            prompt.push_str("</skills>\n");
        }

        prompt
    }
}

#[cfg(test)]
mod tests {
    use super::{
        PaneConfidence, PaneEvidenceSource, PaneMode, PaneObservationFrame, PaneRuntimeState,
        TerminalContext, detect_remote_host_hint, detect_tmux_session, infer_pane_mode,
        shell_metadata_is_fresh,
    };

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

    #[test]
    fn runtime_state_tracks_nested_tmux_and_agent_cli_scopes() {
        let observation = PaneObservationFrame {
            title: Some("Codex".to_string()),
            cwd: Some("/srv/app".to_string()),
            recent_output: vec![
                "claude code".to_string(),
                "0* api 1 deploy".to_string(),
                "codex".to_string(),
            ],
            last_command: Some("tmux attach -t deploy".to_string()),
            last_exit_code: Some(0),
            last_command_duration_secs: Some(1.2),
            detected_remote_host: Some("prod-2".to_string()),
            has_shell_integration: true,
            is_alt_screen: false,
            is_busy: false,
        };

        let runtime = PaneRuntimeState::from_observation(&observation);
        assert_eq!(runtime.mode, PaneMode::Multiplexer);
        assert_eq!(runtime.tmux_session.as_deref(), Some("deploy"));
        assert!(
            runtime
                .scope_stack
                .iter()
                .any(|scope| scope.host.as_deref() == Some("prod-2"))
        );
        assert!(runtime.scope_stack.iter().any(|scope| {
            scope.label.as_deref() == Some("deploy")
                && scope.evidence_source == PaneEvidenceSource::CommandLine
                && scope.confidence == PaneConfidence::Strong
        }));
    }

    #[test]
    fn detects_remote_host_from_tmux_status_when_osc7_is_missing() {
        let hint = detect_remote_host_hint(
            Some("haswell"),
            None,
            &vec![
                "nvtop 1.4.1".to_string(),
                "0* 63d 3h 22m 1 model-serving 2 ops 3 nekomaster haswell".to_string(),
            ],
        );

        assert_eq!(
            hint,
            Some((
                "haswell".to_string(),
                PaneConfidence::Advisory,
                PaneEvidenceSource::ScreenStructure,
            ))
        );
    }

    #[test]
    fn runtime_state_uses_inferred_remote_host_without_osc7() {
        let observation = PaneObservationFrame {
            title: Some("haswell".to_string()),
            cwd: Some("/home/w".to_string()),
            recent_output: vec![
                "nvtop 1.4.1".to_string(),
                "0* 63d 3h 22m 1 model-serving 2 ops 3 nekomaster haswell".to_string(),
            ],
            last_command: None,
            last_exit_code: None,
            last_command_duration_secs: None,
            detected_remote_host: None,
            has_shell_integration: true,
            is_alt_screen: false,
            is_busy: false,
        };

        let runtime = PaneRuntimeState::from_observation(&observation);
        assert_eq!(runtime.remote_host.as_deref(), Some("haswell"));
        assert_eq!(
            runtime.remote_host_source,
            Some(PaneEvidenceSource::ScreenStructure)
        );
    }

    #[test]
    fn system_prompt_marks_unknown_host_as_unknown_not_local() {
        let prompt = TerminalContext {
            focused_pane_index: 1,
            focused_hostname: None,
            focused_hostname_confidence: None,
            focused_hostname_source: None,
            focused_title: Some("Terminal".to_string()),
            focused_pane_mode: PaneMode::Shell,
            focused_has_shell_integration: false,
            focused_shell_metadata_fresh: false,
            focused_runtime_stack: Vec::new(),
            focused_runtime_warnings: Vec::new(),
            cwd: Some("/tmp".to_string()),
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
            other_panes: vec![],
            git_diff: None,
            project_structure: None,
        }
        .to_system_prompt();

        assert!(!prompt.contains("host=\"local\""));
    }

    #[test]
    fn runtime_state_marks_agent_cli_as_tui_when_visible() {
        let observation = PaneObservationFrame {
            title: Some("Claude Code".to_string()),
            cwd: None,
            recent_output: vec!["Claude Code".to_string(), "Claude Code".to_string()],
            last_command: None,
            last_exit_code: None,
            last_command_duration_secs: None,
            detected_remote_host: None,
            has_shell_integration: false,
            is_alt_screen: false,
            is_busy: false,
        };

        let runtime = PaneRuntimeState::from_observation(&observation);
        assert_eq!(runtime.mode, PaneMode::Tui);
        assert!(runtime.scope_stack.iter().any(|scope| {
            scope.label.as_deref() == Some("claude_code")
                && scope.evidence_source == PaneEvidenceSource::Title
        }));
    }
}
