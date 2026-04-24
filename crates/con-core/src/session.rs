use con_agent::ProviderKind;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Session state for persistence across restarts
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    pub tabs: Vec<TabState>,
    pub active_tab: usize,
    pub agent_panel_open: bool,
    #[serde(default)]
    pub agent_panel_width: Option<f32>,
    #[serde(default = "default_input_bar_visible")]
    pub input_bar_visible: bool,
    #[serde(default)]
    pub global_shell_history: Vec<CommandHistoryEntryState>,
    #[serde(default)]
    pub input_history: Vec<String>,
    #[serde(default)]
    pub conversation_id: Option<String>,
    /// Whether the vertical-tabs side panel is pinned open (full panel)
    /// or collapsed to its icon rail. Only consulted when the user has
    /// `appearance.tabs_orientation = vertical` in config; in horizontal
    /// mode this field is preserved verbatim across restarts so toggling
    /// orientation later restores the previous expansion state.
    #[serde(default)]
    pub vertical_tabs_pinned: bool,
}

/// App-wide command history, stored separately from window layout so it survives
/// fresh-window starts and recoverable session-layout load failures.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct GlobalHistoryState {
    #[serde(default)]
    pub global_shell_history: Vec<CommandHistoryEntryState>,
    #[serde(default)]
    pub input_history: Vec<String>,
}

fn default_input_bar_visible() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TabState {
    pub title: String,
    pub cwd: Option<String>,
    #[serde(default)]
    pub layout: Option<PaneLayoutState>,
    #[serde(default)]
    pub focused_pane_id: Option<usize>,
    pub panes: Vec<PaneState>,
    #[serde(default)]
    pub shell_history: Vec<PaneCommandHistoryState>,
    /// Per-tab conversation ID (persisted across restarts)
    #[serde(default)]
    pub conversation_id: Option<String>,
    /// Per-tab agent routing overrides. Global settings still provide credentials
    /// and shared behavior; tabs only persist the selected provider and model choices.
    #[serde(default)]
    pub agent_routing: AgentRoutingState,
    /// Optional user-supplied label that overrides every auto-derived name
    /// (focused-process, cwd basename, shell name) shown in the vertical
    /// tabs panel and horizontal tab strip. Set via the inline rename
    /// affordance (double-click a row in the vertical panel) or the
    /// context menu's "Rename" entry.
    #[serde(default)]
    pub user_label: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct AgentRoutingState {
    pub provider: Option<ProviderKind>,
    pub model_overrides: Vec<AgentModelOverrideState>,
}

impl AgentRoutingState {
    pub fn is_empty(&self) -> bool {
        self.provider.is_none() && self.model_overrides.is_empty()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentModelOverrideState {
    pub provider: ProviderKind,
    pub model: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PaneLayoutState {
    Leaf {
        pane_id: usize,
        cwd: Option<String>,
    },
    Split {
        direction: PaneSplitDirection,
        ratio: f32,
        first: Box<PaneLayoutState>,
        second: Box<PaneLayoutState>,
    },
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PaneSplitDirection {
    Horizontal,
    Vertical,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaneState {
    pub cwd: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaneCommandHistoryState {
    #[serde(default)]
    pub pane_id: Option<usize>,
    #[serde(default)]
    pub entries: Vec<CommandHistoryEntryState>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommandHistoryEntryState {
    pub command: String,
    #[serde(default)]
    pub cwd: Option<String>,
}

impl Default for Session {
    fn default() -> Self {
        Self {
            tabs: vec![TabState {
                title: "Terminal".to_string(),
                cwd: None,
                layout: None,
                focused_pane_id: Some(0),
                panes: vec![PaneState { cwd: None }],
                shell_history: Vec::new(),
                conversation_id: None,
                agent_routing: AgentRoutingState::default(),
                user_label: None,
            }],
            active_tab: 0,
            agent_panel_open: false,
            agent_panel_width: None,
            input_bar_visible: true,
            global_shell_history: Vec::new(),
            input_history: Vec::new(),
            conversation_id: None,
            vertical_tabs_pinned: false,
        }
    }
}

impl Session {
    pub fn save(&self) -> anyhow::Result<()> {
        let path = Self::session_path();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let content = serde_json::to_string_pretty(self)?;
        std::fs::write(&path, content)?;
        Ok(())
    }

    pub fn load() -> anyhow::Result<Self> {
        let path = Self::session_path();
        if path.exists() {
            let content = std::fs::read_to_string(&path)?;
            let session: Session = serde_json::from_str(&content)?;
            Ok(session)
        } else {
            Ok(Session::default())
        }
    }

    pub fn session_path() -> PathBuf {
        if let Some(path) = std::env::var_os("CON_SESSION_PATH") {
            return PathBuf::from(path);
        }
        con_paths::app_data_dir().join("session.json")
    }
}

impl GlobalHistoryState {
    pub fn save(&self) -> anyhow::Result<()> {
        let path = Self::history_path();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let content = serde_json::to_string_pretty(self)?;
        std::fs::write(&path, content)?;
        Ok(())
    }

    pub fn load() -> anyhow::Result<Self> {
        let path = Self::history_path();
        if path.exists() {
            let content = std::fs::read_to_string(&path)?;
            let history: GlobalHistoryState = serde_json::from_str(&content)?;
            Ok(history)
        } else {
            Ok(GlobalHistoryState::default())
        }
    }

    pub fn history_path() -> PathBuf {
        if let Some(path) = std::env::var_os("CON_HISTORY_PATH") {
            return PathBuf::from(path);
        }
        con_paths::app_data_dir().join("history.json")
    }
}
