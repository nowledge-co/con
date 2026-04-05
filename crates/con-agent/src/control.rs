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
    TmuxSession,
    AgentCli,
    InteractiveApp,
    UnknownTui,
}

impl PaneVisibleTargetKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ShellPrompt => "shell_prompt",
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
            (_, Some(label), _) => format!("{}({label})", self.kind.as_str()),
            _ => self.kind.as_str().to_string(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PaneControlState {
    pub address_space: PaneAddressSpace,
    pub visible_target: PaneVisibleTarget,
    pub channels: Vec<PaneControlChannel>,
    pub capabilities: Vec<PaneControlCapability>,
    pub notes: Vec<String>,
}

impl PaneControlState {
    pub fn from_runtime(runtime: &PaneRuntimeState) -> Self {
        let visible_target = visible_target_from_runtime(runtime);
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
            }
            PaneVisibleTargetKind::InteractiveApp | PaneVisibleTargetKind::UnknownTui => notes.push(
                "This pane shows an interactive app. con can inspect screen state and send raw input, but it does not have app-native control for this target yet."
                    .to_string(),
            ),
            PaneVisibleTargetKind::ShellPrompt => {}
        }

        Self {
            address_space: PaneAddressSpace::ConPane,
            visible_target,
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

fn visible_target_from_runtime(runtime: &PaneRuntimeState) -> PaneVisibleTarget {
    if let Some(agent_cli) = &runtime.agent_cli {
        return PaneVisibleTarget {
            kind: PaneVisibleTargetKind::AgentCli,
            label: Some(agent_cli.clone()),
            host: runtime.remote_host.clone(),
        };
    }

    if runtime.mode == PaneMode::Multiplexer {
        return PaneVisibleTarget {
            kind: PaneVisibleTargetKind::TmuxSession,
            label: runtime.tmux_session.clone().or_else(|| {
                runtime
                    .active_scope
                    .as_ref()
                    .and_then(|scope| scope.label.clone())
                    .or_else(|| Some("tmux".to_string()))
            }),
            host: runtime.remote_host.clone(),
        };
    }

    if runtime.mode == PaneMode::Shell {
        return PaneVisibleTarget {
            kind: PaneVisibleTargetKind::ShellPrompt,
            label: runtime
                .active_scope
                .as_ref()
                .and_then(|scope| scope.label.clone()),
            host: runtime.remote_host.clone(),
        };
    }

    if runtime
        .active_scope
        .as_ref()
        .is_some_and(|scope| scope.kind == PaneScopeKind::InteractiveApp)
    {
        return PaneVisibleTarget {
            kind: PaneVisibleTargetKind::InteractiveApp,
            label: runtime
                .active_scope
                .as_ref()
                .and_then(|scope| scope.label.clone()),
            host: runtime.remote_host.clone(),
        };
    }

    PaneVisibleTarget {
        kind: PaneVisibleTargetKind::UnknownTui,
        label: runtime
            .active_scope
            .as_ref()
            .and_then(|scope| scope.label.clone()),
        host: runtime.remote_host.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        PaneControlCapability, PaneControlState, PaneVisibleTargetKind, visible_target_from_runtime,
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
            visible_target_from_runtime(&runtime).kind,
            PaneVisibleTargetKind::ShellPrompt
        );
        assert!(control.allows_visible_shell_exec());
    }
}
