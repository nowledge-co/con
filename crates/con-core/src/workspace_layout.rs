use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub const WORKSPACE_LAYOUT_VERSION: u32 = 1;
pub const DEFAULT_WORKSPACE_LAYOUT_PATH: &str = ".con/workspace.toml";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WorkspaceLayout {
    #[serde(default = "default_version")]
    pub version: u32,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default = "default_root")]
    pub root: String,
    #[serde(default)]
    pub defaults: WorkspaceDefaults,
    #[serde(default)]
    pub tabs: Vec<WorkspaceTab>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct WorkspaceDefaults {
    #[serde(default)]
    pub shell: Option<String>,
    #[serde(default)]
    pub agent_provider: Option<String>,
    #[serde(default)]
    pub agent_model: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WorkspaceTab {
    pub id: String,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub cwd: Option<String>,
    #[serde(default)]
    pub active_pane: Option<String>,
    #[serde(default)]
    pub agent: WorkspaceTabAgent,
    #[serde(default)]
    pub layout: Option<WorkspaceLayoutNode>,
    #[serde(default)]
    pub panes: Vec<WorkspacePane>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct WorkspaceTabAgent {
    #[serde(default)]
    pub provider: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum WorkspaceLayoutNode {
    Pane {
        id: String,
    },
    Split {
        direction: WorkspaceSplitDirection,
        ratio: f32,
        first: Box<WorkspaceLayoutNode>,
        second: Box<WorkspaceLayoutNode>,
    },
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum WorkspaceSplitDirection {
    Horizontal,
    Vertical,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WorkspacePane {
    pub id: String,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub cwd: Option<String>,
    #[serde(default)]
    pub active_surface: Option<String>,
    #[serde(default)]
    pub surfaces: Vec<WorkspaceSurface>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WorkspaceSurface {
    pub id: String,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub owner: Option<String>,
    #[serde(default)]
    pub cwd: Option<String>,
    #[serde(default)]
    pub run: Option<String>,
    #[serde(default)]
    pub restore: RestorePolicy,
    #[serde(default)]
    pub close_pane_when_last: bool,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RestorePolicy {
    #[default]
    Manual,
    Ask,
    Auto,
}

fn default_version() -> u32 {
    WORKSPACE_LAYOUT_VERSION
}

fn default_root() -> String {
    ".".to_string()
}

impl Default for WorkspaceLayout {
    fn default() -> Self {
        Self {
            version: WORKSPACE_LAYOUT_VERSION,
            name: None,
            root: default_root(),
            defaults: WorkspaceDefaults::default(),
            tabs: Vec::new(),
        }
    }
}

impl WorkspaceLayout {
    pub fn from_toml_str(input: &str) -> anyhow::Result<Self> {
        let layout: Self = toml::from_str(input)?;
        layout.validate()?;
        Ok(layout)
    }

    pub fn to_toml_string(&self) -> anyhow::Result<String> {
        self.validate()?;
        Ok(toml::to_string_pretty(self)?)
    }

    pub fn load(path: impl AsRef<Path>) -> anyhow::Result<Self> {
        let content = std::fs::read_to_string(path)?;
        Self::from_toml_str(&content)
    }

    pub fn save(&self, path: impl AsRef<Path>) -> anyhow::Result<()> {
        let path = path.as_ref();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, self.to_toml_string()?)?;
        Ok(())
    }

    pub fn default_path_for_root(root: impl AsRef<Path>) -> PathBuf {
        root.as_ref().join(DEFAULT_WORKSPACE_LAYOUT_PATH)
    }

    pub fn validate(&self) -> anyhow::Result<()> {
        anyhow::ensure!(
            self.version == WORKSPACE_LAYOUT_VERSION,
            "unsupported workspace layout version {}",
            self.version
        );

        let mut tab_ids = std::collections::HashSet::new();
        for tab in &self.tabs {
            anyhow::ensure!(
                !tab.id.trim().is_empty(),
                "workspace tab id cannot be empty"
            );
            anyhow::ensure!(
                tab_ids.insert(tab.id.as_str()),
                "duplicate workspace tab id {:?}",
                tab.id
            );

            let mut pane_ids = std::collections::HashSet::new();
            for pane in &tab.panes {
                anyhow::ensure!(
                    !pane.id.trim().is_empty(),
                    "workspace pane id cannot be empty in tab {:?}",
                    tab.id
                );
                anyhow::ensure!(
                    pane_ids.insert(pane.id.as_str()),
                    "duplicate pane id {:?} in tab {:?}",
                    pane.id,
                    tab.id
                );

                let mut surface_ids = std::collections::HashSet::new();
                for surface in &pane.surfaces {
                    anyhow::ensure!(
                        !surface.id.trim().is_empty(),
                        "workspace surface id cannot be empty in pane {:?}",
                        pane.id
                    );
                    anyhow::ensure!(
                        surface_ids.insert(surface.id.as_str()),
                        "duplicate surface id {:?} in pane {:?}",
                        surface.id,
                        pane.id
                    );
                }

                if let Some(active_surface) = pane.active_surface.as_deref() {
                    anyhow::ensure!(
                        surface_ids.contains(active_surface),
                        "active surface {:?} is not defined in pane {:?}",
                        active_surface,
                        pane.id
                    );
                }
            }

            if let Some(active_pane) = tab.active_pane.as_deref() {
                anyhow::ensure!(
                    pane_ids.contains(active_pane),
                    "active pane {:?} is not defined in tab {:?}",
                    active_pane,
                    tab.id
                );
            }

            if let Some(layout) = tab.layout.as_ref() {
                validate_layout_node(layout, &pane_ids, &tab.id)?;
            }
        }

        Ok(())
    }
}

fn validate_layout_node(
    node: &WorkspaceLayoutNode,
    pane_ids: &std::collections::HashSet<&str>,
    tab_id: &str,
) -> anyhow::Result<()> {
    match node {
        WorkspaceLayoutNode::Pane { id } => {
            anyhow::ensure!(
                pane_ids.contains(id.as_str()),
                "layout references undefined pane {:?} in tab {:?}",
                id,
                tab_id
            );
        }
        WorkspaceLayoutNode::Split {
            ratio,
            first,
            second,
            ..
        } => {
            anyhow::ensure!(
                (0.05..=0.95).contains(ratio),
                "layout split ratio {} in tab {:?} is outside 0.05..=0.95",
                ratio,
                tab_id
            );
            validate_layout_node(first, pane_ids, tab_id)?;
            validate_layout_node(second, pane_ids, tab_id)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn workspace_layout_round_trips_as_toml() {
        let layout = WorkspaceLayout {
            version: 1,
            name: Some("con".to_string()),
            root: ".".to_string(),
            defaults: WorkspaceDefaults {
                shell: Some("login".to_string()),
                agent_provider: Some("openai".to_string()),
                agent_model: Some("gpt-5.2".to_string()),
            },
            tabs: vec![WorkspaceTab {
                id: "dev".to_string(),
                title: Some("Dev".to_string()),
                cwd: Some(".".to_string()),
                active_pane: Some("editor".to_string()),
                agent: WorkspaceTabAgent::default(),
                layout: Some(WorkspaceLayoutNode::Split {
                    direction: WorkspaceSplitDirection::Horizontal,
                    ratio: 0.6,
                    first: Box::new(WorkspaceLayoutNode::Pane {
                        id: "editor".to_string(),
                    }),
                    second: Box::new(WorkspaceLayoutNode::Pane {
                        id: "server".to_string(),
                    }),
                }),
                panes: vec![
                    WorkspacePane {
                        id: "editor".to_string(),
                        title: Some("Editor".to_string()),
                        cwd: Some(".".to_string()),
                        active_surface: Some("shell".to_string()),
                        surfaces: vec![WorkspaceSurface {
                            id: "shell".to_string(),
                            title: Some("Shell".to_string()),
                            owner: None,
                            cwd: Some(".".to_string()),
                            run: None,
                            restore: RestorePolicy::Manual,
                            close_pane_when_last: false,
                        }],
                    },
                    WorkspacePane {
                        id: "server".to_string(),
                        title: Some("Server".to_string()),
                        cwd: Some("crates/con-app".to_string()),
                        active_surface: Some("server".to_string()),
                        surfaces: vec![WorkspaceSurface {
                            id: "server".to_string(),
                            title: Some("Server".to_string()),
                            owner: None,
                            cwd: Some("crates/con-app".to_string()),
                            run: Some("cargo run -p con".to_string()),
                            restore: RestorePolicy::Ask,
                            close_pane_when_last: false,
                        }],
                    },
                ],
            }],
        };

        let toml = layout.to_toml_string().unwrap();
        assert!(toml.contains("version = 1"));
        assert!(toml.contains("run = \"cargo run -p con\""));

        let decoded = WorkspaceLayout::from_toml_str(&toml).unwrap();
        assert_eq!(decoded, layout);
    }

    #[test]
    fn workspace_layout_rejects_dangling_layout_panes() {
        let toml = r#"
version = 1

[[tabs]]
id = "dev"
layout = { kind = "pane", id = "missing" }
panes = []
"#;

        let err = WorkspaceLayout::from_toml_str(toml).unwrap_err();
        assert!(err.to_string().contains("undefined pane"));
    }
}
