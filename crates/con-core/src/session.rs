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
            }],
            active_tab: 0,
            agent_panel_open: false,
            agent_panel_width: None,
            input_bar_visible: true,
            global_shell_history: Vec::new(),
            input_history: Vec::new(),
            conversation_id: None,
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

    fn session_path() -> PathBuf {
        if let Some(path) = std::env::var_os("CON_SESSION_PATH") {
            return PathBuf::from(path);
        }
        dirs::data_dir()
            .or_else(|| dirs::home_dir().map(|h| h.join(".local/share")))
            .unwrap_or_else(|| PathBuf::from("/tmp"))
            .join("con")
            .join("session.json")
    }
}
