use anyhow::Result;
use con_agent::AgentConfig;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

fn default_font_family() -> String { "JetBrains Mono".into() }
fn default_font_size() -> f32 { 14.0 }
fn default_theme() -> String { "catppuccin-mocha".into() }
fn default_scrollback() -> usize { 10_000 }
fn default_cursor_style() -> String { "block".into() }

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct TerminalConfig {
    pub font_family: String,
    pub font_size: f32,
    pub theme: String,
    pub scrollback_lines: usize,
    pub cursor_style: String,
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

fn default_toggle_agent() -> String { "cmd+l".into() }
fn default_command_palette() -> String { "cmd+shift+p".into() }
fn default_new_tab() -> String { "cmd+t".into() }
fn default_split_right() -> String { "cmd+d".into() }
fn default_split_down() -> String { "cmd+shift+d".into() }

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct KeybindingConfig {
    pub toggle_agent: String,
    pub command_palette: String,
    pub new_tab: String,
    pub split_right: String,
    pub split_down: String,
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

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub terminal: TerminalConfig,
    pub agent: AgentConfig,
    pub keybindings: KeybindingConfig,
}

impl Config {
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
