use anyhow::Result;
use con_agent::AgentConfig;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

fn default_font_family() -> String {
    "Ioskeley Mono".into()
}
fn default_font_size() -> f32 {
    14.0
}
fn default_theme() -> String {
    "flexoki-light".into()
}
fn default_scrollback() -> usize {
    10_000
}
fn default_cursor_style() -> String {
    "bar".into()
}
fn default_backend() -> String {
    "auto".into()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct TerminalConfig {
    pub font_family: String,
    pub font_size: f32,
    pub theme: String,
    pub scrollback_lines: usize,
    pub cursor_style: String,
    /// Terminal backend: "auto" (ghostty on macOS, grid elsewhere), "grid", or "ghostty" (macOS only).
    pub backend: String,
}

impl Default for TerminalConfig {
    fn default() -> Self {
        Self {
            font_family: default_font_family(),
            font_size: default_font_size(),
            theme: default_theme(),
            scrollback_lines: default_scrollback(),
            cursor_style: default_cursor_style(),
            backend: default_backend(),
        }
    }
}

impl TerminalConfig {
    /// Resolve the effective backend based on config value and platform.
    /// Returns true if ghostty should be used.
    pub fn use_ghostty(&self) -> bool {
        match self.backend.as_str() {
            "ghostty" => cfg!(target_os = "macos"),
            "grid" => false,
            // "auto" — use ghostty on macOS, grid elsewhere
            _ => cfg!(target_os = "macos"),
        }
    }
}

fn default_toggle_agent() -> String {
    "cmd-l".into()
}
fn default_command_palette() -> String {
    "cmd-shift-p".into()
}
fn default_new_tab() -> String {
    "cmd-t".into()
}
fn default_close_tab() -> String {
    "cmd-w".into()
}
fn default_settings() -> String {
    "cmd-,".into()
}
fn default_quit() -> String {
    "cmd-q".into()
}
fn default_split_right() -> String {
    "cmd-d".into()
}
fn default_split_down() -> String {
    "cmd-shift-d".into()
}
fn default_focus_input() -> String {
    "cmd-k".into()
}
fn default_toggle_input_bar() -> String {
    "ctrl-`".into()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct KeybindingConfig {
    pub toggle_agent: String,
    pub command_palette: String,
    pub new_tab: String,
    pub close_tab: String,
    pub settings: String,
    pub quit: String,
    pub split_right: String,
    pub split_down: String,
    pub focus_input: String,
    pub toggle_input_bar: String,
}

impl Default for KeybindingConfig {
    fn default() -> Self {
        Self {
            toggle_agent: default_toggle_agent(),
            command_palette: default_command_palette(),
            new_tab: default_new_tab(),
            close_tab: default_close_tab(),
            settings: default_settings(),
            quit: default_quit(),
            split_right: default_split_right(),
            split_down: default_split_down(),
            focus_input: default_focus_input(),
            toggle_input_bar: default_toggle_input_bar(),
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub terminal: TerminalConfig,
    pub agent: AgentConfig,
    pub keybindings: KeybindingConfig,
    pub skills: SkillsConfig,
}

/// Configuration for skill discovery paths.
///
/// Skills are SKILL.md files following the open skills ecosystem format (skills.sh).
/// con scans both project-local and global directories for skills.
///
/// # Defaults
/// ```toml
/// [skills]
/// project_paths = [".con/skills"]
/// global_paths = ["~/.config/con/skills"]
/// ```
///
/// # Sharing with other agents
/// ```toml
/// [skills]
/// # Scan Claude Code + universal agents paths too
/// project_paths = [".con/skills", ".claude/skills", ".agents/skills"]
/// global_paths = ["~/.config/con/skills", "~/.claude/skills"]
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct SkillsConfig {
    /// Project-local skill directories (relative to cwd).
    /// Scanned in order; later entries override earlier ones on name collision.
    pub project_paths: Vec<String>,
    /// Global skill directories (absolute paths, ~ expanded).
    /// Scanned before project paths; project skills override global on collision.
    pub global_paths: Vec<String>,
}

impl Default for SkillsConfig {
    fn default() -> Self {
        Self {
            project_paths: vec![
                "skills".into(),
                ".agents/skills".into(),
                ".con/skills".into(),
            ],
            global_paths: vec!["~/.config/con/skills".into(), "~/.agents/skills".into()],
        }
    }
}

impl SkillsConfig {
    /// Resolve global paths, expanding ~ to the user's home directory.
    pub fn resolved_global_paths(&self) -> Vec<PathBuf> {
        let home = dirs::home_dir();
        self.global_paths
            .iter()
            .map(|p| {
                if p.starts_with("~/") {
                    if let Some(ref h) = home {
                        h.join(&p[2..])
                    } else {
                        PathBuf::from(p)
                    }
                } else {
                    PathBuf::from(p)
                }
            })
            .collect()
    }

    /// Resolve project-local paths relative to a cwd.
    pub fn resolved_project_paths(&self, cwd: &std::path::Path) -> Vec<PathBuf> {
        self.project_paths.iter().map(|p| cwd.join(p)).collect()
    }
}

impl Config {
    pub fn load() -> Result<Self> {
        let config_path = Self::config_path();
        if config_path.exists() {
            let content = std::fs::read_to_string(&config_path)?;
            let mut config: Config = toml::from_str(&content)?;
            config.agent.migrate_legacy();
            Ok(config)
        } else {
            Ok(Config::default())
        }
    }

    pub fn config_path() -> PathBuf {
        dirs::config_dir()
            .or_else(|| dirs::home_dir().map(|h| h.join(".config")))
            .unwrap_or_else(|| PathBuf::from("/tmp"))
            .join("con")
            .join("config.toml")
    }
}
