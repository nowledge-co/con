use serde::{Deserialize, Serialize};

use crate::control::{
    format_target_stack, PaneAddressSpace, PaneControlCapability, PaneControlChannel,
    PaneControlState, PaneVisibleTarget, PaneVisibleTargetKind,
};

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
    SurfaceState,
    Osc7,
    CommandLine,
    Title,
    ScreenStructure,
}

impl PaneEvidenceSource {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ShellIntegration => "shell_integration",
            Self::SurfaceState => "surface_state",
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
pub struct PaneObservationSupport {
    /// Whether the backend can provide authoritative foreground command text.
    pub foreground_command: bool,
    /// Whether the backend can provide authoritative alternate-screen state.
    pub alternate_screen: bool,
    /// Whether the backend can provide authoritative remote-host identity.
    pub remote_host_identity: bool,
}

impl PaneObservationSupport {
    pub fn backend_limit_note(&self) -> Option<String> {
        let mut missing = Vec::new();
        if !self.foreground_command {
            missing.push("foreground command text");
        }
        if !self.alternate_screen {
            missing.push("alternate-screen state");
        }
        if !self.remote_host_identity {
            missing.push("remote-host identity");
        }

        if missing.is_empty() {
            return None;
        }

        Some(format!(
            "Embedded Ghostty does not currently export authoritative {} for this pane. Unproven foreground runtimes must stay unknown.",
            missing.join(", ")
        ))
    }
}

impl Default for PaneObservationSupport {
    fn default() -> Self {
        Self {
            foreground_command: false,
            alternate_screen: false,
            remote_host_identity: false,
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
    pub support: PaneObservationSupport,
    pub has_shell_integration: bool,
    pub is_alt_screen: bool,
    pub is_busy: bool,
    pub input_generation: u64,
    pub last_command_finished_input_generation: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaneRuntimeState {
    pub mode: PaneMode,
    pub shell_metadata_fresh: bool,
    pub remote_host: Option<String>,
    pub remote_host_confidence: Option<PaneConfidence>,
    pub remote_host_source: Option<PaneEvidenceSource>,
    pub agent_cli: Option<String>,
    pub tmux_session: Option<String>,
    pub active_scope: Option<PaneRuntimeScope>,
    pub evidence: Vec<PaneEvidence>,
    pub scope_stack: Vec<PaneRuntimeScope>,
    pub warnings: Vec<String>,
}

impl PaneRuntimeState {
    pub fn from_observation(observation: &PaneObservationFrame) -> Self {
        PaneRuntimeObserver::default().observe(observation.clone())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaneEvidence {
    pub subject: String,
    pub value: Option<String>,
    pub source: PaneEvidenceSource,
    pub confidence: PaneConfidence,
    pub generation: u64,
    pub note: Option<String>,
}

#[derive(Debug, Clone)]
struct StickyFact {
    value: String,
    confidence: PaneConfidence,
    source: PaneEvidenceSource,
    generation: u64,
}

#[derive(Debug, Clone, Default)]
pub struct PaneRuntimeObserver {
    generation: u64,
    multiplexer: Option<StickyFact>,
    tmux_session: Option<StickyFact>,
    agent_cli: Option<StickyFact>,
}

impl PaneRuntimeObserver {
    pub fn observe(&mut self, observation: PaneObservationFrame) -> PaneRuntimeState {
        self.generation += 1;

        let shell_metadata_fresh = shell_metadata_is_fresh(
            observation.has_shell_integration,
            observation.input_generation,
            observation.last_command_finished_input_generation,
        );

        if shell_metadata_fresh {
            self.multiplexer = None;
            self.tmux_session = None;
            self.agent_cli = None;
        } else if let Some(scope) = detect_tmux_scope(observation.last_command.as_deref()) {
            self.multiplexer = Some(StickyFact {
                value: scope.label.clone().unwrap_or_else(|| "tmux".to_string()),
                confidence: scope.confidence,
                source: scope.evidence_source,
                generation: self.generation,
            });
            self.tmux_session =
                detect_tmux_session(observation.last_command.as_deref()).map(|value| StickyFact {
                    value,
                    confidence: PaneConfidence::Strong,
                    source: PaneEvidenceSource::CommandLine,
                    generation: self.generation,
                });
            self.agent_cli = None;
        } else if let Some(scope) = detect_agent_cli_scope(observation.last_command.as_deref()) {
            self.agent_cli = scope.label.clone().map(|value| StickyFact {
                value,
                confidence: scope.confidence,
                source: scope.evidence_source,
                generation: self.generation,
            });
            self.multiplexer = None;
            self.tmux_session = None;
        }

        let mode = if self.multiplexer.is_some() {
            PaneMode::Multiplexer
        } else if self.agent_cli.is_some() || observation.is_alt_screen {
            PaneMode::Tui
        } else if shell_metadata_fresh {
            PaneMode::Shell
        } else {
            PaneMode::Unknown
        };

        let mut scope_stack = Vec::new();
        if shell_metadata_fresh {
            scope_stack.push(PaneRuntimeScope {
                kind: PaneScopeKind::Shell,
                label: None,
                host: None,
                confidence: PaneConfidence::Strong,
                evidence_source: PaneEvidenceSource::ShellIntegration,
            });
        }

        let mut evidence = Vec::new();

        if shell_metadata_fresh {
            evidence.push(PaneEvidence {
                subject: "shell_prompt".to_string(),
                value: Some("confirmed".to_string()),
                source: PaneEvidenceSource::ShellIntegration,
                confidence: PaneConfidence::Strong,
                generation: self.generation,
                note: Some(
                    "Ghostty shell integration observed a clean shell prompt after the most recent input.".to_string(),
                ),
            });
        }

        if let Some(multiplexer) = &self.multiplexer {
            scope_stack.push(PaneRuntimeScope {
                kind: PaneScopeKind::Multiplexer,
                label: Some(multiplexer.value.clone()),
                host: None,
                confidence: multiplexer.confidence,
                evidence_source: multiplexer.source,
            });
            evidence.push(PaneEvidence {
                subject: "multiplexer".to_string(),
                value: Some(multiplexer.value.clone()),
                source: multiplexer.source,
                confidence: multiplexer.confidence,
                generation: multiplexer.generation,
                note: Some("The proven foreground command is tmux/tmate.".to_string()),
            });
        }

        if let Some(agent_cli) = &self.agent_cli {
            scope_stack.push(PaneRuntimeScope {
                kind: PaneScopeKind::AgentCli,
                label: Some(agent_cli.value.clone()),
                host: None,
                confidence: agent_cli.confidence,
                evidence_source: agent_cli.source,
            });
            evidence.push(PaneEvidence {
                subject: "agent_cli".to_string(),
                value: Some(agent_cli.value.clone()),
                source: agent_cli.source,
                confidence: agent_cli.confidence,
                generation: agent_cli.generation,
                note: Some(
                    "The proven foreground command matches a supported agent CLI.".to_string(),
                ),
            });
        } else if observation.is_alt_screen {
            scope_stack.push(PaneRuntimeScope {
                kind: PaneScopeKind::InteractiveApp,
                label: command_label(observation.last_command.as_deref()),
                host: None,
                confidence: PaneConfidence::Strong,
                evidence_source: PaneEvidenceSource::SurfaceState,
            });
            evidence.push(PaneEvidence {
                subject: "interactive_app".to_string(),
                value: command_label(observation.last_command.as_deref()),
                source: PaneEvidenceSource::SurfaceState,
                confidence: PaneConfidence::Strong,
                generation: self.generation,
                note: Some(
                    "Ghostty reports alternate-screen mode for the visible surface.".to_string(),
                ),
            });
        }

        let active_scope = scope_stack.last().cloned();
        let tmux_session = self.tmux_session.as_ref().map(|value| value.value.clone());
        let agent_cli = self.agent_cli.as_ref().map(|value| value.value.clone());

        let mut warnings = Vec::new();
        if !shell_metadata_fresh {
            warnings.push(
                "Visible shell prompt is not confirmed. Treat cwd and last_command as historical shell metadata, not foreground-app truth.".to_string(),
            );
        }
        if let Some(note) = observation.support.backend_limit_note() {
            warnings.push(note);
        }
        if mode == PaneMode::Multiplexer {
            warnings.push(
                "con only knows that tmux/tmate is the foreground command of this outer pane. It does not yet know the active inner tmux window or pane.".to_string(),
            );
        }

        PaneRuntimeState {
            mode,
            shell_metadata_fresh,
            remote_host: None,
            remote_host_confidence: None,
            remote_host_source: None,
            agent_cli,
            tmux_session,
            active_scope,
            evidence,
            scope_stack,
            warnings,
        }
    }
}

fn looks_like_tmux_command(command: &str) -> bool {
    command_basename(command)
        .as_deref()
        .is_some_and(|name| matches!(name, "tmux" | "tmate"))
}

fn is_env_assignment(token: &str) -> bool {
    token.split_once('=').is_some_and(|(name, _)| {
        !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
    })
}

fn command_basename(command: &str) -> Option<String> {
    let mut tokens = command.split_whitespace().peekable();
    while let Some(token) = tokens.next() {
        let token = token.trim_matches(&['"', '\''][..]);
        if token.is_empty() {
            continue;
        }
        if is_env_assignment(token) || matches!(token, "env" | "sudo" | "command" | "nohup") {
            continue;
        }
        let basename = token
            .rsplit('/')
            .next()
            .unwrap_or(token)
            .to_ascii_lowercase();
        if !basename.is_empty() {
            return Some(basename);
        }
    }
    None
}

fn command_label(command: Option<&str>) -> Option<String> {
    command_basename(command?).map(|name| match name.as_str() {
        "claude" | "claude-code" => "claude_code".to_string(),
        "codex" => "codex".to_string(),
        "opencode" | "open-code" => "opencode".to_string(),
        other => other.to_string(),
    })
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

fn detect_tmux_scope(last_command: Option<&str>) -> Option<PaneRuntimeScope> {
    let command = last_command?;
    if !looks_like_tmux_command(command) {
        return None;
    }

    let label = parse_tmux_target(command).or_else(|| {
        if command_basename(command).as_deref() == Some("tmate") {
            Some("tmate".to_string())
        } else {
            Some("tmux".to_string())
        }
    });
    Some(PaneRuntimeScope {
        kind: PaneScopeKind::Multiplexer,
        label,
        host: None,
        confidence: PaneConfidence::Strong,
        evidence_source: PaneEvidenceSource::CommandLine,
    })
}

pub fn detect_tmux_session(last_command: Option<&str>) -> Option<String> {
    last_command
        .filter(|command| looks_like_tmux_command(command))
        .and_then(parse_tmux_target)
}

fn detect_agent_cli_scope(last_command: Option<&str>) -> Option<PaneRuntimeScope> {
    let label = match command_basename(last_command?)?.as_str() {
        "claude" | "claude-code" => "claude_code",
        "codex" => "codex",
        "opencode" | "open-code" => "opencode",
        _ => return None,
    };

    Some(PaneRuntimeScope {
        kind: PaneScopeKind::AgentCli,
        label: Some(label.to_string()),
        host: None,
        confidence: PaneConfidence::Strong,
        evidence_source: PaneEvidenceSource::CommandLine,
    })
}

pub fn infer_pane_mode(
    last_command: Option<&str>,
    has_shell_integration: bool,
    is_alt_screen: bool,
    input_generation: u64,
    last_command_finished_input_generation: u64,
) -> PaneMode {
    if detect_tmux_scope(last_command).is_some() {
        return PaneMode::Multiplexer;
    }

    if detect_agent_cli_scope(last_command).is_some() || is_alt_screen {
        return PaneMode::Tui;
    }

    if shell_metadata_is_fresh(
        has_shell_integration,
        input_generation,
        last_command_finished_input_generation,
    ) {
        return PaneMode::Shell;
    }

    PaneMode::Unknown
}

pub fn shell_metadata_is_fresh(
    has_shell_integration: bool,
    input_generation: u64,
    last_command_finished_input_generation: u64,
) -> bool {
    has_shell_integration && input_generation == last_command_finished_input_generation
}

pub fn direct_terminal_exec_is_safe(runtime: &PaneRuntimeState) -> bool {
    PaneControlState::from_runtime(runtime).allows_visible_shell_exec()
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
    /// What the backend can actually prove for the focused pane.
    pub focused_observation_support: PaneObservationSupport,
    /// Structured runtime scopes derived from the focused pane's current evidence.
    pub focused_runtime_stack: Vec<PaneRuntimeScope>,
    /// Warnings that should constrain interpretation of the focused pane.
    pub focused_runtime_warnings: Vec<String>,
    /// Typed control contract for the focused pane.
    pub focused_control: PaneControlState,
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
    pub observation_support: PaneObservationSupport,
    pub control: PaneControlState,
    pub agent_cli: Option<String>,
    pub active_scope: Option<PaneRuntimeScope>,
    pub evidence: Vec<PaneEvidence>,
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

fn format_scope_summary(scope: &PaneRuntimeScope) -> String {
    scope.summary()
}

fn format_control_channels(channels: &[PaneControlChannel]) -> String {
    channels
        .iter()
        .map(|channel| channel.as_str())
        .collect::<Vec<_>>()
        .join(",")
}

fn format_control_capabilities(capabilities: &[PaneControlCapability]) -> String {
    capabilities
        .iter()
        .map(|capability| capability.as_str())
        .collect::<Vec<_>>()
        .join(",")
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
            focused_observation_support: PaneObservationSupport::default(),
            focused_runtime_stack: Vec::new(),
            focused_runtime_warnings: Vec::new(),
            focused_control: PaneControlState {
                address_space: PaneAddressSpace::ConPane,
                target_stack: vec![PaneVisibleTarget {
                    kind: PaneVisibleTargetKind::Unknown,
                    label: None,
                    host: None,
                }],
                visible_target: PaneVisibleTarget {
                    kind: PaneVisibleTargetKind::Unknown,
                    label: None,
                    host: None,
                },
                tmux: None,
                channels: vec![
                    PaneControlChannel::ReadScreen,
                    PaneControlChannel::SearchScrollback,
                    PaneControlChannel::RawInput,
                ],
                capabilities: vec![
                    PaneControlCapability::ReadScreen,
                    PaneControlCapability::SearchScrollback,
                    PaneControlCapability::SendRawInput,
                ],
                notes: vec![
                    "Pane control state is unavailable; treat the visible target as unknown."
                        .to_string(),
                ],
            },
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
             You are an orchestrator with direct API access to the terminal.\n\n\
             ### Keystroke categories — know the difference\n\
             1. **con application shortcuts** (Cmd+T, Cmd+W, Cmd+N): These control the con app itself.\n\
                You CANNOT send these. Use tools instead: create_pane for new terminals.\n\
             2. **Terminal protocol sequences** (\\x02c for tmux prefix+c, \\x1b for Escape, \\x03 for Ctrl-C):\n\
                These travel through the PTY to the remote program. send_keys is the correct tool for these.\n\
             3. **Shell commands** (ls, apt update, git status):\n\
                Use terminal_exec when `exec_visible_shell` is available. When it is NOT, first observe the pane \
                and only use send_keys if a shell prompt is visibly present.\n\n\
             ### Choose the right tool\n\
             - SHELL COMMANDS on a pane with `exec_visible_shell` → terminal_exec / batch_exec.\n\
             - PARALLEL WORK across hosts → create_pane for each host (output shows connection state), \
               then terminal_exec (if exec_visible_shell) or send_keys.\n\
             - SHELL COMMANDS on a pane WITHOUT `exec_visible_shell` → use read_pane first, then send_keys \"command\\n\" only if a shell prompt is visibly present.\n\
             - LONG-RUNNING commands → launch, then wait_for (not repeated read_pane).\n\
             - INTERACTIVE TUI (vim, tmux, htop) → send_keys + read_pane (follow playbooks).\n\
             - LOCAL FILE operations → file_read, file_write, edit_file, search, list_files.\n\
             - REMOTE FILE operations → send_keys in a remote shell (cat, heredoc, editor commands).\n\n\
             ## Turn efficiency\n\
             - read_pane default (50 lines) is usually sufficient. Only increase if needed.\n\
             - Use search_panes instead of reading full scrollback to find specific output.\n\
             - Batch keystrokes: e.g., send_keys \"\\x1bggdG\" is one turn, not three.\n\n\
             ## Tools\n\n\
             <tools>\n\
             - terminal_exec: Run a command visibly in a con pane. Requires `exec_visible_shell` capability.\n\
               `pane_index` addresses a con pane from list_panes — not a tmux pane/window or editor buffer.\n\
               Check exit_code: 0 = success, non-zero = failure, null = no shell integration or still running.\n\n\
             - batch_exec: Run commands on MULTIPLE panes in PARALLEL. Same `exec_visible_shell` requirement.\n\n\
             - shell_exec: Run a command in a hidden LOCAL subprocess. Output not shown to user.\n\
               For local-only tasks: git, file searches, package lookups. Never for remote environments.\n\n\
             - create_pane: Create a new terminal pane (split in current tab). Optionally run a startup command \
               (e.g. \"ssh host\"). The command executes automatically — do NOT re-send it.\n\
               Returns the pane index AND initial output (waits for output to settle). \
               Check the output to see what happened — no need for a separate read_pane.\n\n\
             - list_panes: List all panes with metadata, control state, capabilities, and addressing notes.\n\n\
             - read_pane: Read last N lines from any pane (includes scrollback).\n\n\
             - send_keys: Send keystrokes to a pane's PTY. Use for:\n\
               (a) TUI interaction — tmux prefix sequences, vim commands, arrow keys, Escape, Ctrl-C.\n\
               (b) Shell commands on panes without exec_visible_shell (SSH, tmux shells).\n\
               NEVER use to simulate con application shortcuts (Cmd+T, Cmd+W).\n\n\
             - wait_for: Wait for a pane to become idle or for a pattern to appear. Use after launching \
               long-running commands instead of polling with read_pane. Idle mode (no pattern) works universally — \
               shell integration for precise detection, output quiescence as fallback. On timeout, \
               read_pane to check progress and call wait_for again.\n\n\
             - tmux_inspect: Inspect tmux adapter state for a pane containing a tmux session.\n\
             - search_panes: Search scrollback across panes by regex.\n\n\
             - file_read, file_write, edit_file: LOCAL filesystem only. Cannot access remote SSH hosts.\n\
             - list_files: List LOCAL directory. Respects .gitignore. Max 500 entries.\n\
             - search: Search LOCAL file contents by regex.\n\
             </tools>\n\n\
             <safety>\n\
             - NEVER execute rm -rf, DROP TABLE, or destructive commands without explicit user confirmation.\n\
             - Check is_alive before executing — false means PTY exited, commands will fail.\n\
             - If is_busy is true, a command is already running — wait or use a different pane.\n\
             - Observation is not control. Pane title and visible screen text are raw observations, not typed runtime facts.\n\
             - Backend support is explicit. If `supports_foreground_command`, `supports_alt_screen`, or `supports_remote_host_identity` is false, treat missing runtime data as unavailable backend truth, not as proof of absence.\n\
             - Addressing is layered. A con pane index ≠ tmux pane id ≠ tmux window index ≠ editor buffer.\n\
             - When pane mode is not `shell` or shell metadata is stale, do NOT trust cwd/hostname/last_command and do NOT assume a shell prompt.\n\
             - If a command fails (exit_code != 0), diagnose the error before retrying.\n\
             - When editing files: always read first, ensure old_text is unique, verify the edit succeeded.\n\
             </safety>\n\n\
             <verify_before_act>\n\
             MANDATORY: Observe before acting. Never assume terminal state.\n\
             - Before send_keys: read_pane to see what is on screen and where keystrokes will go.\n\
             - After send_keys: read_pane to verify the action took effect.\n\
             - After create_pane: check the returned output to see what happened (output is included).\n\
             - Never chain multiple actions without observing between them.\n\
             </verify_before_act>\n\n",
        );

        self.emit_tui_guide(&mut prompt);

        prompt.push_str("<terminal_context>\n");
        prompt.push_str(&format!(
            "<focused_pane index=\"{}\" mode=\"{}\" shell_integration=\"{}\" shell_metadata_fresh=\"{}\"",
            self.focused_pane_index,
            self.focused_pane_mode.as_str(),
            self.focused_has_shell_integration,
            self.focused_shell_metadata_fresh,
        ));
        prompt.push_str(&format!(
            " supports_foreground_command=\"{}\" supports_alt_screen=\"{}\" supports_remote_host_identity=\"{}\"",
            self.focused_observation_support.foreground_command,
            self.focused_observation_support.alternate_screen,
            self.focused_observation_support.remote_host_identity,
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
        if let Some(scope) = self.focused_runtime_stack.last() {
            prompt.push_str(&format!(
                " active_scope=\"{}\"",
                xml_escape(&format_scope_summary(scope))
            ));
        }
        prompt.push_str("/>\n");
        prompt.push_str(&format!(
            "<focused_control address_space=\"{}\" visible_target=\"{}\" target_stack=\"{}\" channels=\"{}\" capabilities=\"{}\"",
            self.focused_control.address_space.as_str(),
            xml_escape(&self.focused_control.visible_target.summary()),
            xml_escape(&format_target_stack(&self.focused_control.target_stack)),
            xml_escape(&format_control_channels(&self.focused_control.channels)),
            xml_escape(&format_control_capabilities(&self.focused_control.capabilities)),
        ));
        if let Some(host) = &self.focused_control.visible_target.host {
            prompt.push_str(&format!(" target_host=\"{}\"", xml_escape(host)));
        }
        if let Some(tmux) = &self.focused_control.tmux {
            prompt.push_str(&format!(" tmux_mode=\"{}\"", tmux.mode.as_str()));
            if let Some(session) = &tmux.session_name {
                prompt.push_str(&format!(" tmux_session=\"{}\"", xml_escape(session)));
            }
        }
        prompt.push_str("/>\n");
        if let Some(tmux) = &self.focused_control.tmux {
            let front_target = tmux
                .front_target
                .as_ref()
                .map(PaneVisibleTarget::summary)
                .unwrap_or_else(|| "unknown".to_string());
            prompt.push_str(&format!(
                "<tmux_control mode=\"{}\" front_target=\"{}\">{}</tmux_control>\n",
                tmux.mode.as_str(),
                xml_escape(&front_target),
                xml_escape(&tmux.reason)
            ));
        }
        for note in &self.focused_control.notes {
            prompt.push_str(&format!(
                "<control_note>{}</control_note>\n",
                xml_escape(note)
            ));
        }
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
                "  <pane index=\"{}\" focused=\"true\" cwd=\"{}\" mode=\"{}\" shell_metadata_fresh=\"{}\" runtime=\"{}\" control_target=\"{}\" target_stack=\"{}\" control_channels=\"{}\" control_capabilities=\"{}\"",
                self.focused_pane_index,
                xml_escape(cwd_label),
                self.focused_pane_mode.as_str(),
                self.focused_shell_metadata_fresh,
                xml_escape(&format_runtime_stack(&self.focused_runtime_stack)),
                xml_escape(&self.focused_control.visible_target.summary()),
                xml_escape(&format_target_stack(&self.focused_control.target_stack)),
                xml_escape(&format_control_channels(&self.focused_control.channels)),
                xml_escape(&format_control_capabilities(&self.focused_control.capabilities))
            ));
            prompt.push_str(&format!(
                " supports_foreground_command=\"{}\" supports_alt_screen=\"{}\" supports_remote_host_identity=\"{}\"",
                self.focused_observation_support.foreground_command,
                self.focused_observation_support.alternate_screen,
                self.focused_observation_support.remote_host_identity,
            ));
            if let Some(host) = &self.focused_hostname {
                prompt.push_str(&format!(" host=\"{}\"", xml_escape(host)));
                if let Some(confidence) = self.focused_hostname_confidence {
                    prompt.push_str(&format!(" host_confidence=\"{}\"", confidence.as_str()));
                }
            } else {
                prompt.push_str(" host=\"unknown\"");
            }
            if let Some(scope) = self.focused_runtime_stack.last() {
                prompt.push_str(&format!(
                    " active_scope=\"{}\"",
                    xml_escape(&format_scope_summary(scope))
                ));
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
                let active_scope = pane
                    .active_scope
                    .as_ref()
                    .map(|scope| {
                        format!(
                            " active_scope=\"{}\"",
                            xml_escape(&format_scope_summary(scope))
                        )
                    })
                    .unwrap_or_default();
                let control_target = format!(
                    " control_target=\"{}\"",
                    xml_escape(&pane.control.visible_target.summary())
                );
                let target_stack = format!(
                    " target_stack=\"{}\"",
                    xml_escape(&format_target_stack(&pane.control.target_stack))
                );
                let control_channels = format!(
                    " control_channels=\"{}\"",
                    xml_escape(&format_control_channels(&pane.control.channels))
                );
                let control_capabilities = format!(
                    " control_capabilities=\"{}\"",
                    xml_escape(&format_control_capabilities(&pane.control.capabilities))
                );
                let tmux_control = pane
                    .control
                    .tmux
                    .as_ref()
                    .map(|tmux| format!(" tmux_mode=\"{}\"", tmux.mode.as_str()))
                    .unwrap_or_default();
                let support = format!(
                    " supports_foreground_command=\"{}\" supports_alt_screen=\"{}\" supports_remote_host_identity=\"{}\"",
                    pane.observation_support.foreground_command,
                    pane.observation_support.alternate_screen,
                    pane.observation_support.remote_host_identity,
                );
                prompt.push_str(&format!(
                    "  <pane index=\"{}\" cwd=\"{}\" mode=\"{}\" runtime=\"{}\"{}{}{}{}{}{}{}{}{}{}{}{}/>\n",
                    pane.pane_index,
                    xml_escape(cwd),
                    pane.mode.as_str(),
                    xml_escape(&format_runtime_stack(&pane.runtime_stack)),
                    host,
                    host_confidence,
                    active_scope,
                    busy,
                    stale,
                    tmux,
                    control_target,
                    target_stack,
                    tmux_control,
                    control_channels,
                    control_capabilities,
                    support,
                ));
                for note in &pane.control.notes {
                    prompt.push_str(&format!(
                        "  <pane_control_note index=\"{}\">{}</pane_control_note>\n",
                        pane.pane_index,
                        xml_escape(note)
                    ));
                }
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

    /// Emit contextual TUI interaction guidance when any visible pane has a TUI target.
    /// Only adds content when a TUI is detected — shell-only sessions are unchanged.
    fn emit_tui_guide(&self, prompt: &mut String) {
        use crate::control::PaneVisibleTargetKind;
        use crate::playbooks;

        let focused_is_tui = matches!(
            self.focused_control.visible_target.kind,
            PaneVisibleTargetKind::InteractiveApp
                | PaneVisibleTargetKind::TmuxSession
                | PaneVisibleTargetKind::AgentCli
        );
        let any_other_pane_tui = self.other_panes.iter().any(|p| {
            matches!(
                p.control.visible_target.kind,
                PaneVisibleTargetKind::InteractiveApp
                    | PaneVisibleTargetKind::TmuxSession
                    | PaneVisibleTargetKind::AgentCli
            )
        });
        let is_remote = self.focused_hostname.is_some() || self.ssh_host.is_some();

        if !focused_is_tui && !any_other_pane_tui && !is_remote {
            return;
        }

        prompt.push_str("<tui_interaction_guide>\n");

        // Remote work rules come first — they change what tools are valid
        if is_remote {
            prompt.push_str(playbooks::REMOTE_WORK);
            prompt.push('\n');
        }

        if focused_is_tui || any_other_pane_tui {
            prompt.push_str(playbooks::VERIFY_AFTER_ACT);
            prompt.push('\n');
        }

        // Check for tmux anywhere in focused stack or other panes
        let has_tmux = self
            .focused_control
            .target_stack
            .iter()
            .any(|t| t.kind == PaneVisibleTargetKind::TmuxSession)
            || self.other_panes.iter().any(|p| {
                p.control
                    .target_stack
                    .iter()
                    .any(|t| t.kind == PaneVisibleTargetKind::TmuxSession)
            });

        // Check for vim/nvim in visible targets or titles
        let has_vim = self.has_vim_visible()
            || self
                .other_panes
                .iter()
                .any(|p| is_vim_target(&p.control.visible_target));

        if has_tmux {
            prompt.push_str(playbooks::TMUX_PLAYBOOK);
            prompt.push('\n');
        }
        if has_vim {
            prompt.push_str(playbooks::VIM_PLAYBOOK);
            prompt.push('\n');
        }
        if (focused_is_tui || any_other_pane_tui) && !has_tmux && !has_vim {
            prompt.push_str(playbooks::GENERAL_TUI);
            prompt.push('\n');
        }

        prompt.push_str("</tui_interaction_guide>\n\n");
    }

    fn has_vim_visible(&self) -> bool {
        is_vim_target(&self.focused_control.visible_target)
    }
}

fn is_vim_target(target: &crate::control::PaneVisibleTarget) -> bool {
    target.label.as_ref().is_some_and(|label| {
        let lower = label.to_lowercase();
        lower.contains("vim") || lower.contains("nvim") || lower.contains("neovim")
    })
}

#[cfg(test)]
mod tests {
    use crate::control::PaneControlState;

    use super::{
        detect_tmux_session, direct_terminal_exec_is_safe, infer_pane_mode,
        shell_metadata_is_fresh, PaneConfidence, PaneEvidenceSource, PaneMode,
        PaneObservationFrame, PaneObservationSupport, PaneRuntimeObserver, PaneRuntimeScope,
        PaneRuntimeState, PaneScopeKind, TerminalContext,
    };

    #[test]
    fn detects_tmux_from_command_target() {
        let session = detect_tmux_session(Some("tmux attach -t model-serving"));
        assert_eq!(session.as_deref(), Some("model-serving"));
    }

    #[test]
    fn shell_metadata_is_fresh_requires_command_boundary() {
        assert!(shell_metadata_is_fresh(true, 0, 0));
        assert!(!shell_metadata_is_fresh(true, 2, 1));
        assert!(!shell_metadata_is_fresh(false, 0, 0));
    }

    #[test]
    fn infer_pane_mode_requires_confirmed_shell_prompt() {
        assert_eq!(infer_pane_mode(None, true, false, 0, 0), PaneMode::Shell);
        assert_eq!(infer_pane_mode(None, true, false, 2, 1), PaneMode::Unknown);
    }

    #[test]
    fn title_and_screen_do_not_create_structured_tmux_state() {
        let observation = PaneObservationFrame {
            title: Some("haswell ❐ 0 ● 4 nvim".to_string()),
            cwd: None,
            recent_output: vec![
                "❐ 0  ↑ 63d 6h 21m  <4 nvim      ↗  | 11:31 | 05 Apr  w  haswell".to_string(),
            ],
            last_command: None,
            last_exit_code: None,
            last_command_duration_secs: None,
            support: PaneObservationSupport::default(),
            has_shell_integration: false,
            is_alt_screen: false,
            is_busy: false,
            input_generation: 0,
            last_command_finished_input_generation: 0,
        };

        let runtime = PaneRuntimeState::from_observation(&observation);
        assert_eq!(runtime.mode, PaneMode::Unknown);
        assert!(runtime.scope_stack.is_empty());
        assert_eq!(runtime.tmux_session, None);
    }

    #[test]
    fn runtime_state_tracks_tmux_from_command_line() {
        let observation = PaneObservationFrame {
            title: Some("tmux".to_string()),
            cwd: Some("/home/w".to_string()),
            recent_output: vec!["".to_string()],
            last_command: Some("tmux attach -t deploy".to_string()),
            last_exit_code: Some(0),
            last_command_duration_secs: Some(1.2),
            support: PaneObservationSupport {
                foreground_command: true,
                ..PaneObservationSupport::default()
            },
            has_shell_integration: true,
            is_alt_screen: false,
            is_busy: true,
            input_generation: 1,
            last_command_finished_input_generation: 0,
        };

        let runtime = PaneRuntimeState::from_observation(&observation);
        assert_eq!(runtime.mode, PaneMode::Multiplexer);
        assert_eq!(runtime.tmux_session.as_deref(), Some("deploy"));
        assert_eq!(
            runtime.active_scope.as_ref().map(PaneRuntimeScope::summary),
            Some("multiplexer(deploy)".to_string())
        );
    }

    #[test]
    fn observer_retains_tmux_scope_until_shell_prompt_returns() {
        let mut observer = PaneRuntimeObserver::default();

        let tmux = PaneObservationFrame {
            title: Some("tmux".to_string()),
            cwd: Some("/home/w".to_string()),
            recent_output: vec!["".to_string()],
            last_command: Some("tmux a -t work".to_string()),
            last_exit_code: None,
            last_command_duration_secs: None,
            support: PaneObservationSupport {
                foreground_command: true,
                ..PaneObservationSupport::default()
            },
            has_shell_integration: true,
            is_alt_screen: false,
            is_busy: true,
            input_generation: 1,
            last_command_finished_input_generation: 0,
        };
        let sparse = PaneObservationFrame {
            title: Some("tmux".to_string()),
            cwd: Some("/home/w".to_string()),
            recent_output: vec!["".to_string()],
            last_command: None,
            last_exit_code: None,
            last_command_duration_secs: None,
            support: PaneObservationSupport::default(),
            has_shell_integration: true,
            is_alt_screen: false,
            is_busy: true,
            input_generation: 1,
            last_command_finished_input_generation: 0,
        };
        let shell = PaneObservationFrame {
            title: Some("bash".to_string()),
            cwd: Some("/Users/weyl/conductor/workspaces/con/kingston".to_string()),
            recent_output: vec!["$".to_string()],
            last_command: Some("cargo test".to_string()),
            last_exit_code: Some(0),
            last_command_duration_secs: Some(1.0),
            support: PaneObservationSupport {
                foreground_command: true,
                ..PaneObservationSupport::default()
            },
            has_shell_integration: true,
            is_alt_screen: false,
            is_busy: false,
            input_generation: 1,
            last_command_finished_input_generation: 1,
        };

        let tmux_runtime = observer.observe(tmux);
        let sparse_runtime = observer.observe(sparse);
        let shell_runtime = observer.observe(shell);

        assert_eq!(tmux_runtime.mode, PaneMode::Multiplexer);
        assert_eq!(sparse_runtime.mode, PaneMode::Multiplexer);
        assert_eq!(shell_runtime.mode, PaneMode::Shell);
        assert_eq!(shell_runtime.tmux_session, None);
    }

    #[test]
    fn runtime_state_tracks_agent_cli_from_command_line() {
        let observation = PaneObservationFrame {
            title: Some("Codex".to_string()),
            cwd: Some("/tmp".to_string()),
            recent_output: vec!["".to_string()],
            last_command: Some("codex".to_string()),
            last_exit_code: None,
            last_command_duration_secs: None,
            support: PaneObservationSupport {
                foreground_command: true,
                ..PaneObservationSupport::default()
            },
            has_shell_integration: true,
            is_alt_screen: false,
            is_busy: true,
            input_generation: 1,
            last_command_finished_input_generation: 0,
        };

        let runtime = PaneRuntimeState::from_observation(&observation);
        assert_eq!(runtime.mode, PaneMode::Tui);
        assert_eq!(runtime.agent_cli.as_deref(), Some("codex"));
        assert_eq!(
            runtime.active_scope.as_ref().map(PaneRuntimeScope::summary),
            Some("agent_cli(codex)".to_string())
        );
    }

    #[test]
    fn alt_screen_creates_strong_interactive_scope() {
        let observation = PaneObservationFrame {
            title: Some("nvim test.sh".to_string()),
            cwd: Some("/tmp".to_string()),
            recent_output: vec!["".to_string()],
            last_command: Some("nvim test.sh".to_string()),
            last_exit_code: None,
            last_command_duration_secs: None,
            support: PaneObservationSupport {
                foreground_command: true,
                alternate_screen: true,
                ..PaneObservationSupport::default()
            },
            has_shell_integration: true,
            is_alt_screen: true,
            is_busy: true,
            input_generation: 1,
            last_command_finished_input_generation: 0,
        };

        let runtime = PaneRuntimeState::from_observation(&observation);
        assert_eq!(runtime.mode, PaneMode::Tui);
        assert_eq!(
            runtime.active_scope,
            Some(PaneRuntimeScope {
                kind: PaneScopeKind::InteractiveApp,
                label: Some("nvim".to_string()),
                host: None,
                confidence: PaneConfidence::Strong,
                evidence_source: PaneEvidenceSource::SurfaceState,
            })
        );
    }

    #[test]
    fn system_prompt_marks_unknown_host_as_unknown_not_local() {
        let runtime = PaneRuntimeState {
            mode: PaneMode::Shell,
            shell_metadata_fresh: false,
            remote_host: None,
            remote_host_confidence: None,
            remote_host_source: None,
            agent_cli: None,
            tmux_session: None,
            active_scope: None,
            evidence: Vec::new(),
            scope_stack: Vec::new(),
            warnings: Vec::new(),
        };
        let prompt = TerminalContext {
            focused_pane_index: 1,
            focused_hostname: None,
            focused_hostname_confidence: None,
            focused_hostname_source: None,
            focused_title: Some("Terminal".to_string()),
            focused_pane_mode: PaneMode::Shell,
            focused_has_shell_integration: false,
            focused_shell_metadata_fresh: false,
            focused_observation_support: PaneObservationSupport::default(),
            focused_runtime_stack: Vec::new(),
            focused_runtime_warnings: Vec::new(),
            focused_control: PaneControlState::from_runtime(&runtime),
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
    fn direct_terminal_exec_requires_fresh_shell() {
        let shell_runtime = PaneRuntimeState {
            mode: PaneMode::Shell,
            shell_metadata_fresh: true,
            remote_host: None,
            remote_host_confidence: None,
            remote_host_source: None,
            agent_cli: None,
            tmux_session: None,
            active_scope: None,
            evidence: Vec::new(),
            scope_stack: Vec::new(),
            warnings: Vec::new(),
        };
        let tmux_runtime = PaneRuntimeState {
            mode: PaneMode::Multiplexer,
            shell_metadata_fresh: false,
            remote_host: None,
            remote_host_confidence: None,
            remote_host_source: None,
            agent_cli: None,
            tmux_session: Some("work".to_string()),
            active_scope: Some(PaneRuntimeScope {
                kind: PaneScopeKind::Multiplexer,
                label: Some("work".to_string()),
                host: None,
                confidence: PaneConfidence::Strong,
                evidence_source: PaneEvidenceSource::CommandLine,
            }),
            evidence: Vec::new(),
            scope_stack: vec![PaneRuntimeScope {
                kind: PaneScopeKind::Multiplexer,
                label: Some("work".to_string()),
                host: None,
                confidence: PaneConfidence::Strong,
                evidence_source: PaneEvidenceSource::CommandLine,
            }],
            warnings: Vec::new(),
        };

        assert!(direct_terminal_exec_is_safe(&shell_runtime));
        assert!(!direct_terminal_exec_is_safe(&tmux_runtime));
    }

    fn make_shell_context() -> TerminalContext {
        let runtime = PaneRuntimeState {
            mode: PaneMode::Shell,
            shell_metadata_fresh: true,
            remote_host: None,
            remote_host_confidence: None,
            remote_host_source: None,
            agent_cli: None,
            tmux_session: None,
            active_scope: None,
            evidence: Vec::new(),
            scope_stack: Vec::new(),
            warnings: Vec::new(),
        };
        TerminalContext {
            focused_pane_index: 1,
            focused_hostname: None,
            focused_hostname_confidence: None,
            focused_hostname_source: None,
            focused_title: None,
            focused_pane_mode: PaneMode::Shell,
            focused_has_shell_integration: true,
            focused_shell_metadata_fresh: true,
            focused_observation_support: PaneObservationSupport::default(),
            focused_runtime_stack: Vec::new(),
            focused_runtime_warnings: Vec::new(),
            focused_control: PaneControlState::from_runtime(&runtime),
            cwd: Some("/home/user".to_string()),
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

    #[test]
    fn tui_guide_absent_for_plain_shell() {
        let ctx = make_shell_context();
        let prompt = ctx.to_system_prompt();
        assert!(
            !prompt.contains("<tui_interaction_guide>"),
            "TUI guide should not appear for plain shell pane"
        );
    }

    #[test]
    fn tui_guide_emitted_for_tmux_focused_pane() {
        let mut ctx = make_shell_context();
        let runtime = PaneRuntimeState {
            mode: PaneMode::Multiplexer,
            shell_metadata_fresh: false,
            remote_host: None,
            remote_host_confidence: None,
            remote_host_source: None,
            agent_cli: None,
            tmux_session: Some("work".to_string()),
            active_scope: Some(PaneRuntimeScope {
                kind: PaneScopeKind::Multiplexer,
                label: Some("work".to_string()),
                host: None,
                confidence: PaneConfidence::Strong,
                evidence_source: PaneEvidenceSource::CommandLine,
            }),
            evidence: Vec::new(),
            scope_stack: vec![PaneRuntimeScope {
                kind: PaneScopeKind::Multiplexer,
                label: Some("work".to_string()),
                host: None,
                confidence: PaneConfidence::Strong,
                evidence_source: PaneEvidenceSource::CommandLine,
            }],
            warnings: Vec::new(),
        };
        ctx.focused_pane_mode = PaneMode::Multiplexer;
        ctx.focused_shell_metadata_fresh = false;
        ctx.focused_control = PaneControlState::from_runtime(&runtime);
        let prompt = ctx.to_system_prompt();
        assert!(
            prompt.contains("<tui_interaction_guide>"),
            "TUI guide should appear when focused pane is tmux"
        );
        assert!(
            prompt.contains("tmux prefix"),
            "Tmux playbook should be included"
        );
        assert!(
            prompt.contains("Verify-after-act"),
            "Verify-after-act should always be included"
        );
    }

    #[test]
    fn title_only_vim_does_not_emit_vim_playbook() {
        let mut ctx = make_shell_context();
        ctx.focused_title = Some("nvim test.sh".to_string());
        let prompt = ctx.to_system_prompt();
        assert!(
            !prompt.contains("vim/nvim interaction"),
            "Vim playbook should not appear from title-only observations"
        );
    }
}
