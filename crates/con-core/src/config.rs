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
fn default_terminal_blur() -> bool {
    true
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
    pub terminal_blur: bool,
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
            terminal_blur: default_terminal_blur(),
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

// Default keybindings are chosen per platform. On macOS the `secondary-`
// modifier token (⌘) is the right primary: Cmd+<letter> doesn't collide
// with anything the terminal expects. On Windows/Linux `secondary-`
// resolves to `Ctrl`, and bare Ctrl+<letter> often has shell meaning
// (Ctrl+L = clear, Ctrl+C = SIGINT, Ctrl+I = Tab, ...), so most app
// actions avoid that space. Pane-management shortcuts therefore stay in
// app-level modifier space instead of borrowing terminal control
// characters like Ctrl+D, which terminals consume as EOF before Con's
// keybindings ever see them.
#[cfg(target_os = "macos")]
fn default_toggle_agent() -> String {
    "secondary-l".into()
}
#[cfg(not(target_os = "macos"))]
fn default_toggle_agent() -> String {
    "ctrl-shift-l".into()
}

fn default_command_palette() -> String {
    // Ctrl+Shift+P on Windows / Linux, Cmd+Shift+P on macOS.
    "secondary-shift-p".into()
}

#[cfg(target_os = "macos")]
fn default_new_tab() -> String {
    "secondary-t".into()
}
#[cfg(not(target_os = "macos"))]
fn default_new_tab() -> String {
    "ctrl-shift-t".into()
}

#[cfg(target_os = "macos")]
fn default_new_window() -> String {
    "secondary-n".into()
}
#[cfg(not(target_os = "macos"))]
fn default_new_window() -> String {
    "ctrl-shift-n".into()
}

#[cfg(target_os = "macos")]
fn default_close_tab() -> String {
    "secondary-w".into()
}
#[cfg(not(target_os = "macos"))]
fn default_close_tab() -> String {
    "ctrl-shift-w".into()
}

#[cfg(target_os = "macos")]
fn default_close_pane() -> String {
    "secondary-alt-w".into()
}
#[cfg(not(target_os = "macos"))]
fn default_close_pane() -> String {
    "alt-shift-w".into()
}

fn default_next_tab() -> String {
    "ctrl-tab".into()
}
fn default_previous_tab() -> String {
    "ctrl-shift-tab".into()
}

fn default_settings() -> String {
    // Ctrl+, is the cross-editor convention (VSCode, IntelliJ, Windows
    // Terminal) and doesn't produce a control character, so it works
    // the same on both platforms via `secondary-`.
    "secondary-,".into()
}

#[cfg(target_os = "macos")]
fn default_quit() -> String {
    "secondary-q".into()
}
#[cfg(not(target_os = "macos"))]
fn default_quit() -> String {
    // Alt+F4 is the Windows platform convention for "close the app
    // window". Ctrl+Q is XOFF / pwsh's quoted-insert, so it can't be
    // used without stealing it from the shell.
    "alt-f4".into()
}

#[cfg(target_os = "macos")]
fn default_split_right() -> String {
    "secondary-d".into()
}
#[cfg(not(target_os = "macos"))]
fn default_split_right() -> String {
    "alt-d".into()
}

#[cfg(target_os = "macos")]
fn default_split_down() -> String {
    "secondary-shift-d".into()
}
#[cfg(not(target_os = "macos"))]
fn default_split_down() -> String {
    "alt-shift-d".into()
}

#[cfg(target_os = "macos")]
fn default_focus_input() -> String {
    "secondary-i".into()
}
#[cfg(not(target_os = "macos"))]
fn default_focus_input() -> String {
    // Ctrl+I is the Tab character (0x09). Ctrl+Shift+I stays free.
    "ctrl-shift-i".into()
}

fn default_toggle_input_bar() -> String {
    "ctrl-`".into()
}
fn default_cycle_input_mode() -> String {
    "secondary-;".into()
}
fn default_toggle_pane_scope() -> String {
    "secondary-'".into()
}
fn default_global_summon() -> String {
    "alt-space".into()
}
fn default_global_summon_enabled() -> bool {
    false
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct KeybindingConfig {
    pub toggle_agent: String,
    pub command_palette: String,
    pub new_window: String,
    pub new_tab: String,
    pub close_tab: String,
    pub close_pane: String,
    pub next_tab: String,
    pub previous_tab: String,
    pub settings: String,
    pub quit: String,
    pub split_right: String,
    pub split_down: String,
    pub focus_input: String,
    pub cycle_input_mode: String,
    pub toggle_input_bar: String,
    pub toggle_pane_scope: String,
    pub global_summon_enabled: bool,
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
            close_pane: default_close_pane(),
            next_tab: default_next_tab(),
            previous_tab: default_previous_tab(),
            settings: default_settings(),
            quit: default_quit(),
            split_right: default_split_right(),
            split_down: default_split_down(),
            focus_input: default_focus_input(),
            cycle_input_mode: default_cycle_input_mode(),
            toggle_input_bar: default_toggle_input_bar(),
            toggle_pane_scope: default_toggle_pane_scope(),
            global_summon_enabled: default_global_summon_enabled(),
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
/// On Windows, the default global path uses `~/.config/con-terminal/skills`
/// because `con` is a reserved DOS device name.
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
            global_paths: vec![
                con_paths::default_global_skills_path(),
                "~/.agents/skills".into(),
            ],
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
        con_paths::config_file()
    }
}

#[cfg(test)]
mod tests {
    use super::SkillsConfig;

    #[test]
    fn default_skill_path_uses_shared_app_path_policy() {
        let config = SkillsConfig::default();
        assert_eq!(
            config.global_paths.first().map(String::as_str),
            Some(if cfg!(target_os = "windows") {
                "~/.config/con-terminal/skills"
            } else {
                "~/.config/con/skills"
            })
        );
    }
}
