use std::collections::VecDeque;

use serde::{Deserialize, Serialize};

use crate::control::{
    PaneAddressSpace, PaneControlCapability, PaneControlChannel, PaneControlState,
    PaneVisibleTarget, PaneVisibleTargetKind, format_control_attachments, format_target_stack,
};
use crate::shell_probe::{ShellProbeResult, ShellProbeTmuxContext};
use crate::tmux::TmuxSnapshot;

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
pub enum PaneFrontState {
    ShellPrompt,
    InteractiveSurface,
    Unknown,
}

impl PaneFrontState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ShellPrompt => "shell_prompt",
            Self::InteractiveSurface => "interactive_surface",
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
    ShellProbe,
    ActionHistory,
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
            Self::ShellProbe => "shell_probe",
            Self::ActionHistory => "action_history",
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

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PaneActionKind {
    PaneCreated,
    VisibleShellExec,
    RawInput,
    ShellProbe,
    ProcessExited,
}

impl PaneActionKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::PaneCreated => "pane_created",
            Self::VisibleShellExec => "visible_shell_exec",
            Self::RawInput => "raw_input",
            Self::ShellProbe => "shell_probe",
            Self::ProcessExited => "process_exited",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PaneActionRecord {
    pub sequence: u64,
    pub kind: PaneActionKind,
    pub summary: String,
    pub source: PaneEvidenceSource,
    pub confidence: PaneConfidence,
    pub input_generation: Option<u64>,
    pub note: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaneShellContext {
    pub captured_input_generation: u64,
    pub host: Option<String>,
    pub pwd: Option<String>,
    pub term: Option<String>,
    pub term_program: Option<String>,
    pub ssh_connection: Option<String>,
    pub ssh_tty: Option<String>,
    pub tmux_env: Option<String>,
    pub nvim_listen_address: Option<String>,
    pub tmux: Option<ShellProbeTmuxContext>,
}

impl PaneShellContext {
    fn from_probe(result: &ShellProbeResult, captured_input_generation: u64) -> Self {
        Self {
            captured_input_generation,
            host: result.host.clone(),
            pwd: result.pwd.clone(),
            term: result.term.clone(),
            term_program: result.term_program.clone(),
            ssh_connection: result.ssh_connection.clone(),
            ssh_tty: result.ssh_tty.clone(),
            tmux_env: result.tmux_env.clone(),
            nvim_listen_address: result.nvim_listen_address.clone(),
            tmux: result.tmux.clone(),
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
    pub screen_hints: Vec<PaneObservationHint>,
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
    pub front_state: PaneFrontState,
    pub mode: PaneMode,
    pub shell_metadata_fresh: bool,
    pub remote_host: Option<String>,
    pub remote_host_confidence: Option<PaneConfidence>,
    pub remote_host_source: Option<PaneEvidenceSource>,
    pub agent_cli: Option<String>,
    pub tmux_session: Option<String>,
    pub last_verified_scope_stack: Vec<PaneRuntimeScope>,
    pub last_verified_tmux_session: Option<String>,
    pub shell_context: Option<PaneShellContext>,
    pub shell_context_fresh: bool,
    pub active_scope: Option<PaneRuntimeScope>,
    pub evidence: Vec<PaneEvidence>,
    pub scope_stack: Vec<PaneRuntimeScope>,
    pub recent_actions: Vec<PaneActionRecord>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PaneObservationHintKind {
    PromptLikeInput,
    HtopLikeScreen,
}

impl PaneObservationHintKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::PromptLikeInput => "prompt_like_input",
            Self::HtopLikeScreen => "htop_like_screen",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PaneObservationHint {
    pub kind: PaneObservationHintKind,
    pub confidence: PaneConfidence,
    pub detail: String,
}

impl PaneRuntimeState {
    pub fn from_observation(observation: &PaneObservationFrame) -> Self {
        PaneRuntimeTracker::default().observe(observation.clone())
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
struct TrackedShellContext {
    result: ShellProbeResult,
    captured_input_generation: u64,
}

#[derive(Debug, Clone)]
pub enum PaneRuntimeEvent {
    PaneCreated {
        startup_command: Option<String>,
    },
    VisibleShellExec {
        command: String,
        input_generation: u64,
    },
    RawInput {
        keys: String,
        input_generation: u64,
    },
    ShellProbe {
        result: ShellProbeResult,
        captured_input_generation: u64,
    },
    ProcessExited,
}

#[derive(Debug, Clone, Default)]
pub struct PaneRuntimeTracker {
    generation: u64,
    event_sequence: u64,
    shell_context: Option<TrackedShellContext>,
    recent_actions: VecDeque<PaneActionRecord>,
}

impl PaneRuntimeTracker {
    pub fn record_action(&mut self, event: PaneRuntimeEvent) {
        self.event_sequence += 1;
        let sequence = self.event_sequence;

        match event {
            PaneRuntimeEvent::PaneCreated { startup_command } => {
                let summary = startup_command
                    .as_ref()
                    .map(|command| {
                        format!("con created this pane with startup command `{command}`")
                    })
                    .unwrap_or_else(|| "con created this pane".to_string());
                self.push_recent_action(PaneActionRecord {
                    sequence,
                    kind: PaneActionKind::PaneCreated,
                    summary,
                    source: PaneEvidenceSource::ActionHistory,
                    confidence: PaneConfidence::Advisory,
                    input_generation: None,
                    note: startup_command.as_deref().and_then(command_intent_note),
                });
            }
            PaneRuntimeEvent::VisibleShellExec {
                command,
                input_generation,
            } => {
                self.push_recent_action(PaneActionRecord {
                    sequence,
                    kind: PaneActionKind::VisibleShellExec,
                    summary: format!("con executed `{command}` in the visible shell"),
                    source: PaneEvidenceSource::ActionHistory,
                    confidence: PaneConfidence::Advisory,
                    input_generation: Some(input_generation),
                    note: command_intent_note(&command),
                });
            }
            PaneRuntimeEvent::RawInput {
                keys,
                input_generation,
            } => {
                self.push_recent_action(PaneActionRecord {
                    sequence,
                    kind: PaneActionKind::RawInput,
                    summary: format!("con sent raw input `{}`", summarize_raw_input(&keys)),
                    source: PaneEvidenceSource::ActionHistory,
                    confidence: PaneConfidence::Advisory,
                    input_generation: Some(input_generation),
                    note: Some(
                        "Raw input describes what con sent to the pane, not what the foreground app proved in response."
                            .to_string(),
                    ),
                });
            }
            PaneRuntimeEvent::ShellProbe {
                result,
                captured_input_generation,
            } => {
                self.shell_context = Some(TrackedShellContext {
                    result: result.clone(),
                    captured_input_generation,
                });
                self.push_recent_action(PaneActionRecord {
                    sequence,
                    kind: PaneActionKind::ShellProbe,
                    summary: summarize_shell_probe(&result),
                    source: PaneEvidenceSource::ShellProbe,
                    confidence: PaneConfidence::Strong,
                    input_generation: Some(captured_input_generation),
                    note: Some(
                        "This probe describes the shell frame that was visible when con ran the probe."
                            .to_string(),
                    ),
                });
            }
            PaneRuntimeEvent::ProcessExited => {
                self.shell_context = None;
                self.push_recent_action(PaneActionRecord {
                    sequence,
                    kind: PaneActionKind::ProcessExited,
                    summary: "the pane process exited".to_string(),
                    source: PaneEvidenceSource::ActionHistory,
                    confidence: PaneConfidence::Strong,
                    input_generation: None,
                    note: None,
                });
            }
        }
    }

    pub fn observe(&mut self, observation: PaneObservationFrame) -> PaneRuntimeState {
        self.generation += 1;
        let generation = self.generation;

        let shell_metadata_fresh = shell_metadata_is_fresh(
            observation.has_shell_integration,
            observation.input_generation,
            observation.last_command_finished_input_generation,
        );

        let shell_context = self.shell_context.as_ref().map(|context| {
            PaneShellContext::from_probe(&context.result, context.captured_input_generation)
        });
        let shell_context_fresh = self.shell_context.as_ref().is_some_and(|context| {
            shell_metadata_fresh
                && observation.input_generation == context.captured_input_generation
        });

        let (
            remote_host,
            remote_host_confidence,
            remote_host_source,
            remote_scope,
            remote_host_evidence,
        ) = remote_host_from_shell_context(
            self.shell_context.as_ref(),
            shell_context_fresh,
            generation,
        );
        let tmux_scope = tmux_scope_from_shell_context(
            self.shell_context.as_ref(),
            shell_context_fresh,
            remote_host.as_deref(),
        );
        let interactive_scope = if observation.support.alternate_screen && observation.is_alt_screen
        {
            Some(PaneRuntimeScope {
                kind: PaneScopeKind::InteractiveApp,
                label: None,
                host: remote_host.clone(),
                confidence: PaneConfidence::Strong,
                evidence_source: PaneEvidenceSource::SurfaceState,
            })
        } else {
            None
        };

        let mut last_verified_scope_stack =
            shell_context_scope_stack(self.shell_context.as_ref(), shell_context_fresh);
        let current_scope_stack = if shell_metadata_fresh {
            let mut stack = Vec::new();
            if let Some(scope) = remote_scope {
                stack.push(scope);
            }
            if let Some(scope) = tmux_scope.clone() {
                stack.push(scope);
            }
            stack.push(PaneRuntimeScope {
                kind: PaneScopeKind::Shell,
                label: None,
                host: remote_host.clone(),
                confidence: PaneConfidence::Strong,
                evidence_source: PaneEvidenceSource::ShellIntegration,
            });
            stack
        } else if let Some(scope) = interactive_scope.clone() {
            vec![scope]
        } else {
            Vec::new()
        };
        if last_verified_scope_stack.is_empty() && shell_metadata_fresh {
            last_verified_scope_stack = current_scope_stack.clone();
        }

        let front_scope = current_scope_stack.last().cloned();
        let front_state = if shell_metadata_fresh {
            PaneFrontState::ShellPrompt
        } else if observation.support.alternate_screen && observation.is_alt_screen {
            PaneFrontState::InteractiveSurface
        } else {
            PaneFrontState::Unknown
        };

        let mode = match front_state {
            PaneFrontState::ShellPrompt => PaneMode::Shell,
            PaneFrontState::InteractiveSurface => PaneMode::Tui,
            PaneFrontState::Unknown => PaneMode::Unknown,
        };

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

        if let Some(evidence_item) = remote_host_evidence {
            evidence.push(evidence_item);
        }

        if let Some(multiplexer) = tmux_scope.as_ref() {
            evidence.push(PaneEvidence {
                subject: "multiplexer".to_string(),
                value: multiplexer.label.clone(),
                source: multiplexer.evidence_source,
                confidence: multiplexer.confidence,
                generation,
                note: Some(
                    "A typed shell probe confirmed that the current shell prompt is nested inside tmux."
                        .to_string(),
                ),
            });
        }

        if let Some(scope) = interactive_scope.as_ref() {
            evidence.push(PaneEvidence {
                subject: "interactive_surface".to_string(),
                value: None,
                source: PaneEvidenceSource::SurfaceState,
                confidence: scope.confidence,
                generation,
                note: Some(
                    "Ghostty reports alternate-screen mode for the visible surface.".to_string(),
                ),
            });
        }

        if let Some(context) = shell_context.as_ref() {
            evidence.push(PaneEvidence {
                subject: "shell_probe".to_string(),
                value: context.host.clone().or_else(|| context.pwd.clone()),
                source: PaneEvidenceSource::ShellProbe,
                confidence: if shell_context_fresh {
                    PaneConfidence::Strong
                } else {
                    PaneConfidence::Advisory
                },
                generation,
                note: Some(if shell_context_fresh {
                    "A typed shell probe matches the current visible shell frame.".to_string()
                } else {
                    "The last typed shell probe describes an earlier shell frame.".to_string()
                }),
            });
        }

        let active_scope = front_scope;
        let tmux_session = current_scope_stack
            .iter()
            .find(|scope| scope.kind == PaneScopeKind::Multiplexer)
            .and_then(|scope| scope.label.clone());
        let last_verified_tmux_session = last_verified_scope_stack
            .iter()
            .find(|scope| scope.kind == PaneScopeKind::Multiplexer)
            .and_then(|scope| scope.label.clone());
        let agent_cli = None;

        let mut warnings = Vec::new();
        if !shell_metadata_fresh {
            warnings.push(
                "Visible shell prompt is not confirmed. Treat cwd and last_command as historical shell metadata, not foreground-app truth.".to_string(),
            );
        }
        if shell_context.is_some() && !shell_context_fresh {
            warnings.push(
                "The last shell probe is historical. It can explain the last verified shell frame, but it does not prove the current foreground target.".to_string(),
            );
        }
        if let Some(note) = observation.support.backend_limit_note() {
            warnings.push(note);
        }
        if current_scope_stack.is_empty() && !last_verified_scope_stack.is_empty() {
            warnings.push(format!(
                "Last verified shell frame: {}.",
                format_runtime_stack(&last_verified_scope_stack)
            ));
        }

        PaneRuntimeState {
            front_state,
            mode,
            shell_metadata_fresh,
            remote_host,
            remote_host_confidence,
            remote_host_source,
            agent_cli,
            tmux_session,
            last_verified_scope_stack,
            last_verified_tmux_session,
            shell_context,
            shell_context_fresh,
            active_scope,
            evidence,
            scope_stack: current_scope_stack,
            recent_actions: self.recent_actions.iter().cloned().collect(),
            warnings,
        }
    }

    fn push_recent_action(&mut self, action: PaneActionRecord) {
        const MAX_RECENT_ACTIONS: usize = 8;
        self.recent_actions.push_back(action);
        while self.recent_actions.len() > MAX_RECENT_ACTIONS {
            self.recent_actions.pop_front();
        }
    }
}

fn summarize_raw_input(keys: &str) -> String {
    let escaped: String = keys.chars().flat_map(char::escape_default).collect();
    let mut preview = escaped;
    if preview.len() > 48 {
        preview.truncate(48);
        preview.push_str("...");
    }
    preview
}

fn summarize_shell_probe(result: &ShellProbeResult) -> String {
    let mut parts = Vec::new();
    if let Some(host) = &result.host {
        parts.push(format!("host `{host}`"));
    }
    if let Some(tmux) = &result.tmux {
        if let Some(session) = &tmux.session_name {
            parts.push(format!("tmux session `{session}`"));
        } else {
            parts.push("tmux context".to_string());
        }
        if let Some(pane) = &tmux.pane_id {
            parts.push(format!("pane `{pane}`"));
        }
    }
    if let Some(path) = &result.nvim_listen_address {
        parts.push(format!("nvim socket `{path}`"));
    }

    if parts.is_empty() {
        "con probed the visible shell context".to_string()
    } else {
        format!(
            "con probed the visible shell context and captured {}",
            parts.join(", ")
        )
    }
}

fn command_intent_note(command: &str) -> Option<String> {
    if looks_like_tmux_command(command) {
        return Some(
            "This command targets tmux/tmate. Treat it as causal history about how con entered a multiplexer, not as proof that tmux is still front-most now."
                .to_string(),
        );
    }

    if is_agent_cli_command(command) {
        return Some(
            "This command launched a supported agent CLI. Treat it as causal history until newer pane evidence proves what is currently visible."
                .to_string(),
        );
    }

    if command_basename(command).as_deref() == Some("ssh") {
        return Some(
            "This command opened an SSH connection. It is useful history, but remote identity is only proven after a typed shell probe or backend export."
                .to_string(),
        );
    }

    None
}

fn remote_host_from_shell_context(
    shell_context: Option<&TrackedShellContext>,
    shell_context_fresh: bool,
    generation: u64,
) -> (
    Option<String>,
    Option<PaneConfidence>,
    Option<PaneEvidenceSource>,
    Option<PaneRuntimeScope>,
    Option<PaneEvidence>,
) {
    let Some(context) = shell_context else {
        return (None, None, None, None, None);
    };
    let Some(host) = context.result.host.clone() else {
        return (None, None, None, None, None);
    };
    if context.result.ssh_connection.is_none() {
        return (None, None, None, None, None);
    }

    let confidence = if shell_context_fresh {
        PaneConfidence::Strong
    } else {
        PaneConfidence::Advisory
    };
    let source = PaneEvidenceSource::ShellProbe;

    (
        if shell_context_fresh {
            Some(host.clone())
        } else {
            None
        },
        if shell_context_fresh {
            Some(confidence)
        } else {
            None
        },
        if shell_context_fresh {
            Some(source)
        } else {
            None
        },
        Some(PaneRuntimeScope {
            kind: PaneScopeKind::RemoteShell,
            label: Some(host.clone()),
            host: Some(host.clone()),
            confidence,
            evidence_source: source,
        }),
        Some(PaneEvidence {
            subject: "remote_host".to_string(),
            value: Some(host),
            source,
            confidence,
            generation,
            note: Some(if shell_context_fresh {
                "The last shell probe confirmed that the visible shell is running on a remote host."
                    .to_string()
            } else {
                "A historical shell probe previously confirmed remote shell context for this pane."
                    .to_string()
            }),
        }),
    )
}

fn shell_context_scope_stack(
    shell_context: Option<&TrackedShellContext>,
    shell_context_fresh: bool,
) -> Vec<PaneRuntimeScope> {
    let Some(context) = shell_context else {
        return Vec::new();
    };

    let confidence = if shell_context_fresh {
        PaneConfidence::Strong
    } else {
        PaneConfidence::Advisory
    };
    let host = context
        .result
        .host
        .clone()
        .filter(|_| context.result.ssh_connection.is_some());

    let mut scopes = Vec::new();
    if let Some(host) = host.clone() {
        scopes.push(PaneRuntimeScope {
            kind: PaneScopeKind::RemoteShell,
            label: Some(host.clone()),
            host: Some(host),
            confidence,
            evidence_source: PaneEvidenceSource::ShellProbe,
        });
    }
    if let Some(tmux) = &context.result.tmux {
        scopes.push(PaneRuntimeScope {
            kind: PaneScopeKind::Multiplexer,
            label: tmux
                .session_name
                .clone()
                .or_else(|| Some("tmux".to_string())),
            host: host.clone(),
            confidence,
            evidence_source: PaneEvidenceSource::ShellProbe,
        });
    }
    scopes.push(PaneRuntimeScope {
        kind: PaneScopeKind::Shell,
        label: None,
        host,
        confidence,
        evidence_source: PaneEvidenceSource::ShellProbe,
    });
    scopes
}

fn tmux_scope_from_shell_context(
    shell_context: Option<&TrackedShellContext>,
    shell_context_fresh: bool,
    remote_host: Option<&str>,
) -> Option<PaneRuntimeScope> {
    let context = shell_context?;
    let tmux = context.result.tmux.as_ref()?;
    if !shell_context_fresh {
        return None;
    }

    Some(PaneRuntimeScope {
        kind: PaneScopeKind::Multiplexer,
        label: tmux
            .session_name
            .clone()
            .or_else(|| Some("tmux".to_string())),
        host: remote_host.map(str::to_string),
        confidence: PaneConfidence::Strong,
        evidence_source: PaneEvidenceSource::ShellProbe,
    })
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

pub fn detect_tmux_session(last_command: Option<&str>) -> Option<String> {
    last_command
        .filter(|command| looks_like_tmux_command(command))
        .and_then(parse_tmux_target)
}

fn is_agent_cli_command(command: &str) -> bool {
    command_basename(command).as_deref().is_some_and(|name| {
        matches!(
            name,
            "claude" | "claude-code" | "codex" | "opencode" | "open-code"
        )
    })
}

pub fn infer_pane_mode(
    _last_command: Option<&str>,
    has_shell_integration: bool,
    is_alt_screen: bool,
    input_generation: u64,
    last_command_finished_input_generation: u64,
) -> PaneMode {
    if is_alt_screen {
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
    /// Current verified front-state for the focused pane.
    pub focused_front_state: PaneFrontState,
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
    /// Last verified shell-frame stack for the focused pane.
    pub focused_last_verified_runtime_stack: Vec<PaneRuntimeScope>,
    /// Warnings that should constrain interpretation of the focused pane.
    pub focused_runtime_warnings: Vec<String>,
    /// Typed control contract for the focused pane.
    pub focused_control: PaneControlState,
    /// Last typed shell-context snapshot for the focused pane, when available.
    pub focused_shell_context: Option<PaneShellContext>,
    /// Whether the typed shell-context snapshot still matches the current shell frame.
    pub focused_shell_context_fresh: bool,
    /// Recent con-originated actions on the focused pane.
    pub focused_recent_actions: Vec<PaneActionRecord>,
    /// Current working directory (from OSC 7 or manual detection)
    pub cwd: Option<String>,
    /// Last N lines of terminal output
    pub recent_output: Vec<String>,
    /// Weak observation hints derived from the current visible screen snapshot.
    pub focused_screen_hints: Vec<PaneObservationHint>,
    /// Structured tmux target inventory from a proven same-session tmux control anchor.
    pub focused_tmux_snapshot: Option<TmuxSnapshot>,
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
    pub front_state: PaneFrontState,
    pub mode: PaneMode,
    pub has_shell_integration: bool,
    pub shell_metadata_fresh: bool,
    pub observation_support: PaneObservationSupport,
    pub control: PaneControlState,
    pub agent_cli: Option<String>,
    pub active_scope: Option<PaneRuntimeScope>,
    pub evidence: Vec<PaneEvidence>,
    pub runtime_stack: Vec<PaneRuntimeScope>,
    pub last_verified_runtime_stack: Vec<PaneRuntimeScope>,
    pub runtime_warnings: Vec<String>,
    pub tmux_session: Option<String>,
    pub cwd: Option<String>,
    pub screen_hints: Vec<PaneObservationHint>,
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

pub fn derive_screen_hints(lines: &[String]) -> Vec<PaneObservationHint> {
    let mut hints = Vec::new();

    let non_empty: Vec<&str> = lines
        .iter()
        .map(|line| line.trim_end())
        .filter(|line| !line.trim().is_empty())
        .collect();

    if let Some(line) = non_empty
        .iter()
        .rev()
        .take(3)
        .find(|line| is_prompt_like_line(line))
    {
        hints.push(PaneObservationHint {
            kind: PaneObservationHintKind::PromptLikeInput,
            confidence: PaneConfidence::Advisory,
            detail: format!(
                "A prompt-like input line is visible near the bottom of the current screen: `{}`.",
                line.trim()
            ),
        });
    }

    let htop_markers = [
        "Load average:",
        "Tasks:",
        "PID USER",
        "TIME+  Command",
        "Swp[",
        "Mem[",
    ];
    let marker_count = htop_markers
        .iter()
        .filter(|marker| lines.iter().any(|line| line.contains(**marker)))
        .count();
    if marker_count >= 2 {
        hints.push(PaneObservationHint {
            kind: PaneObservationHintKind::HtopLikeScreen,
            confidence: PaneConfidence::Advisory,
            detail: "The current visible screen resembles htop output.".to_string(),
        });
    }

    hints
}

fn is_prompt_like_line(line: &str) -> bool {
    let trimmed = line.trim();
    if trimmed.is_empty() || trimmed.len() > 100 || trimmed.contains("Expected:") {
        return false;
    }
    if trimmed.contains("  ") {
        return false;
    }

    let starts_like_prompt = trimmed
        .chars()
        .next()
        .is_some_and(|c| matches!(c, '$' | '#' | '%' | '>' | ')' | '❯'));
    let ends_like_prompt = trimmed
        .chars()
        .last()
        .is_some_and(|c| matches!(c, '$' | '#' | '%' | '>'));

    starts_like_prompt || ends_like_prompt
}

fn focused_screen_assessment(
    hints: &[PaneObservationHint],
    control: &PaneControlState,
    tmux_snapshot: Option<&TmuxSnapshot>,
) -> Option<String> {
    if hints.is_empty() {
        return None;
    }

    let has_prompt = hints
        .iter()
        .any(|hint| hint.kind == PaneObservationHintKind::PromptLikeInput);
    let has_htop = hints
        .iter()
        .any(|hint| hint.kind == PaneObservationHintKind::HtopLikeScreen);

    let summary = match (has_prompt, has_htop) {
        (true, true) => {
            "The current visible screen shows prompt-like input near the bottom while htop-like content is also visible above. Treat this as mixed current-screen evidence, not proof of the true foreground app."
        }
        (true, false) => {
            "The current visible screen appears to be at a prompt-like input state, but backend facts still do not prove that the foreground target is truly a shell."
        }
        (false, true) => {
            "The current visible screen appears to resemble htop output, but that remains a screen observation rather than authoritative foreground-process proof."
        }
        (false, false) => return None,
    };

    let next_step = if tmux_snapshot.is_some() {
        "A stronger tmux-native fact source is already attached for this pane."
    } else if control
        .capabilities
        .contains(&PaneControlCapability::QueryTmux)
    {
        "A stronger next step is available: query tmux targets through the existing tmux control anchor."
    } else if control.allows_shell_probe() {
        "A stronger next step is available: run a shell probe to collect shell-scoped facts."
    } else {
        "No stronger read-only fact source is currently available because a proven fresh shell prompt or tmux control anchor is not established."
    };

    Some(format!("{summary} {next_step}"))
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
            focused_front_state: PaneFrontState::Unknown,
            focused_pane_mode: PaneMode::Unknown,
            focused_has_shell_integration: false,
            focused_shell_metadata_fresh: false,
            focused_observation_support: PaneObservationSupport::default(),
            focused_runtime_stack: Vec::new(),
            focused_last_verified_runtime_stack: Vec::new(),
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
                attachments: Vec::new(),
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
            focused_shell_context: None,
            focused_shell_context_fresh: false,
            focused_recent_actions: Vec::new(),
            cwd: None,
            recent_output: Vec::new(),
            focused_screen_hints: Vec::new(),
            focused_tmux_snapshot: None,
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
                These travel through the PTY to the remote program. send_keys is correct for direct TUI interaction, \
                but when a pane exposes tmux native control you should prefer tmux_list_targets / tmux_capture_pane / tmux_send_keys over outer-pane PTY keystrokes.\n\
             3. **Shell commands** (ls, apt update, git status):\n\
                Use terminal_exec when `exec_visible_shell` is available. When it is NOT, first observe the pane \
                and only use send_keys if a shell prompt is visibly present.\n\n\
             ### Choose the right tool\n\
             - SHELL COMMANDS on a pane with `exec_visible_shell` → terminal_exec / batch_exec.\n\
             - READ-ONLY SHELL INTROSPECTION on a pane with `probe_shell_context` → probe_shell_context.\n\
             - CURRENT TERMINAL SITUATION questions (\"where am I?\", \"am I in tmux?\", \"what host is this?\") → use the provided focused-pane context first, including any `shell_context` or `tmux_snapshot`. Only call list_panes / probe_shell_context / tmux_list_targets when a stronger fact source is still needed.\n\
             - For CURRENT TERMINAL SITUATION answers, structure the response as: proven facts, current-screen assessment, and unknowns/limits. Use `screen_hints` and `terminal_output` to describe what appears on screen now without promoting it to backend truth.\n\
             - TMUX TARGET DISCOVERY on a pane with tmux native control → tmux_list_targets, then tmux_capture_pane.\n\
             - TMUX NATIVE INTERACTION on a pane with tmux native control → tmux_send_keys to a specific tmux pane target.\n\
             - TMUX WITHOUT native control → read_pane first, then outer-pane send_keys only as a fallback.\n\
             - PARALLEL WORK across hosts → create_pane for each host (output shows connection state), \
               then terminal_exec (if exec_visible_shell) or send_keys.\n\
             - SHELL COMMANDS on a pane WITHOUT `exec_visible_shell` → use read_pane first, then send_keys \"command\\n\" only if a shell prompt is visibly present.\n\
             - LONG-RUNNING commands → launch, then wait_for (not repeated read_pane).\n\
             - INTERACTIVE TUI (vim, htop, menus) → send_keys + read_pane (follow playbooks).\n\
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
             - probe_shell_context: Run a read-only shell-scoped probe in a pane with a proven fresh shell prompt. \
               Use this to gather authoritative shell facts such as hostname, SSH env, tmux env, tmux session/window/pane ids, \
               and NVIM_LISTEN_ADDRESS. This is shell-scope truth, not foreground-app truth after control passes to a TUI.\n\n\
             - read_pane: Read last N lines from any pane (includes scrollback).\n\n\
             - send_keys: Send keystrokes to a pane's PTY. Use for:\n\
               (a) direct TUI interaction when there is no stronger native attachment.\n\
               (b) shell commands on panes without exec_visible_shell when a shell prompt is visibly present.\n\
               (c) tmux prefix sequences ONLY when tmux native control is unavailable.\n\
               NEVER use to simulate con application shortcuts (Cmd+T, Cmd+W).\n\n\
             - wait_for: Wait for a pane to become idle or for a pattern to appear. Use after launching \
               long-running commands instead of polling with read_pane. Idle mode (no pattern) works universally — \
               shell integration for precise detection, output quiescence as fallback. On timeout, \
               read_pane to check progress and call wait_for again.\n\n\
             - tmux_inspect: Inspect tmux adapter state for a pane containing a tmux session.\n\
             - tmux_list_targets: List tmux windows/panes through a proven same-session tmux shell anchor.\n\
             - tmux_capture_pane: Capture the content of a specific tmux pane target without confusing it with the outer con pane.\n\
             - tmux_send_keys: Send text or tmux key names to a specific tmux pane target through a proven same-session tmux shell anchor.\n\
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
             - Action history is causal evidence, not foreground truth. Use recent con actions to understand how a pane was reached, but rely on current probes and control capabilities before acting.\n\
             - `runtime_stack` is current-only. `last_verified_shell_stack` describes the most recent shell frame con verified, and may be historical.\n\
             - Prefer typed attachments and probes over inference. If `probe_shell_context` is available, use it before guessing about SSH, tmux, or editor context.\n\
             - Backend support is explicit. If `supports_foreground_command`, `supports_alt_screen`, or `supports_remote_host_identity` is false, treat missing runtime data as unavailable backend truth, not as proof of absence.\n\
             - `screen_hints` are weak observations derived from the current visible screen snapshot. They can describe what appears to be on screen now, but they are not backend facts and must not unlock control.\n\
             - Do not end a session-state answer with a vague offer to \"inspect more closely\". Only propose a next step when a stronger fact source is concretely available, such as `probe_shell_context`, `tmux_snapshot`, or tmux native tools already present on the pane.\n\
             - Addressing is layered. A con pane index ≠ tmux pane id ≠ tmux window index ≠ editor buffer.\n\
             - If list_panes shows `query_tmux` or `send_tmux_keys`, treat tmux as a native attachment. Do NOT navigate tmux by outer-pane send_keys unless the native path is unavailable.\n\
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
            "<focused_pane index=\"{}\" front_state=\"{}\" mode=\"{}\" shell_integration=\"{}\" shell_metadata_fresh=\"{}\" shell_context_fresh=\"{}\"",
            self.focused_pane_index,
            self.focused_front_state.as_str(),
            self.focused_pane_mode.as_str(),
            self.focused_has_shell_integration,
            self.focused_shell_metadata_fresh,
            self.focused_shell_context_fresh,
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
            "<focused_control address_space=\"{}\" visible_target=\"{}\" target_stack=\"{}\" attachments=\"{}\" channels=\"{}\" capabilities=\"{}\"",
            self.focused_control.address_space.as_str(),
            xml_escape(&self.focused_control.visible_target.summary()),
            xml_escape(&format_target_stack(&self.focused_control.target_stack)),
            xml_escape(&format_control_attachments(&self.focused_control.attachments)),
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
        if let Some(context) = &self.focused_shell_context {
            prompt.push_str("<shell_context");
            prompt.push_str(&format!(
                " captured_input_generation=\"{}\" fresh=\"{}\"",
                context.captured_input_generation, self.focused_shell_context_fresh
            ));
            if let Some(host) = &context.host {
                prompt.push_str(&format!(" host=\"{}\"", xml_escape(host)));
            }
            if let Some(pwd) = &context.pwd {
                prompt.push_str(&format!(" pwd=\"{}\"", xml_escape(pwd)));
            }
            if let Some(tmux) = &context.tmux {
                if let Some(session) = &tmux.session_name {
                    prompt.push_str(&format!(" tmux_session=\"{}\"", xml_escape(session)));
                }
                if let Some(pane_id) = &tmux.pane_id {
                    prompt.push_str(&format!(" tmux_pane=\"{}\"", xml_escape(pane_id)));
                }
                if let Some(command) = &tmux.pane_current_command {
                    prompt.push_str(&format!(
                        " tmux_pane_current_command=\"{}\"",
                        xml_escape(command)
                    ));
                }
            }
            if let Some(nvim) = &context.nvim_listen_address {
                prompt.push_str(&format!(" nvim_socket=\"{}\"", xml_escape(nvim)));
            }
            prompt.push_str("/>\n");
        }
        if let Some(snapshot) = &self.focused_tmux_snapshot {
            prompt.push_str("<tmux_snapshot>\n");
            for pane in &snapshot.panes {
                prompt.push_str(&format!(
                    "  <tmux_pane session=\"{}\" window_id=\"{}\" window_name=\"{}\" pane_id=\"{}\" pane_index=\"{}\" active=\"{}\"",
                    xml_escape(&pane.session_name),
                    xml_escape(&pane.window_id),
                    xml_escape(&pane.window_name),
                    xml_escape(&pane.pane_id),
                    xml_escape(&pane.pane_index),
                    pane.pane_active,
                ));
                if let Some(command) = &pane.pane_current_command {
                    prompt.push_str(&format!(" current_command=\"{}\"", xml_escape(command)));
                }
                if let Some(path) = &pane.pane_current_path {
                    prompt.push_str(&format!(" current_path=\"{}\"", xml_escape(path)));
                }
                prompt.push_str("/>\n");
            }
            prompt.push_str("</tmux_snapshot>\n");
        }
        if !self.focused_recent_actions.is_empty() {
            prompt.push_str("<recent_actions>\n");
            for action in &self.focused_recent_actions {
                prompt.push_str(&format!(
                    "  <action kind=\"{}\" source=\"{}\" confidence=\"{}\"",
                    action.kind.as_str(),
                    action.source.as_str(),
                    action.confidence.as_str(),
                ));
                if let Some(input_generation) = action.input_generation {
                    prompt.push_str(&format!(" input_generation=\"{}\"", input_generation));
                }
                prompt.push_str(">");
                prompt.push_str(&xml_escape(&action.summary));
                prompt.push_str("</action>\n");
            }
            prompt.push_str("</recent_actions>\n");
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
        if !self.focused_last_verified_runtime_stack.is_empty() {
            prompt.push_str("<last_verified_shell_stack>\n");
            for scope in &self.focused_last_verified_runtime_stack {
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
            prompt.push_str("</last_verified_shell_stack>\n");
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
                "  <pane index=\"{}\" focused=\"true\" cwd=\"{}\" front_state=\"{}\" mode=\"{}\" shell_metadata_fresh=\"{}\" runtime=\"{}\" last_verified_shell_stack=\"{}\" control_target=\"{}\" target_stack=\"{}\" control_attachments=\"{}\" control_channels=\"{}\" control_capabilities=\"{}\"",
                self.focused_pane_index,
                xml_escape(cwd_label),
                self.focused_front_state.as_str(),
                self.focused_pane_mode.as_str(),
                self.focused_shell_metadata_fresh,
                xml_escape(&format_runtime_stack(&self.focused_runtime_stack)),
                xml_escape(&format_runtime_stack(&self.focused_last_verified_runtime_stack)),
                xml_escape(&self.focused_control.visible_target.summary()),
                xml_escape(&format_target_stack(&self.focused_control.target_stack)),
                xml_escape(&format_control_attachments(&self.focused_control.attachments)),
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
                let control_attachments = format!(
                    " control_attachments=\"{}\"",
                    xml_escape(&format_control_attachments(&pane.control.attachments))
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
                    "  <pane index=\"{}\" cwd=\"{}\" front_state=\"{}\" mode=\"{}\" runtime=\"{}\" last_verified_shell_stack=\"{}\"{}{}{}{}{}{}{}{}{}{}{}{}{}/>\n",
                    pane.pane_index,
                    xml_escape(cwd),
                    pane.front_state.as_str(),
                    pane.mode.as_str(),
                    xml_escape(&format_runtime_stack(&pane.runtime_stack)),
                    xml_escape(&format_runtime_stack(&pane.last_verified_runtime_stack)),
                    host,
                    host_confidence,
                    active_scope,
                    busy,
                    stale,
                    tmux,
                    control_target,
                    target_stack,
                    tmux_control,
                    control_attachments,
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
            prompt.push_str("<terminal_output source=\"current_visible_screen\">\n");
            for line in &self.recent_output {
                prompt.push_str(&xml_escape(line));
                prompt.push('\n');
            }
            prompt.push_str("</terminal_output>\n");
        }
        if !self.focused_screen_hints.is_empty() {
            prompt.push_str("<screen_hints>\n");
            for hint in &self.focused_screen_hints {
                prompt.push_str(&format!(
                    "  <hint kind=\"{}\" confidence=\"{}\">{}</hint>\n",
                    hint.kind.as_str(),
                    hint.confidence.as_str(),
                    xml_escape(&hint.detail)
                ));
            }
            prompt.push_str("</screen_hints>\n");
        }
        if let Some(assessment) = focused_screen_assessment(
            &self.focused_screen_hints,
            &self.focused_control,
            self.focused_tmux_snapshot.as_ref(),
        ) {
            prompt.push_str("<current_screen_assessment>");
            prompt.push_str(&xml_escape(&assessment));
            prompt.push_str("</current_screen_assessment>\n");
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
        let is_remote = self.focused_hostname.is_some()
            || self.ssh_host.is_some()
            || self
                .focused_shell_context
                .as_ref()
                .is_some_and(|context| context.ssh_connection.is_some());
        let has_tmux = self
            .focused_control
            .target_stack
            .iter()
            .any(|t| t.kind == PaneVisibleTargetKind::TmuxSession)
            || self
                .focused_last_verified_runtime_stack
                .iter()
                .any(|scope| scope.kind == PaneScopeKind::Multiplexer)
            || self.other_panes.iter().any(|p| {
                p.control
                    .target_stack
                    .iter()
                    .any(|t| t.kind == PaneVisibleTargetKind::TmuxSession)
                    || p.last_verified_runtime_stack
                        .iter()
                        .any(|scope| scope.kind == PaneScopeKind::Multiplexer)
            });
        let has_vim = self.has_vim_visible()
            || self
                .other_panes
                .iter()
                .any(|p| is_vim_target(&p.control.visible_target));

        if !focused_is_tui && !any_other_pane_tui && !is_remote && !has_tmux && !has_vim {
            return;
        }

        prompt.push_str("<tui_interaction_guide>\n");

        // Remote work rules come first — they change what tools are valid
        if is_remote {
            prompt.push_str(playbooks::REMOTE_WORK);
            prompt.push('\n');
        }

        if focused_is_tui || any_other_pane_tui || has_tmux || has_vim {
            prompt.push_str(playbooks::VERIFY_AFTER_ACT);
            prompt.push('\n');
        }

        // Check for tmux anywhere in focused stack or other panes
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
    use crate::shell_probe::{ShellProbeResult, ShellProbeTmuxContext};
    use crate::tmux::{TmuxPaneInfo, TmuxSnapshot};

    use super::{
        PaneConfidence, PaneEvidenceSource, PaneFrontState, PaneMode, PaneObservationFrame,
        PaneObservationHintKind, PaneObservationSupport, PaneRuntimeEvent, PaneRuntimeScope,
        PaneRuntimeState, PaneRuntimeTracker, PaneScopeKind, TerminalContext, derive_screen_hints,
        detect_tmux_session, direct_terminal_exec_is_safe, infer_pane_mode,
        shell_metadata_is_fresh,
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
            screen_hints: Vec::new(),
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
    fn runtime_state_keeps_tmux_command_history_out_of_foreground_state() {
        let observation = PaneObservationFrame {
            title: Some("tmux".to_string()),
            cwd: Some("/home/w".to_string()),
            recent_output: vec!["".to_string()],
            screen_hints: Vec::new(),
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
        assert_eq!(runtime.front_state, PaneFrontState::Unknown);
        assert_eq!(runtime.mode, PaneMode::Unknown);
        assert!(runtime.scope_stack.is_empty());
        assert!(runtime.last_verified_scope_stack.is_empty());
    }

    #[test]
    fn observer_does_not_promote_tmux_command_history_to_foreground_state() {
        let mut observer = PaneRuntimeTracker::default();

        let tmux = PaneObservationFrame {
            title: Some("tmux".to_string()),
            cwd: Some("/home/w".to_string()),
            recent_output: vec!["".to_string()],
            screen_hints: Vec::new(),
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
            screen_hints: Vec::new(),
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
            screen_hints: Vec::new(),
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

        assert_eq!(tmux_runtime.mode, PaneMode::Unknown);
        assert_eq!(sparse_runtime.mode, PaneMode::Unknown);
        assert_eq!(shell_runtime.mode, PaneMode::Shell);
        assert_eq!(shell_runtime.tmux_session, None);
    }

    #[test]
    fn runtime_state_keeps_agent_cli_history_out_of_foreground_state() {
        let observation = PaneObservationFrame {
            title: Some("Codex".to_string()),
            cwd: Some("/tmp".to_string()),
            recent_output: vec!["".to_string()],
            screen_hints: Vec::new(),
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
        assert_eq!(runtime.front_state, PaneFrontState::Unknown);
        assert_eq!(runtime.mode, PaneMode::Unknown);
        assert_eq!(runtime.agent_cli, None);
        assert_eq!(runtime.active_scope, None);
    }

    #[test]
    fn shell_probe_turns_tmux_shell_into_nested_runtime_stack() {
        let mut tracker = PaneRuntimeTracker::default();
        tracker.record_action(PaneRuntimeEvent::ShellProbe {
            result: ShellProbeResult {
                host: Some("haswell".to_string()),
                pwd: Some("/home/weyl".to_string()),
                term: Some("xterm-ghostty".to_string()),
                term_program: Some("con".to_string()),
                ssh_connection: Some("1.2.3.4 5555 5.6.7.8 22".to_string()),
                ssh_tty: Some("/dev/pts/7".to_string()),
                tmux_env: Some("/tmp/tmux-1000/default,123,0".to_string()),
                nvim_listen_address: None,
                tmux: Some(ShellProbeTmuxContext {
                    session_name: Some("work".to_string()),
                    window_id: Some("@3".to_string()),
                    window_name: Some("shell".to_string()),
                    pane_id: Some("%17".to_string()),
                    pane_current_command: Some("zsh".to_string()),
                    pane_current_path: Some("/home/weyl".to_string()),
                    client_tty: Some("/dev/pts/7".to_string()),
                }),
                facts: Default::default(),
            },
            captured_input_generation: 3,
        });

        let observation = PaneObservationFrame {
            title: Some("zsh".to_string()),
            cwd: Some("/home/weyl".to_string()),
            recent_output: vec!["$".to_string()],
            screen_hints: Vec::new(),
            last_command: Some("ls".to_string()),
            last_exit_code: Some(0),
            last_command_duration_secs: Some(0.1),
            support: PaneObservationSupport::default(),
            has_shell_integration: true,
            is_alt_screen: false,
            is_busy: false,
            input_generation: 3,
            last_command_finished_input_generation: 3,
        };

        let runtime = tracker.observe(observation);
        let control = PaneControlState::from_runtime(&runtime);

        assert_eq!(runtime.mode, PaneMode::Shell);
        assert_eq!(runtime.front_state, PaneFrontState::ShellPrompt);
        assert_eq!(runtime.remote_host.as_deref(), Some("haswell"));
        assert_eq!(runtime.tmux_session.as_deref(), Some("work"));
        assert!(runtime.shell_context_fresh);
        assert_eq!(
            runtime
                .scope_stack
                .iter()
                .map(PaneRuntimeScope::summary)
                .collect::<Vec<_>>(),
            vec![
                "remote_shell(haswell)".to_string(),
                "multiplexer(work)".to_string(),
                "shell".to_string(),
            ]
        );
        assert_eq!(
            runtime
                .last_verified_scope_stack
                .iter()
                .map(PaneRuntimeScope::summary)
                .collect::<Vec<_>>(),
            vec![
                "remote_shell(haswell)".to_string(),
                "multiplexer(work)".to_string(),
                "shell".to_string(),
            ]
        );
        assert_eq!(
            crate::control::format_target_stack(&control.target_stack),
            "remote_shell(haswell) -> tmux_session(work) -> shell_prompt(haswell)"
        );
        assert!(direct_terminal_exec_is_safe(&runtime));
    }

    #[test]
    fn alt_screen_creates_strong_interactive_scope() {
        let observation = PaneObservationFrame {
            title: Some("nvim test.sh".to_string()),
            cwd: Some("/tmp".to_string()),
            recent_output: vec!["".to_string()],
            screen_hints: Vec::new(),
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
        assert_eq!(runtime.front_state, PaneFrontState::InteractiveSurface);
        assert_eq!(runtime.mode, PaneMode::Tui);
        assert_eq!(
            runtime.active_scope,
            Some(PaneRuntimeScope {
                kind: PaneScopeKind::InteractiveApp,
                label: None,
                host: None,
                confidence: PaneConfidence::Strong,
                evidence_source: PaneEvidenceSource::SurfaceState,
            })
        );
    }

    #[test]
    fn system_prompt_marks_unknown_host_as_unknown_not_local() {
        let runtime = PaneRuntimeState {
            front_state: PaneFrontState::Unknown,
            mode: PaneMode::Shell,
            shell_metadata_fresh: false,
            remote_host: None,
            remote_host_confidence: None,
            remote_host_source: None,
            agent_cli: None,
            tmux_session: None,
            last_verified_scope_stack: Vec::new(),
            last_verified_tmux_session: None,
            shell_context: None,
            shell_context_fresh: false,
            active_scope: None,
            evidence: Vec::new(),
            scope_stack: Vec::new(),
            recent_actions: Vec::new(),
            warnings: Vec::new(),
        };
        let prompt = TerminalContext {
            focused_pane_index: 1,
            focused_hostname: None,
            focused_hostname_confidence: None,
            focused_hostname_source: None,
            focused_title: Some("Terminal".to_string()),
            focused_front_state: PaneFrontState::Unknown,
            focused_pane_mode: PaneMode::Shell,
            focused_has_shell_integration: false,
            focused_shell_metadata_fresh: false,
            focused_observation_support: PaneObservationSupport::default(),
            focused_runtime_stack: Vec::new(),
            focused_last_verified_runtime_stack: Vec::new(),
            focused_runtime_warnings: Vec::new(),
            focused_control: PaneControlState::from_runtime(&runtime),
            focused_shell_context: None,
            focused_shell_context_fresh: false,
            focused_recent_actions: Vec::new(),
            cwd: Some("/tmp".to_string()),
            recent_output: Vec::new(),
            focused_screen_hints: Vec::new(),
            focused_tmux_snapshot: None,
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
            front_state: PaneFrontState::ShellPrompt,
            mode: PaneMode::Shell,
            shell_metadata_fresh: true,
            remote_host: None,
            remote_host_confidence: None,
            remote_host_source: None,
            agent_cli: None,
            tmux_session: None,
            last_verified_scope_stack: Vec::new(),
            last_verified_tmux_session: None,
            shell_context: None,
            shell_context_fresh: false,
            active_scope: None,
            evidence: Vec::new(),
            scope_stack: Vec::new(),
            recent_actions: Vec::new(),
            warnings: Vec::new(),
        };
        let tmux_runtime = PaneRuntimeState {
            front_state: PaneFrontState::Unknown,
            mode: PaneMode::Unknown,
            shell_metadata_fresh: false,
            remote_host: None,
            remote_host_confidence: None,
            remote_host_source: None,
            agent_cli: None,
            tmux_session: None,
            last_verified_scope_stack: vec![PaneRuntimeScope {
                kind: PaneScopeKind::Multiplexer,
                label: Some("work".to_string()),
                host: None,
                confidence: PaneConfidence::Advisory,
                evidence_source: PaneEvidenceSource::ShellProbe,
            }],
            last_verified_tmux_session: Some("work".to_string()),
            shell_context: None,
            shell_context_fresh: false,
            active_scope: None,
            evidence: Vec::new(),
            scope_stack: Vec::new(),
            recent_actions: Vec::new(),
            warnings: Vec::new(),
        };

        assert!(direct_terminal_exec_is_safe(&shell_runtime));
        assert!(!direct_terminal_exec_is_safe(&tmux_runtime));
    }

    fn make_shell_context() -> TerminalContext {
        let runtime = PaneRuntimeState {
            front_state: PaneFrontState::ShellPrompt,
            mode: PaneMode::Shell,
            shell_metadata_fresh: true,
            remote_host: None,
            remote_host_confidence: None,
            remote_host_source: None,
            agent_cli: None,
            tmux_session: None,
            last_verified_scope_stack: Vec::new(),
            last_verified_tmux_session: None,
            shell_context: None,
            shell_context_fresh: false,
            active_scope: None,
            evidence: Vec::new(),
            scope_stack: Vec::new(),
            recent_actions: Vec::new(),
            warnings: Vec::new(),
        };
        TerminalContext {
            focused_pane_index: 1,
            focused_hostname: None,
            focused_hostname_confidence: None,
            focused_hostname_source: None,
            focused_title: None,
            focused_front_state: PaneFrontState::ShellPrompt,
            focused_pane_mode: PaneMode::Shell,
            focused_has_shell_integration: true,
            focused_shell_metadata_fresh: true,
            focused_observation_support: PaneObservationSupport::default(),
            focused_runtime_stack: Vec::new(),
            focused_last_verified_runtime_stack: Vec::new(),
            focused_runtime_warnings: Vec::new(),
            focused_control: PaneControlState::from_runtime(&runtime),
            focused_shell_context: None,
            focused_shell_context_fresh: false,
            focused_recent_actions: Vec::new(),
            cwd: Some("/home/user".to_string()),
            recent_output: Vec::new(),
            focused_screen_hints: Vec::new(),
            focused_tmux_snapshot: None,
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
            front_state: PaneFrontState::Unknown,
            mode: PaneMode::Unknown,
            shell_metadata_fresh: false,
            remote_host: None,
            remote_host_confidence: None,
            remote_host_source: None,
            agent_cli: None,
            tmux_session: None,
            last_verified_scope_stack: vec![
                PaneRuntimeScope {
                    kind: PaneScopeKind::RemoteShell,
                    label: Some("haswell".to_string()),
                    host: Some("haswell".to_string()),
                    confidence: PaneConfidence::Advisory,
                    evidence_source: PaneEvidenceSource::ShellProbe,
                },
                PaneRuntimeScope {
                    kind: PaneScopeKind::Multiplexer,
                    label: Some("work".to_string()),
                    host: Some("haswell".to_string()),
                    confidence: PaneConfidence::Advisory,
                    evidence_source: PaneEvidenceSource::ShellProbe,
                },
                PaneRuntimeScope {
                    kind: PaneScopeKind::Shell,
                    label: None,
                    host: Some("haswell".to_string()),
                    confidence: PaneConfidence::Advisory,
                    evidence_source: PaneEvidenceSource::ShellProbe,
                },
            ],
            last_verified_tmux_session: Some("work".to_string()),
            shell_context: None,
            shell_context_fresh: false,
            active_scope: None,
            evidence: Vec::new(),
            scope_stack: Vec::new(),
            recent_actions: Vec::new(),
            warnings: Vec::new(),
        };
        ctx.focused_front_state = PaneFrontState::Unknown;
        ctx.focused_pane_mode = PaneMode::Unknown;
        ctx.focused_shell_metadata_fresh = false;
        ctx.focused_last_verified_runtime_stack = runtime.last_verified_scope_stack.clone();
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
            prompt.contains("prefer tmux-native")
                || prompt.contains("Prefer tmux-native")
                || prompt.contains("tmux native control"),
            "Prompt should prefer tmux-native tools when tmux is involved"
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

    #[test]
    fn derive_screen_hints_marks_visible_prompt_and_htop_as_observations() {
        let hints = derive_screen_hints(&[
            "Tasks: 105, 738 thr, 692 kthr; 1 running".to_string(),
            "Load average: 0.08 0.09 0.06".to_string(),
            "  PID USER      PRI  NI  VIRT   RES   SHR S CPU% MEM%   TIME+  Command".to_string(),
            ") htop".to_string(),
        ]);

        assert!(
            hints
                .iter()
                .any(|hint| hint.kind == PaneObservationHintKind::HtopLikeScreen)
        );
        assert!(
            hints
                .iter()
                .any(|hint| hint.kind == PaneObservationHintKind::PromptLikeInput)
        );
        assert!(
            hints
                .iter()
                .all(|hint| hint.confidence == PaneConfidence::Advisory)
        );
    }

    #[test]
    fn prompt_emits_tmux_snapshot_when_attached() {
        let mut ctx = make_shell_context();
        ctx.focused_tmux_snapshot = Some(TmuxSnapshot {
            panes: vec![TmuxPaneInfo {
                session_name: "work".to_string(),
                window_id: "@4".to_string(),
                window_name: "shell".to_string(),
                pane_id: "%17".to_string(),
                pane_index: "0".to_string(),
                pane_active: true,
                pane_current_command: Some("zsh".to_string()),
                pane_current_path: Some("/home/w/repo".to_string()),
            }],
        });
        let prompt = ctx.to_system_prompt();
        assert!(prompt.contains("<tmux_snapshot>"));
        assert!(prompt.contains("pane_id=\"%17\""));
        assert!(prompt.contains("current_command=\"zsh\""));
    }
}
