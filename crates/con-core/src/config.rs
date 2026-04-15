use anyhow::Result;
use con_agent::AgentConfig;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

pub const MIN_UI_FONT_SIZE: f32 = 12.0;
pub const MAX_UI_FONT_SIZE: f32 = 24.0;

fn default_font_family() -> String {
    "Ioskeley Mono".into()
}
fn default_font_size() -> f32 {
    14.0
}
fn default_theme() -> String {
    "flexoki-light".into()
}
fn default_cursor_style() -> String {
    "bar".into()
}
fn default_ui_font_family() -> String {
    ".SystemUIFont".into()
}
fn default_ui_font_size() -> f32 {
    16.0f32.clamp(MIN_UI_FONT_SIZE, MAX_UI_FONT_SIZE)
}
fn default_terminal_opacity() -> f32 {
    0.80
}
fn default_ui_opacity() -> f32 {
    0.90
}
fn default_background_image_opacity() -> f32 {
    0.55
}
fn default_background_image_position() -> String {
    "center".into()
}
fn default_background_image_fit() -> String {
    "contain".into()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct TerminalConfig {
    pub font_family: String,
    pub font_size: f32,
    pub theme: String,
    pub cursor_style: String,
}

impl Default for TerminalConfig {
    fn default() -> Self {
        Self {
            font_family: default_font_family(),
            font_size: default_font_size(),
            theme: default_theme(),
            cursor_style: default_cursor_style(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AppearanceConfig {
    pub terminal_opacity: f32,
    pub ui_opacity: f32,
    pub ui_font_family: String,
    pub ui_font_size: f32,
    pub background_image: Option<String>,
    pub background_image_opacity: f32,
    pub background_image_position: String,
    pub background_image_fit: String,
    pub background_image_repeat: bool,
}

impl Default for AppearanceConfig {
    fn default() -> Self {
        Self {
            terminal_opacity: default_terminal_opacity(),
            ui_opacity: default_ui_opacity(),
            ui_font_family: default_ui_font_family(),
            ui_font_size: default_ui_font_size(),
            background_image: None,
            background_image_opacity: default_background_image_opacity(),
            background_image_position: default_background_image_position(),
            background_image_fit: default_background_image_fit(),
            background_image_repeat: false,
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
fn default_new_window() -> String {
    "cmd-n".into()
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
    "cmd-i".into()
}
fn default_toggle_input_bar() -> String {
    "ctrl-`".into()
}
fn default_cycle_input_mode() -> String {
    "cmd-;".into()
}
fn default_toggle_pane_scope() -> String {
    "cmd-'".into()
}
fn default_global_summon() -> String {
    String::new()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct KeybindingConfig {
    pub toggle_agent: String,
    pub command_palette: String,
    pub new_window: String,
    pub new_tab: String,
    pub close_tab: String,
    pub settings: String,
    pub quit: String,
    pub split_right: String,
    pub split_down: String,
    pub focus_input: String,
    pub cycle_input_mode: String,
    pub toggle_input_bar: String,
    pub toggle_pane_scope: String,
    pub global_summon: String,
}

impl Default for KeybindingConfig {
    fn default() -> Self {
        Self {
            toggle_agent: default_toggle_agent(),
            command_palette: default_command_palette(),
            new_window: default_new_window(),
            new_tab: default_new_tab(),
            close_tab: default_close_tab(),
            settings: default_settings(),
            quit: default_quit(),
            split_right: default_split_right(),
            split_down: default_split_down(),
            focus_input: default_focus_input(),
            cycle_input_mode: default_cycle_input_mode(),
            toggle_input_bar: default_toggle_input_bar(),
            toggle_pane_scope: default_toggle_pane_scope(),
            global_summon: default_global_summon(),
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub terminal: TerminalConfig,
    pub appearance: AppearanceConfig,
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
/// # Scan other agent-specific and shared paths too
/// project_paths = [".con/skills", ".agents/skills"]
/// global_paths = ["~/.config/con/skills", "~/.agents/skills"]
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
