use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Session state for persistence across restarts
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    pub tabs: Vec<TabState>,
    pub active_tab: usize,
    pub agent_panel_open: bool,
    #[serde(default)]
    pub conversation_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TabState {
    pub title: String,
    pub cwd: Option<String>,
    pub panes: Vec<PaneState>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaneState {
    pub cwd: Option<String>,
    pub scrollback_lines: usize,
}

impl Default for Session {
    fn default() -> Self {
        Self {
            tabs: vec![TabState {
                title: "Terminal".to_string(),
                cwd: None,
                panes: vec![PaneState {
                    cwd: None,
                    scrollback_lines: 0,
                }],
            }],
            active_tab: 0,
            agent_panel_open: false,
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
        dirs::data_dir()
            .or_else(|| dirs::home_dir().map(|h| h.join(".local/share")))
            .unwrap_or_else(|| PathBuf::from("/tmp"))
            .join("con")
            .join("session.json")
    }
}
