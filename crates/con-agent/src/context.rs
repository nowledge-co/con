use std::sync::OnceLock;

use serde::{Deserialize, Serialize};

use crate::control::{
    PaneAddressSpace, PaneControlCapability, PaneControlChannel, PaneControlState,
    PaneVisibleTarget, PaneVisibleTargetKind, format_target_stack,
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
    remote_host: Option<StickyFact>,
    multiplexer: Option<StickyFact>,
    tmux_session: Option<StickyFact>,
    agent_cli: Option<StickyFact>,
}

impl PaneRuntimeObserver {
    pub fn observe(&mut self, observation: PaneObservationFrame) -> PaneRuntimeState {
        self.generation += 1;

        let detected_mode = infer_pane_mode(
            observation.title.as_deref(),
            &observation.recent_output,
            observation.last_command.as_deref(),
            observation.has_shell_integration,
            observation.is_alt_screen,
        );

        let shell_metadata_fresh = shell_metadata_is_fresh(
            detected_mode,
            observation.has_shell_integration,
            observation.last_command.as_deref(),
            observation.cwd.as_deref(),
        );

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
        let agent_cli_scope =
            detect_agent_cli_scope(observation.title.as_deref(), &observation.recent_output);

        self.remote_host = Self::merge_fact(
            self.remote_host.take(),
            remote_host_hint.map(|(value, confidence, source)| StickyFact {
                value,
                confidence,
                source,
                generation: self.generation,
            }),
            detected_mode != PaneMode::Shell || !shell_metadata_fresh,
            self.generation,
            12,
        );
        self.multiplexer = Self::merge_fact(
            self.multiplexer.take(),
            tmux_scope.as_ref().map(|scope| StickyFact {
                value: scope.label.clone().unwrap_or_else(|| "tmux".to_string()),
                confidence: scope.confidence,
                source: scope.evidence_source,
                generation: self.generation,
            }),
            detected_mode != PaneMode::Shell,
            self.generation,
            12,
        );
        self.tmux_session = Self::merge_fact(
            self.tmux_session.take(),
            tmux_session.map(|value| StickyFact {
                value,
                confidence: PaneConfidence::Strong,
                source: PaneEvidenceSource::CommandLine,
                generation: self.generation,
            }),
            detected_mode != PaneMode::Shell,
            self.generation,
            12,
        );
        self.agent_cli = Self::merge_fact(
            self.agent_cli.take(),
            agent_cli_scope.as_ref().and_then(|scope| {
                scope.label.clone().map(|value| StickyFact {
                    value,
                    confidence: scope.confidence,
                    source: scope.evidence_source,
                    generation: self.generation,
                })
            }),
            detected_mode != PaneMode::Shell,
            self.generation,
            8,
        );

        let mode = if self.multiplexer.is_some() {
            PaneMode::Multiplexer
        } else if self.agent_cli.is_some() || detected_mode == PaneMode::Tui {
            PaneMode::Tui
        } else {
            detected_mode
        };

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

        let mut evidence = Vec::new();

        if let Some(remote_host) = &self.remote_host {
            scope_stack.push(PaneRuntimeScope {
                kind: PaneScopeKind::RemoteShell,
                label: None,
                host: Some(remote_host.value.clone()),
                confidence: remote_host.confidence,
                evidence_source: remote_host.source,
            });
            evidence.push(PaneEvidence {
                subject: "remote_host".to_string(),
                value: Some(remote_host.value.clone()),
                source: remote_host.source,
                confidence: remote_host.confidence,
                generation: remote_host.generation,
                note: Some("Pane-local host evidence".to_string()),
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
                note: Some("tmux-like pane structure".to_string()),
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
                note: Some("Visible agent CLI chrome".to_string()),
            });
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

        let active_scope = scope_stack.last().cloned();
        let tmux_session = self.tmux_session.as_ref().map(|value| value.value.clone());
        let agent_cli = self.agent_cli.as_ref().map(|value| value.value.clone());
        let remote_host = self.remote_host.as_ref().map(|value| value.value.clone());
        let remote_host_confidence = self.remote_host.as_ref().map(|value| value.confidence);
        let remote_host_source = self.remote_host.as_ref().map(|value| value.source);

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
        if remote_host.is_some() {
            warnings.push(
                "Remote host identity is inferred from terminal metadata and should be verified before destructive actions.".to_string(),
            );
        }

        PaneRuntimeState {
            mode,
            shell_metadata_fresh,
            remote_host,
            remote_host_confidence,
            remote_host_source,
            agent_cli,
            tmux_session,
            active_scope,
            evidence,
            scope_stack,
            warnings,
        }
    }

    fn merge_fact(
        previous: Option<StickyFact>,
        fresh: Option<StickyFact>,
        allow_retention: bool,
        generation: u64,
        ttl: u64,
    ) -> Option<StickyFact> {
        if let Some(fresh) = fresh {
            return Some(fresh);
        }

        match previous {
            Some(previous)
                if allow_retention && generation.saturating_sub(previous.generation) <= ttl =>
            {
                Some(previous)
            }
            _ => None,
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
    if title_lower.contains("tmux") || title_lower.contains("tmate") {
        return true;
    }

    // Detect tmux status indicators commonly embedded in terminal titles.
    // tmux sets the window title to patterns like:
    //   "haswell ❐ 0 ● 4 nvim"
    //   "hostname ❐ session ● N window-name"
    // The ❐ (U+2750) and similar box/window symbols are tmux indicators.
    if title.contains('❐') || title.contains('❑') || title.contains('❏') {
        return true;
    }

    // tmux window-list patterns in titles: "N window-name" with status symbols
    // like ● (active), ○, ◉, * (classic tmux), - (last), # (flagged)
    let tmux_window_markers = ['●', '○', '◉'];
    let has_window_marker = tmux_window_markers.iter().any(|m| title.contains(*m));
    if has_window_marker {
        // Verify there's a digit before/near the marker, suggesting "N● name" or "N name"
        let has_numbered_window = title
            .split_whitespace()
            .any(|token| token.chars().next().is_some_and(|c| c.is_ascii_digit()));
        if has_numbered_window {
            return true;
        }
    }

    false
}

fn line_looks_like_tmux_status(line: &str) -> bool {
    let line = line.trim();
    if line.is_empty() {
        return false;
    }

    // Modern tmux status bar indicators: ❐ (session), ↑ (uptime), ↗ (prefix)
    // Example: "❐ 0  ↑ 63d 6h 21m  <4 nvim      ↗  | 11:31 | 05 Apr  w  haswell"
    if line.contains('❐') || line.contains('❑') || line.contains('❏') {
        return true;
    }

    // Classic tmux: "0:bash  1:vim*  2:htop-" with N:name patterns
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

    // Classic tmux: "0* bash  1 vim  2- htop" with number-name pairs
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
    if numbered_windows >= 2 {
        return true;
    }

    // Modern tmux with ● window markers: "1 model-serving  2 ops 3 nekomaster  4 nvim"
    // or with ↑ uptime indicators
    if line.contains('↑') && line.chars().any(|c| c.is_ascii_digit()) {
        // ↑ followed by an uptime pattern like "63d" or "5h" is very tmux-specific
        let has_uptime = tokens.iter().any(|token| {
            token.ends_with('d') || token.ends_with('h') || token.ends_with('m')
        }) && tokens
            .iter()
            .any(|token| token.chars().all(|c| c.is_ascii_digit()));
        if has_uptime {
            return true;
        }
    }

    false
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
    // Try user@host first (e.g. "user@myserver:~/dir")
    if let Some(host) = host_candidate_from_user_at_host(title) {
        return Some(host);
    }

    // Try tmux-style titles: "hostname ❐ 0 ● 4 nvim"
    // The hostname is the first token, before the tmux session indicator.
    if title_looks_like_tmux(title) {
        let first_token = title.split_whitespace().next()?;
        // The first token should look like a hostname, not a tmux artifact
        let candidate = normalize_host_candidate(first_token)?;
        if !is_local_hostname(&candidate) {
            return Some(candidate);
        }
    }

    None
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
        ("codex cli", "codex"),
        ("codex", "codex"),
        ("open code", "opencode"),
        ("open-code", "opencode"),
        ("opencode", "opencode"),
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
            focused_runtime_stack: Vec::new(),
            focused_runtime_warnings: Vec::new(),
            focused_control: PaneControlState {
                address_space: PaneAddressSpace::ConPane,
                target_stack: vec![PaneVisibleTarget {
                    kind: PaneVisibleTargetKind::UnknownTui,
                    label: None,
                    host: None,
                }],
                visible_target: PaneVisibleTarget {
                    kind: PaneVisibleTargetKind::UnknownTui,
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
             For QUESTIONS about code, errors, or terminal state: prefer reading files and panes. Minimize side effects.\n\
             For TASKS that modify state: verify context first, explain what you will do, then execute carefully.\n\n\
             ## Turn efficiency\n\
             Each tool call counts as a turn. Plan your actions to minimize turns:\n\
             - Batch safe keystrokes in one send_keys: e.g., send_keys \"\\x1bggdG\" (escape + go to top + delete all) is one turn, not three.\n\
             - read_pane is required after mode changes and content input, but sequential keystrokes in the same mode can be batched.\n\
             - For vim content: compose the full content string and send it in one send_keys call (up to ~50 lines), then read_pane once to verify.\n\
             - For tmux navigation: send_keys \"\\x02c\" (prefix + create window) is one call, not two.\n\n\
             ## Tools\n\n\
             <tools>\n\
             - terminal_exec: Run a command visibly in a con pane only when that pane's control capabilities include `exec_visible_shell`.\n\
               `pane_index` always addresses a con pane from list_panes. It never means a tmux pane, tmux window, editor buffer, or remote host.\n\
               Never use terminal_exec on tmux, vim, nvim, dashboards, or other TUIs unless a future tool explicitly supports that runtime.\n\
               ALWAYS use absolute paths for executables when possible.\n\
               Check exit_code in the response — 0 means success, non-zero means failure.\n\
               If exit_code is null, shell integration may be absent or the command is still running.\n\n\
             - batch_exec: Run commands on MULTIPLE con panes in PARALLEL, but only on panes whose control capabilities include `exec_visible_shell`.\n\
               Fastest for multi-pane tasks\n\
               (e.g., \"check uptime on all servers\"). Returns results for each pane independently.\n\n\
             - shell_exec: Run a command in a hidden LOCAL subprocess on the con workspace machine. Output is NOT shown to the user.\n\
               Never use shell_exec to inspect or modify a remote SSH/tmux environment.\n\
               Prefer over terminal_exec only for local tasks such as git status, file searches, package lookups, and background ops.\n\n\
             - list_panes: List all open panes with runtime state plus control state: visible target kind, control channels, capabilities, and addressing notes.\n\n\
             - tmux_inspect: Inspect the tmux adapter state for a con pane that contains a tmux scope. Returns session, tmux control mode, front-most target inside tmux, and why native tmux control is or is not available.\n\n\
             - read_pane: Read last N lines from any pane (includes scrollback). Use to inspect output.\n\n\
             - send_keys: Send raw keystrokes to any pane. For TUI interaction: Ctrl-C (\\x03), arrows, Enter.\n\n\
             - search_panes: Search scrollback across all panes by regex. Find previous errors, output, etc.\n\n\
             - file_read: Read a LOCAL file on the workspace machine. CANNOT access files on remote SSH hosts.\n\
             - file_write: Write a LOCAL file on the workspace machine. CANNOT write to remote SSH hosts.\n\
             - edit_file: Surgical text replacement on LOCAL files only. old_text must match EXACTLY and be UNIQUE.\n\n\
             - list_files: List LOCAL files in a directory. Respects .gitignore. Max 500 entries.\n\
             - search: Search LOCAL file contents by regex pattern. Returns file:line:match triples.\n\
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
             - Observation is not control. Seeing tmux, nvim, Codex CLI, or Claude Code does not mean con has a native control channel for it.\n\
             - Addressing is layered. A con pane index is not a tmux pane id, tmux window index, editor buffer, or remote hostname.\n\
             - When pane mode is not `shell`, or shell metadata is marked stale, do NOT assume cwd/hostname/last_command describe the visible app.\n\
             - terminal_exec and batch_exec are shell-only operations. Use them only when list_panes or the focused pane context exposes `exec_visible_shell`.\n\
             - For tmux, vim, htop, dashboards, Codex CLI, Claude Code, and other TUIs: inspect first and respect the pane's available control channels.\n\
             - If a command fails (exit_code != 0), diagnose the error before retrying.\n\
             - When editing files: always read first, ensure old_text is unique, verify the edit succeeded.\n\
             </safety>\n\n\
             <remote_file_rules>\n\
             CRITICAL: file_read, file_write, edit_file, list_files, and search are LOCAL-ONLY tools.\n\
             They access the workspace machine's filesystem, NOT remote hosts.\n\
             When the focused pane is remote (host is set, or SSH/tmux scope exists):\n\
             - NEVER use file_read to inspect a remote file — use read_pane to see what is on screen.\n\
             - NEVER use file_write/edit_file for remote files — use send_keys to operate the remote editor or shell.\n\
             - To read a remote file: navigate to a shell pane in tmux, then send_keys \"cat path/to/file\\n\" and read_pane.\n\
             - To write a remote file: use send_keys to type content into an open editor (vim/nvim), or\n\
               navigate to a remote shell and use send_keys \"cat > file << 'CONEOF'\\ncontent\\nCONEOF\\n\".\n\
             - To edit a remote file: use send_keys with editor commands (vim :%s, :e, etc.).\n\
             </remote_file_rules>\n\n\
             <verify_before_act>\n\
             MANDATORY: Before ANY send_keys call, you MUST read_pane first to confirm:\n\
             1. What application is currently visible (shell, nvim, tmux window list, etc.)\n\
             2. What mode that application is in (vim normal/insert, tmux prefix, etc.)\n\
             3. Whether your keystrokes will go to the intended target\n\
             After EVERY send_keys call, read_pane again to verify the action took effect.\n\
             Never chain multiple send_keys without reading between them.\n\
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
                prompt.push_str(&format!(
                    "  <pane index=\"{}\" cwd=\"{}\" mode=\"{}\" runtime=\"{}\"{}{}{}{}{}{}{}{}{}{}{}/>\n",
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
                    control_capabilities
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
                | PaneVisibleTargetKind::UnknownTui
        );
        let any_other_pane_tui = self.other_panes.iter().any(|p| {
            matches!(
                p.control.visible_target.kind,
                PaneVisibleTargetKind::InteractiveApp
                    | PaneVisibleTargetKind::TmuxSession
                    | PaneVisibleTargetKind::AgentCli
                    | PaneVisibleTargetKind::UnknownTui
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
                .any(|p| is_vim_target(&p.control.visible_target, p.title.as_deref()));

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
        is_vim_target(&self.focused_control.visible_target, self.focused_title.as_deref())
    }
}

fn is_vim_target(target: &crate::control::PaneVisibleTarget, title: Option<&str>) -> bool {
    if let Some(label) = &target.label {
        let lower = label.to_lowercase();
        if lower.contains("vim") || lower.contains("nvim") || lower.contains("neovim") {
            return true;
        }
    }
    if let Some(title) = title {
        let lower = title.to_lowercase();
        if lower.contains("vim") || lower.contains("nvim") || lower.contains("neovim") {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use crate::control::PaneControlState;

    use super::{
        PaneConfidence, PaneEvidenceSource, PaneMode, PaneObservationFrame, PaneRuntimeObserver,
        PaneRuntimeState, TerminalContext, detect_remote_host_hint, detect_tmux_session,
        direct_terminal_exec_is_safe, host_candidate_from_title, infer_pane_mode,
        line_looks_like_tmux_status, shell_metadata_is_fresh, title_looks_like_tmux,
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
    fn observer_retains_remote_host_across_sparse_tui_frames() {
        let mut observer = PaneRuntimeObserver::default();

        let first = PaneObservationFrame {
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
        let second = PaneObservationFrame {
            title: Some("haswell".to_string()),
            cwd: Some("/home/w".to_string()),
            recent_output: vec!["gpu0".to_string(), "gpu1".to_string()],
            last_command: None,
            last_exit_code: None,
            last_command_duration_secs: None,
            detected_remote_host: None,
            has_shell_integration: false,
            is_alt_screen: false,
            is_busy: false,
        };

        let first_runtime = observer.observe(first);
        let second_runtime = observer.observe(second);

        assert_eq!(first_runtime.remote_host.as_deref(), Some("haswell"));
        assert_eq!(second_runtime.remote_host.as_deref(), Some("haswell"));
        assert_eq!(second_runtime.mode, PaneMode::Multiplexer);
    }

    #[test]
    fn observer_clears_remote_host_when_fresh_local_shell_returns() {
        let mut observer = PaneRuntimeObserver::default();

        let remote_tmux = PaneObservationFrame {
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
        let local_shell = PaneObservationFrame {
            title: Some("kingston".to_string()),
            cwd: Some("/Users/weyl/conductor/workspaces/con/kingston".to_string()),
            recent_output: vec!["cargo test".to_string()],
            last_command: Some("cargo test".to_string()),
            last_exit_code: Some(0),
            last_command_duration_secs: Some(1.0),
            detected_remote_host: None,
            has_shell_integration: true,
            is_alt_screen: false,
            is_busy: false,
        };

        observer.observe(remote_tmux);
        let runtime = observer.observe(local_shell);

        assert_eq!(runtime.remote_host, None);
        assert_eq!(runtime.mode, PaneMode::Shell);
    }

    #[test]
    fn observer_retains_agent_cli_across_sparse_tui_frames() {
        let mut observer = PaneRuntimeObserver::default();

        let first = PaneObservationFrame {
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
        let second = PaneObservationFrame {
            title: Some("Claude Code".to_string()),
            cwd: None,
            recent_output: vec!["Reviewing files".to_string()],
            last_command: None,
            last_exit_code: None,
            last_command_duration_secs: None,
            detected_remote_host: None,
            has_shell_integration: false,
            is_alt_screen: false,
            is_busy: false,
        };

        observer.observe(first);
        let runtime = observer.observe(second);

        assert_eq!(runtime.agent_cli.as_deref(), Some("claude_code"));
        assert_eq!(
            runtime.active_scope.and_then(|scope| scope.label),
            Some("claude_code".to_string())
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
            remote_host: Some("haswell".to_string()),
            remote_host_confidence: Some(PaneConfidence::Advisory),
            remote_host_source: Some(PaneEvidenceSource::ScreenStructure),
            agent_cli: None,
            tmux_session: Some("nvim".to_string()),
            active_scope: None,
            evidence: Vec::new(),
            scope_stack: Vec::new(),
            warnings: Vec::new(),
        };

        assert!(direct_terminal_exec_is_safe(&shell_runtime));
        assert!(!direct_terminal_exec_is_safe(&tmux_runtime));
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

    // ── TUI guide emission tests ───────────────────────────────────

    use super::PaneSummary;

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
            remote_host: Some("haswell".to_string()),
            remote_host_confidence: Some(PaneConfidence::Advisory),
            remote_host_source: Some(PaneEvidenceSource::ScreenStructure),
            agent_cli: None,
            tmux_session: Some("work".to_string()),
            active_scope: None,
            evidence: Vec::new(),
            scope_stack: vec![
                super::PaneRuntimeScope {
                    kind: super::PaneScopeKind::Multiplexer,
                    label: Some("work".to_string()),
                    host: Some("haswell".to_string()),
                    confidence: PaneConfidence::Advisory,
                    evidence_source: PaneEvidenceSource::ScreenStructure,
                },
            ],
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
    fn tui_guide_includes_vim_when_nvim_visible() {
        let mut ctx = make_shell_context();
        ctx.focused_title = Some("nvim test.sh".to_string());
        let runtime = PaneRuntimeState {
            mode: PaneMode::Tui,
            shell_metadata_fresh: false,
            remote_host: None,
            remote_host_confidence: None,
            remote_host_source: None,
            agent_cli: None,
            tmux_session: None,
            active_scope: Some(super::PaneRuntimeScope {
                kind: super::PaneScopeKind::InteractiveApp,
                label: Some("nvim".to_string()),
                host: None,
                confidence: PaneConfidence::Advisory,
                evidence_source: PaneEvidenceSource::Title,
            }),
            evidence: Vec::new(),
            scope_stack: vec![super::PaneRuntimeScope {
                kind: super::PaneScopeKind::InteractiveApp,
                label: Some("nvim".to_string()),
                host: None,
                confidence: PaneConfidence::Advisory,
                evidence_source: PaneEvidenceSource::Title,
            }],
            warnings: Vec::new(),
        };
        ctx.focused_pane_mode = PaneMode::Tui;
        ctx.focused_shell_metadata_fresh = false;
        ctx.focused_control = PaneControlState::from_runtime(&runtime);
        let prompt = ctx.to_system_prompt();
        assert!(
            prompt.contains("<tui_interaction_guide>"),
            "TUI guide should appear when nvim is visible"
        );
        assert!(
            prompt.contains("vim/nvim interaction"),
            "Vim playbook should be included when nvim is in the title"
        );
    }

    #[test]
    fn tui_guide_emitted_when_other_pane_has_tui() {
        let mut ctx = make_shell_context();
        // Focused pane is shell, but another pane has tmux
        let tmux_runtime = PaneRuntimeState {
            mode: PaneMode::Multiplexer,
            shell_metadata_fresh: false,
            remote_host: Some("haswell".to_string()),
            remote_host_confidence: Some(PaneConfidence::Advisory),
            remote_host_source: Some(PaneEvidenceSource::ScreenStructure),
            agent_cli: None,
            tmux_session: Some("work".to_string()),
            active_scope: None,
            evidence: Vec::new(),
            scope_stack: vec![super::PaneRuntimeScope {
                kind: super::PaneScopeKind::Multiplexer,
                label: Some("work".to_string()),
                host: None,
                confidence: PaneConfidence::Advisory,
                evidence_source: PaneEvidenceSource::ScreenStructure,
            }],
            warnings: Vec::new(),
        };
        ctx.other_panes.push(PaneSummary {
            pane_index: 2,
            hostname: Some("haswell".to_string()),
            hostname_confidence: Some(PaneConfidence::Advisory),
            hostname_source: Some(PaneEvidenceSource::ScreenStructure),
            title: Some("tmux".to_string()),
            mode: PaneMode::Multiplexer,
            has_shell_integration: false,
            shell_metadata_fresh: false,
            control: PaneControlState::from_runtime(&tmux_runtime),
            agent_cli: None,
            active_scope: None,
            evidence: Vec::new(),
            runtime_stack: Vec::new(),
            runtime_warnings: Vec::new(),
            tmux_session: Some("work".to_string()),
            cwd: None,
            last_command: None,
            last_exit_code: None,
            is_busy: false,
            recent_output: Vec::new(),
        });
        let prompt = ctx.to_system_prompt();
        assert!(
            prompt.contains("<tui_interaction_guide>"),
            "TUI guide should appear when any other pane has a TUI target"
        );
    }

    #[test]
    fn remote_work_guide_emitted_for_ssh_pane() {
        let mut ctx = make_shell_context();
        ctx.focused_hostname = Some("haswell".to_string());
        ctx.ssh_host = Some("haswell".to_string());
        // Even a shell pane on a remote host should get the remote work guide
        let prompt = ctx.to_system_prompt();
        assert!(
            prompt.contains("<tui_interaction_guide>"),
            "TUI guide should appear for remote SSH pane"
        );
        assert!(
            prompt.contains("LOCAL-ONLY"),
            "Remote work guide should warn about local-only file tools"
        );
        assert!(
            prompt.contains("CANNOT access this remote host"),
            "Remote work guide should explicitly say files can't reach remote"
        );
    }

    #[test]
    fn system_prompt_contains_remote_file_rules_in_safety() {
        let ctx = make_shell_context();
        let prompt = ctx.to_system_prompt();
        assert!(
            prompt.contains("<remote_file_rules>"),
            "Remote file rules should be in the system prompt"
        );
        assert!(
            prompt.contains("NEVER use file_read to inspect a remote file"),
            "Remote file rules should explicitly prohibit file_read on remote"
        );
    }

    #[test]
    fn system_prompt_contains_verify_before_act_in_safety() {
        let ctx = make_shell_context();
        let prompt = ctx.to_system_prompt();
        assert!(
            prompt.contains("<verify_before_act>"),
            "Verify-before-act should be in the safety section"
        );
        assert!(
            prompt.contains("MUST read_pane first"),
            "Verify-before-act should require read_pane before send_keys"
        );
    }

    // ── tmux/SSH detection tests ───────────────────────────────────

    #[test]
    fn title_detects_tmux_from_unicode_indicators() {
        // Real-world tmux title from user's SSH session
        assert!(title_looks_like_tmux("haswell ❐ 0 ● 4 nvim"));
        // Classic tmux/tmate
        assert!(title_looks_like_tmux("tmux: my-session"));
        assert!(title_looks_like_tmux("tmate"));
        // Other Unicode box symbols
        assert!(title_looks_like_tmux("server ❑ 0 ○ 2 bash"));
        assert!(title_looks_like_tmux("host ❏ 1 ◉ 3 htop"));
        // Plain hostname — not tmux
        assert!(!title_looks_like_tmux("haswell"));
        assert!(!title_looks_like_tmux("user@server:/home"));
        // nvim alone is not tmux
        assert!(!title_looks_like_tmux("nvim test.sh"));
    }

    #[test]
    fn status_line_detects_modern_tmux() {
        // Real-world tmux status bar from user's session
        assert!(line_looks_like_tmux_status(
            " ❐ 0  ↑ 63d 6h 21m  <4 nvim      ↗  | 11:31 | 05 Apr  w  haswell "
        ));
        // Classic tmux status
        assert!(line_looks_like_tmux_status("0:bash  1:vim*  2:htop-"));
        // Plain text — not tmux
        assert!(!line_looks_like_tmux_status("$ cargo test"));
        assert!(!line_looks_like_tmux_status(""));
    }

    #[test]
    fn host_extracted_from_tmux_title() {
        // "haswell ❐ 0 ● 4 nvim" — hostname is "haswell"
        let host = host_candidate_from_title("haswell ❐ 0 ● 4 nvim");
        assert_eq!(host.as_deref(), Some("haswell"));
    }

    #[test]
    fn host_extracted_from_user_at_host_title() {
        // SSH sets titles like "user@myserver" (no colon in the token)
        let host = host_candidate_from_title("user@myserver ~/project");
        assert_eq!(host.as_deref(), Some("myserver"));
    }

    #[test]
    fn no_host_from_plain_title() {
        assert_eq!(host_candidate_from_title("nvim test.sh"), None);
        assert_eq!(host_candidate_from_title("Terminal"), None);
    }
}
