use serde::{Deserialize, Serialize};

use crate::context::{PaneMode, PaneRuntimeState, PaneScopeKind};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PaneAddressSpace {
    ConPane,
}

impl PaneAddressSpace {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ConPane => "con_pane",
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PaneVisibleTargetKind {
    ShellPrompt,
    RemoteShell,
    TmuxSession,
    AgentCli,
    InteractiveApp,
    UnknownTui,
}

impl PaneVisibleTargetKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ShellPrompt => "shell_prompt",
            Self::RemoteShell => "remote_shell",
            Self::TmuxSession => "tmux_session",
            Self::AgentCli => "agent_cli",
            Self::InteractiveApp => "interactive_app",
            Self::UnknownTui => "unknown_tui",
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PaneControlChannel {
    ReadScreen,
    SearchScrollback,
    RawInput,
    VisibleShellExec,
}

impl PaneControlChannel {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ReadScreen => "read_screen",
            Self::SearchScrollback => "search_scrollback",
            Self::RawInput => "raw_input",
            Self::VisibleShellExec => "visible_shell_exec",
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PaneControlCapability {
    ReadScreen,
    SearchScrollback,
    SendRawInput,
    ExecVisibleShell,
}

impl PaneControlCapability {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ReadScreen => "read_screen",
            Self::SearchScrollback => "search_scrollback",
            Self::SendRawInput => "send_raw_input",
            Self::ExecVisibleShell => "exec_visible_shell",
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TmuxControlMode {
    Unavailable,
    InspectOnly,
    Native,
}

impl TmuxControlMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Unavailable => "unavailable",
            Self::InspectOnly => "inspect_only",
            Self::Native => "native",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PaneVisibleTarget {
    pub kind: PaneVisibleTargetKind,
    pub label: Option<String>,
    pub host: Option<String>,
}

impl PaneVisibleTarget {
    pub fn summary(&self) -> String {
        match (self.kind, self.label.as_deref(), self.host.as_deref()) {
            (PaneVisibleTargetKind::ShellPrompt, _, Some(host)) => {
                format!("shell_prompt({host})")
            }
            (PaneVisibleTargetKind::RemoteShell, _, Some(host)) => {
                format!("remote_shell({host})")
            }
            (_, Some(label), _) => format!("{}({label})", self.kind.as_str()),
            _ => self.kind.as_str().to_string(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PaneControlState {
    pub address_space: PaneAddressSpace,
    pub target_stack: Vec<PaneVisibleTarget>,
    pub visible_target: PaneVisibleTarget,
    pub tmux: Option<TmuxControlState>,
    pub channels: Vec<PaneControlChannel>,
    pub capabilities: Vec<PaneControlCapability>,
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TmuxControlState {
    pub session_name: Option<String>,
    pub mode: TmuxControlMode,
    pub front_target: Option<PaneVisibleTarget>,
    pub reason: String,
}

impl PaneControlState {
    pub fn from_runtime(runtime: &PaneRuntimeState) -> Self {
        let target_stack = target_stack_from_runtime(runtime);
        let visible_target = target_stack
            .last()
            .cloned()
            .unwrap_or_else(|| fallback_visible_target(runtime));
        let tmux = tmux_control_from_runtime(runtime, &target_stack);

        let mut channels = vec![
            PaneControlChannel::ReadScreen,
            PaneControlChannel::SearchScrollback,
            PaneControlChannel::RawInput,
        ];
        let mut capabilities = vec![
            PaneControlCapability::ReadScreen,
            PaneControlCapability::SearchScrollback,
            PaneControlCapability::SendRawInput,
        ];
        if runtime.mode == PaneMode::Shell && runtime.shell_metadata_fresh {
            channels.push(PaneControlChannel::VisibleShellExec);
            capabilities.push(PaneControlCapability::ExecVisibleShell);
        }

        let mut notes = Vec::new();
        if !runtime.shell_metadata_fresh {
            notes.push(
                "Shell-derived cwd and command metadata may describe an earlier shell frame, not the current visible target.".to_string(),
            );
        }
        if let Some(host) = &runtime.remote_host {
            if visible_target.kind == PaneVisibleTargetKind::ShellPrompt {
                notes.push(format!(
                    "Visible shell execution in this pane will run on host `{host}`."
                ));
            }
        }
        if target_stack.len() > 1 {
            notes.push(format!(
                "Nested visible target stack: {}.",
                format_target_stack(&target_stack)
            ));
        }

        match visible_target.kind {
            PaneVisibleTargetKind::TmuxSession => notes.push(
                "This pane shows tmux. `pane_index` addresses the outer con pane only, not a tmux window or tmux pane."
                    .to_string(),
            ),
            PaneVisibleTargetKind::AgentCli => {
                let label = visible_target.label.as_deref().unwrap_or("agent_cli");
                notes.push(format!(
                    "The visible target is `{label}`. Raw input affects that agent UI, not a shell prompt."
                ));
                if target_stack
                    .iter()
                    .any(|target| target.kind == PaneVisibleTargetKind::TmuxSession)
                {
                    notes.push(
                        "That agent CLI is nested inside tmux. con still does not have tmux-native pane targeting here."
                            .to_string(),
                    );
                }
            }
            PaneVisibleTargetKind::InteractiveApp | PaneVisibleTargetKind::UnknownTui => notes.push(
                "This pane shows an interactive app. con can inspect screen state and send raw input, but it does not have app-native control for this target yet."
                    .to_string(),
            ),
            PaneVisibleTargetKind::ShellPrompt | PaneVisibleTargetKind::RemoteShell => {}
        }

        Self {
            address_space: PaneAddressSpace::ConPane,
            target_stack,
            visible_target,
            tmux,
            channels,
            capabilities,
            notes,
        }
    }

    pub fn allows_visible_shell_exec(&self) -> bool {
        self.capabilities
            .contains(&PaneControlCapability::ExecVisibleShell)
    }
}

pub fn format_target_stack(targets: &[PaneVisibleTarget]) -> String {
    if targets.is_empty() {
        "unknown".to_string()
    } else {
        targets
            .iter()
            .map(PaneVisibleTarget::summary)
            .collect::<Vec<_>>()
            .join(" -> ")
    }
}

fn tmux_control_from_runtime(
    runtime: &PaneRuntimeState,
    target_stack: &[PaneVisibleTarget],
) -> Option<TmuxControlState> {
    if !target_stack
        .iter()
        .any(|target| target.kind == PaneVisibleTargetKind::TmuxSession)
    {
        return None;
    }

    let tmux_index = target_stack
        .iter()
        .position(|target| target.kind == PaneVisibleTargetKind::TmuxSession)?;

    Some(TmuxControlState {
        session_name: runtime.tmux_session.clone().or_else(|| {
            target_stack
                .get(tmux_index)
                .and_then(|target| target.label.clone())
        }),
        mode: TmuxControlMode::InspectOnly,
        front_target: target_stack.get(tmux_index + 1).cloned(),
        reason: "con can identify the tmux scope in this pane, but it does not yet have a same-session tmux control channel. Native tmux pane/window targeting and tmux command execution are unavailable.".to_string(),
    })
}

fn target_stack_from_runtime(runtime: &PaneRuntimeState) -> Vec<PaneVisibleTarget> {
    let mut stack = Vec::new();

    for scope in &runtime.scope_stack {
        let target = match scope.kind {
            PaneScopeKind::Shell => PaneVisibleTarget {
                kind: PaneVisibleTargetKind::ShellPrompt,
                label: scope.label.clone(),
                host: runtime.remote_host.clone(),
            },
            PaneScopeKind::RemoteShell => PaneVisibleTarget {
                kind: PaneVisibleTargetKind::RemoteShell,
                label: scope.label.clone(),
                host: scope.host.clone().or_else(|| runtime.remote_host.clone()),
            },
            PaneScopeKind::Multiplexer => PaneVisibleTarget {
                kind: PaneVisibleTargetKind::TmuxSession,
                label: scope
                    .label
                    .clone()
                    .or_else(|| runtime.tmux_session.clone())
                    .or_else(|| Some("tmux".to_string())),
                host: runtime.remote_host.clone(),
            },
            PaneScopeKind::AgentCli => PaneVisibleTarget {
                kind: PaneVisibleTargetKind::AgentCli,
                label: scope.label.clone().or_else(|| runtime.agent_cli.clone()),
                host: runtime.remote_host.clone(),
            },
            PaneScopeKind::InteractiveApp => PaneVisibleTarget {
                kind: PaneVisibleTargetKind::InteractiveApp,
                label: scope.label.clone(),
                host: runtime.remote_host.clone(),
            },
        };

        if stack.last() != Some(&target) {
            stack.push(target);
        }
    }

    if stack.is_empty() {
        stack.push(fallback_visible_target(runtime));
    }

    stack
}

fn fallback_visible_target(runtime: &PaneRuntimeState) -> PaneVisibleTarget {
    match runtime.mode {
        PaneMode::Shell => PaneVisibleTarget {
            kind: PaneVisibleTargetKind::ShellPrompt,
            label: runtime
                .active_scope
                .as_ref()
                .and_then(|scope| scope.label.clone()),
            host: runtime.remote_host.clone(),
        },
        PaneMode::Multiplexer => PaneVisibleTarget {
            kind: PaneVisibleTargetKind::TmuxSession,
            label: runtime.tmux_session.clone().or_else(|| {
                runtime
                    .active_scope
                    .as_ref()
                    .and_then(|scope| scope.label.clone())
                    .or_else(|| Some("tmux".to_string()))
            }),
            host: runtime.remote_host.clone(),
        },
        PaneMode::Tui => {
            if let Some(agent_cli) = &runtime.agent_cli {
                PaneVisibleTarget {
                    kind: PaneVisibleTargetKind::AgentCli,
                    label: Some(agent_cli.clone()),
                    host: runtime.remote_host.clone(),
                }
            } else if runtime
                .active_scope
                .as_ref()
                .is_some_and(|scope| scope.kind == PaneScopeKind::InteractiveApp)
            {
                PaneVisibleTarget {
                    kind: PaneVisibleTargetKind::InteractiveApp,
                    label: runtime
                        .active_scope
                        .as_ref()
                        .and_then(|scope| scope.label.clone()),
                    host: runtime.remote_host.clone(),
                }
            } else {
                PaneVisibleTarget {
                    kind: PaneVisibleTargetKind::UnknownTui,
                    label: runtime
                        .active_scope
                        .as_ref()
                        .and_then(|scope| scope.label.clone()),
                    host: runtime.remote_host.clone(),
                }
            }
        }
        PaneMode::Unknown => PaneVisibleTarget {
            kind: PaneVisibleTargetKind::UnknownTui,
            label: runtime
                .active_scope
                .as_ref()
                .and_then(|scope| scope.label.clone()),
            host: runtime.remote_host.clone(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::{
        PaneControlCapability, PaneControlState, PaneVisibleTargetKind, TmuxControlMode,
        fallback_visible_target, format_target_stack,
    };
    use crate::context::{
        PaneConfidence, PaneEvidenceSource, PaneMode, PaneRuntimeScope, PaneRuntimeState,
        PaneScopeKind,
    };

    #[test]
    fn control_state_marks_tmux_as_raw_input_only() {
        let runtime = PaneRuntimeState {
            mode: PaneMode::Multiplexer,
            shell_metadata_fresh: false,
            remote_host: Some("haswell".to_string()),
            remote_host_confidence: Some(PaneConfidence::Advisory),
            remote_host_source: Some(PaneEvidenceSource::ScreenStructure),
            agent_cli: None,
            tmux_session: Some("work".to_string()),
            active_scope: Some(PaneRuntimeScope {
                kind: PaneScopeKind::Multiplexer,
                label: Some("work".to_string()),
                host: Some("haswell".to_string()),
                confidence: PaneConfidence::Advisory,
                evidence_source: PaneEvidenceSource::ScreenStructure,
            }),
            evidence: Vec::new(),
            scope_stack: Vec::new(),
            warnings: Vec::new(),
        };

        let control = PaneControlState::from_runtime(&runtime);
        assert_eq!(
            control.visible_target.kind,
            PaneVisibleTargetKind::TmuxSession
        );
        assert!(!control.allows_visible_shell_exec());
        assert!(
            !control
                .capabilities
                .contains(&PaneControlCapability::ExecVisibleShell)
        );
    }

    #[test]
    fn control_state_marks_fresh_shell_as_visible_exec_capable() {
        let runtime = PaneRuntimeState {
            mode: PaneMode::Shell,
            shell_metadata_fresh: true,
            remote_host: Some("prod".to_string()),
            remote_host_confidence: Some(PaneConfidence::Strong),
            remote_host_source: Some(PaneEvidenceSource::Osc7),
            agent_cli: None,
            tmux_session: None,
            active_scope: Some(PaneRuntimeScope {
                kind: PaneScopeKind::RemoteShell,
                label: None,
                host: Some("prod".to_string()),
                confidence: PaneConfidence::Strong,
                evidence_source: PaneEvidenceSource::Osc7,
            }),
            evidence: Vec::new(),
            scope_stack: Vec::new(),
            warnings: Vec::new(),
        };

        let control = PaneControlState::from_runtime(&runtime);
        assert_eq!(
            fallback_visible_target(&runtime).kind,
            PaneVisibleTargetKind::ShellPrompt
        );
        assert!(control.allows_visible_shell_exec());
    }

    #[test]
    fn control_state_preserves_nested_tmux_and_agent_cli_stack() {
        let runtime = PaneRuntimeState {
            mode: PaneMode::Multiplexer,
            shell_metadata_fresh: false,
            remote_host: Some("haswell".to_string()),
            remote_host_confidence: Some(PaneConfidence::Strong),
            remote_host_source: Some(PaneEvidenceSource::Osc7),
            agent_cli: Some("codex".to_string()),
            tmux_session: Some("work".to_string()),
            active_scope: Some(PaneRuntimeScope {
                kind: PaneScopeKind::AgentCli,
                label: Some("codex".to_string()),
                host: None,
                confidence: PaneConfidence::Advisory,
                evidence_source: PaneEvidenceSource::ScreenStructure,
            }),
            evidence: Vec::new(),
            scope_stack: vec![
                PaneRuntimeScope {
                    kind: PaneScopeKind::Shell,
                    label: None,
                    host: None,
                    confidence: PaneConfidence::Strong,
                    evidence_source: PaneEvidenceSource::ShellIntegration,
                },
                PaneRuntimeScope {
                    kind: PaneScopeKind::RemoteShell,
                    label: None,
                    host: Some("haswell".to_string()),
                    confidence: PaneConfidence::Strong,
                    evidence_source: PaneEvidenceSource::Osc7,
                },
                PaneRuntimeScope {
                    kind: PaneScopeKind::Multiplexer,
                    label: Some("work".to_string()),
                    host: None,
                    confidence: PaneConfidence::Strong,
                    evidence_source: PaneEvidenceSource::CommandLine,
                },
                PaneRuntimeScope {
                    kind: PaneScopeKind::AgentCli,
                    label: Some("codex".to_string()),
                    host: None,
                    confidence: PaneConfidence::Advisory,
                    evidence_source: PaneEvidenceSource::ScreenStructure,
                },
            ],
            warnings: Vec::new(),
        };

        let control = PaneControlState::from_runtime(&runtime);
        assert_eq!(control.visible_target.kind, PaneVisibleTargetKind::AgentCli);
        assert_eq!(
            format_target_stack(&control.target_stack),
            "shell_prompt(haswell) -> remote_shell(haswell) -> tmux_session(work) -> agent_cli(codex)"
        );
        assert_eq!(
            control.tmux.as_ref().map(|tmux| tmux.mode),
            Some(TmuxControlMode::InspectOnly)
        );
        assert_eq!(
            control
                .tmux
                .as_ref()
                .and_then(|tmux| tmux.front_target.as_ref())
                .map(|target| target.summary()),
            Some("agent_cli(codex)".to_string())
        );
        assert!(
            control
                .notes
                .iter()
                .any(|note| note.contains("nested inside tmux"))
        );
    }
}
