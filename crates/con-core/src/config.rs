use anyhow::Result;
use con_agent::AgentConfig;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TerminalConfig {
    #[serde(default = "default_font_family")]
    pub font_family: String,
    #[serde(default = "default_font_size")]
    pub font_size: f32,
    #[serde(default = "default_theme")]
    pub theme: String,
    #[serde(default = "default_scrollback")]
    pub scrollback_lines: usize,
    #[serde(default = "default_cursor_style")]
    pub cursor_style: String,
}

fn default_font_family() -> String {
    "JetBrains Mono".to_string()
}
fn default_font_size() -> f32 {
    14.0
}
fn default_theme() -> String {
    "catppuccin-mocha".to_string()
}
fn default_scrollback() -> usize {
    10_000
}
fn default_cursor_style() -> String {
    "block".to_string()
}

impl Default for TerminalConfig {
    fn default() -> Self {
        Self {
            font_family: default_font_family(),
            font_size: default_font_size(),
            theme: default_theme(),
            scrollback_lines: default_scrollback(),
            cursor_style: default_cursor_style(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeybindingConfig {
    #[serde(default = "default_toggle_agent")]
    pub toggle_agent: String,
    #[serde(default = "default_command_palette")]
    pub command_palette: String,
    #[serde(default = "default_new_tab")]
    pub new_tab: String,
    #[serde(default = "default_split_right")]
    pub split_right: String,
    #[serde(default = "default_split_down")]
    pub split_down: String,
}

fn default_toggle_agent() -> String {
    "cmd+l".to_string()
}
fn default_command_palette() -> String {
    "cmd+shift+p".to_string()
}
fn default_new_tab() -> String {
    "cmd+t".to_string()
}
fn default_split_right() -> String {
    "cmd+d".to_string()
}
fn default_split_down() -> String {
    "cmd+shift+d".to_string()
}

impl Default for KeybindingConfig {
    fn default() -> Self {
        Self {
            toggle_agent: default_toggle_agent(),
            command_palette: default_command_palette(),
            new_tab: default_new_tab(),
            split_right: default_split_right(),
            split_down: default_split_down(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub terminal: TerminalConfig,
    #[serde(default)]
    pub agent: AgentConfig,
    #[serde(default)]
    pub keybindings: KeybindingConfig,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            terminal: TerminalConfig::default(),
            agent: AgentConfig::default(),
            keybindings: KeybindingConfig::default(),
        }
    }
}

impl Config {
    /// Load config from the standard path (~/.config/con/config.toml)
    pub fn load() -> Result<Self> {
        let config_path = Self::config_path();
        if config_path.exists() {
            let content = std::fs::read_to_string(&config_path)?;
            let config: Config = toml::from_str(&content)?;
            Ok(config)
        } else {
            Ok(Config::default())
        }
    }

    pub fn config_path() -> PathBuf {
        dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("~/.config"))
            .join("con")
            .join("config.toml")
    }
}
