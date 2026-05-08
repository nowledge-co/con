use anyhow::Result;
use con_agent::AgentConfig;
use serde::{Deserialize, Serialize};
use std::io::Write;
use std::path::{Path, PathBuf};

pub const MIN_UI_FONT_SIZE: f32 = 12.0;
pub const MAX_UI_FONT_SIZE: f32 = 24.0;
pub const DEFAULT_TERMINAL_FONT_FAMILY: &str = "Ioskeley Mono";

fn default_font_family() -> String {
    DEFAULT_TERMINAL_FONT_FAMILY.into()
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
fn default_tab_accent_inactive_alpha() -> f32 {
    0.15
}
fn default_tab_accent_inactive_hover_alpha() -> f32 {
    0.22
}

fn sanitize_tab_accent_alpha(value: f32, default: f32, max: f32) -> f32 {
    if value.is_finite() {
        value.clamp(AppearanceConfig::MIN_TAB_ACCENT_ALPHA, max)
    } else {
        default
    }
}

fn default_restore_terminal_text() -> bool {
    true
}

pub fn is_gpui_pseudo_font_family(name: &str) -> bool {
    name.trim_start().starts_with('.')
}

pub fn sanitize_terminal_font_family(name: &str) -> String {
    let trimmed = name.trim();
    if trimmed.is_empty() || is_gpui_pseudo_font_family(trimmed) {
        DEFAULT_TERMINAL_FONT_FAMILY.to_string()
    } else {
        trimmed.to_string()
    }
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

/// How the workspace presents its tabs.
///
/// `Horizontal` keeps the historical behavior — a strip of pills along the
/// top of the window that only appears once the second tab opens.
///
/// `Vertical` moves the strip to a left-side panel modelled on Chrome's
/// vertical tabs: a narrow icon rail by default that can be pinned open
/// to show full titles, with a hover-to-peek overlay in between. The top
/// titlebar collapses to its compact one-tab form so the rail owns the
/// tab list.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum TabsOrientation {
    #[default]
    Horizontal,
    Vertical,
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
    /// Accent color alpha for inactive tabs, vertical-tab rows, and unfocused pane titles.
    pub tab_accent_inactive_alpha: f32,
    /// Accent color alpha when hovering inactive tabs and vertical-tab rows.
    pub tab_accent_inactive_hover_alpha: f32,
    /// Keep bounded private terminal text so restart continuity can show what
    /// was on screen. This is never exported to workspace layout profiles.
    pub restore_terminal_text: bool,
    /// Layout of the workspace tab strip. Defaults to `Horizontal` for
    /// backward compatibility with every shipped beta.
    pub tabs_orientation: TabsOrientation,
    /// Hide the per-pane title bar when there are multiple panes. Defaults to
    /// `false` (title bar visible). When `true` the title bar is suppressed
    /// even in split layouts; the fullscreen/close buttons are also hidden.
    pub hide_pane_title_bar: bool,
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
            tab_accent_inactive_alpha: default_tab_accent_inactive_alpha(),
            tab_accent_inactive_hover_alpha: default_tab_accent_inactive_hover_alpha(),
            restore_terminal_text: default_restore_terminal_text(),
            tabs_orientation: TabsOrientation::default(),
            hide_pane_title_bar: false,
        }
    }
}

impl AppearanceConfig {
    pub const MIN_TAB_ACCENT_ALPHA: f32 = 0.05;
    pub const MAX_TAB_ACCENT_INACTIVE_ALPHA: f32 = 0.30;
    pub const MAX_TAB_ACCENT_INACTIVE_HOVER_ALPHA: f32 = 0.40;

    pub fn normalize(&mut self) {
        self.tab_accent_inactive_alpha = sanitize_tab_accent_alpha(
            self.tab_accent_inactive_alpha,
            default_tab_accent_inactive_alpha(),
            Self::MAX_TAB_ACCENT_INACTIVE_ALPHA,
        );
        self.tab_accent_inactive_hover_alpha = sanitize_tab_accent_alpha(
            self.tab_accent_inactive_hover_alpha,
            default_tab_accent_inactive_hover_alpha(),
            Self::MAX_TAB_ACCENT_INACTIVE_HOVER_ALPHA,
        )
        .max(self.tab_accent_inactive_alpha);
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

#[cfg(target_os = "macos")]
fn default_toggle_pane_zoom() -> String {
    "secondary-shift-enter".into()
}
#[cfg(not(target_os = "macos"))]
fn default_toggle_pane_zoom() -> String {
    "alt-shift-enter".into()
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
#[cfg(target_os = "macos")]
fn default_toggle_vertical_tabs() -> String {
    // Cmd+B is the established macOS/editor convention for showing or
    // hiding a sidebar, and Cmd chords do not steal terminal input.
    "secondary-b".into()
}
#[cfg(not(target_os = "macos"))]
fn default_toggle_vertical_tabs() -> String {
    // Avoid bare Ctrl+B on Windows/Linux: it is tmux's prefix and a
    // real terminal control character. Ctrl+Shift+B stays app-level.
    "ctrl-shift-b".into()
}
#[cfg(target_os = "macos")]
fn default_collapse_sidebar() -> String {
    "secondary-shift-b".into()
}
#[cfg(not(target_os = "macos"))]
fn default_collapse_sidebar() -> String {
    "ctrl-alt-b".into()
}

#[cfg(target_os = "macos")]
fn default_new_surface() -> String {
    "secondary-alt-t".into()
}
#[cfg(not(target_os = "macos"))]
fn default_new_surface() -> String {
    "alt-shift-t".into()
}

#[cfg(target_os = "macos")]
fn default_new_surface_split_right() -> String {
    "secondary-alt-d".into()
}
#[cfg(not(target_os = "macos"))]
fn default_new_surface_split_right() -> String {
    "alt-shift-right".into()
}

#[cfg(target_os = "macos")]
fn default_new_surface_split_down() -> String {
    "secondary-alt-shift-d".into()
}
#[cfg(not(target_os = "macos"))]
fn default_new_surface_split_down() -> String {
    "alt-shift-down".into()
}

#[cfg(target_os = "macos")]
fn default_next_surface() -> String {
    "secondary-alt-]".into()
}
#[cfg(not(target_os = "macos"))]
fn default_next_surface() -> String {
    "alt-shift-]".into()
}

#[cfg(target_os = "macos")]
fn default_previous_surface() -> String {
    "secondary-alt-[".into()
}
#[cfg(not(target_os = "macos"))]
fn default_previous_surface() -> String {
    "alt-shift-[".into()
}

#[cfg(target_os = "macos")]
fn default_rename_surface() -> String {
    "secondary-alt-r".into()
}
#[cfg(not(target_os = "macos"))]
fn default_rename_surface() -> String {
    "alt-shift-r".into()
}

#[cfg(target_os = "macos")]
fn default_close_surface() -> String {
    "secondary-alt-shift-w".into()
}
#[cfg(not(target_os = "macos"))]
fn default_close_surface() -> String {
    "alt-shift-x".into()
}

fn default_global_summon() -> String {
    "alt-space".into()
}
fn default_global_summon_enabled() -> bool {
    false
}
fn default_quick_terminal_enabled() -> bool {
    false
}
fn default_quick_terminal() -> String {
    "cmd-\\".into()
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
    pub toggle_pane_zoom: String,
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
    pub toggle_vertical_tabs: String,
    pub collapse_sidebar: String,
    pub new_surface: String,
    pub new_surface_split_right: String,
    pub new_surface_split_down: String,
    pub next_surface: String,
    pub previous_surface: String,
    pub rename_surface: String,
    pub close_surface: String,
    pub global_summon_enabled: bool,
    pub global_summon: String,
    pub quick_terminal_enabled: bool,
    pub quick_terminal: String,
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
            toggle_pane_zoom: default_toggle_pane_zoom(),
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
            toggle_vertical_tabs: default_toggle_vertical_tabs(),
            collapse_sidebar: default_collapse_sidebar(),
            new_surface: default_new_surface(),
            new_surface_split_right: default_new_surface_split_right(),
            new_surface_split_down: default_new_surface_split_down(),
            next_surface: default_next_surface(),
            previous_surface: default_previous_surface(),
            rename_surface: default_rename_surface(),
            close_surface: default_close_surface(),
            global_summon_enabled: default_global_summon_enabled(),
            global_summon: default_global_summon(),
            quick_terminal_enabled: default_quick_terminal_enabled(),
            quick_terminal: default_quick_terminal(),
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
    pub fn normalize(&mut self) {
        self.appearance.normalize();
    }

    pub fn load() -> Result<Self> {
        let config_path = Self::config_path();
        if config_path.exists() {
            let content = std::fs::read_to_string(&config_path)?;
            let mut config: Config = toml::from_str(&content)?;
            config.normalize();
            config.agent.migrate_legacy();
            Ok(config)
        } else {
            let mut config = Config::default();
            config.normalize();
            Ok(config)
        }
    }

    pub fn config_path() -> PathBuf {
        con_paths::config_file()
    }

    pub fn save(&self) -> Result<()> {
        let path = Self::config_path();
        let content = toml::to_string_pretty(self)?;
        write_private_atomic(&path, content.as_bytes())
    }
}

fn write_private_atomic(path: &Path, content: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let tmp_path = {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or_default();
        path.with_extension(format!("tmp.{}.{}", std::process::id(), unique))
    };

    let write_result = (|| -> Result<()> {
        let mut options = std::fs::OpenOptions::new();
        options.create_new(true).write(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&tmp_path)?;
        file.write_all(content)?;
        file.sync_all()?;
        Ok(())
    })();

    if let Err(err) = write_result {
        let _ = std::fs::remove_file(&tmp_path);
        return Err(err);
    }

    replace_file(&tmp_path, path)?;
    Ok(())
}

#[cfg(not(windows))]
fn replace_file(tmp_path: &Path, path: &Path) -> Result<()> {
    std::fs::rename(tmp_path, path)?;
    Ok(())
}

#[cfg(windows)]
fn replace_file(tmp_path: &Path, path: &Path) -> Result<()> {
    if !path.exists() {
        std::fs::rename(tmp_path, path)?;
        return Ok(());
    }

    let backup_path = {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or_default();
        path.with_extension(format!("bak.{}.{}", std::process::id(), unique))
    };

    std::fs::rename(path, &backup_path)?;
    match std::fs::rename(tmp_path, path) {
        Ok(()) => {
            let _ = std::fs::remove_file(&backup_path);
            Ok(())
        }
        Err(err) => {
            let _ = std::fs::rename(&backup_path, path);
            Err(err.into())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        Config, DEFAULT_TERMINAL_FONT_FAMILY, SkillsConfig, sanitize_terminal_font_family,
    };

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

    #[test]
    fn terminal_font_sanitizer_rejects_gpui_pseudo_families() {
        assert_eq!(
            sanitize_terminal_font_family(".ZedMono"),
            DEFAULT_TERMINAL_FONT_FAMILY
        );
        assert_eq!(
            sanitize_terminal_font_family(" .SystemUIFont "),
            DEFAULT_TERMINAL_FONT_FAMILY
        );
        assert_eq!(
            sanitize_terminal_font_family("JetBrains Mono"),
            "JetBrains Mono"
        );
    }

    #[test]
    fn new_configs_enable_restore_terminal_text_by_default() {
        assert!(Config::default().appearance.restore_terminal_text);
    }

    #[test]
    fn loaded_legacy_configs_inherit_restore_terminal_text_default() {
        let content = r#"
[appearance]
terminal_opacity = 0.8
"#;
        let config: Config = toml::from_str(content).unwrap();

        assert!(config.appearance.restore_terminal_text);
    }

    #[test]
    fn loaded_configs_preserve_explicit_restore_terminal_text() {
        let content = r#"
[appearance]
restore_terminal_text = false
"#;
        let config: Config = toml::from_str(content).unwrap();

        assert!(!config.appearance.restore_terminal_text);
    }

    #[test]
    fn default_keybindings_include_quick_terminal_fields() {
        let config = Config::default();
        assert!(!config.keybindings.quick_terminal_enabled);
        assert_eq!(config.keybindings.quick_terminal, "cmd-\\");
    }

    #[test]
    fn serialized_config_omits_removed_quick_terminal_always_on_top_field() {
        let config = Config::default();
        let serialized = toml::to_string(&config).unwrap();
        assert!(!serialized.contains("quick_terminal_always_on_top"));
    }

    #[test]
    fn legacy_configs_receive_quick_terminal_defaults() {
        let content = r#"
[keybindings]
global_summon_enabled = true
global_summon = "alt-space"
"#;
        let config: Config = toml::from_str(content).unwrap();

        assert!(!config.keybindings.quick_terminal_enabled);
        assert_eq!(config.keybindings.quick_terminal, "cmd-\\");
    }

    #[test]
    fn loaded_configs_preserve_explicit_quick_terminal_fields() {
        let content = r#"
[keybindings]
quick_terminal_enabled = true
quick_terminal = "cmd-\\"
"#;
        let config: Config = toml::from_str(content).unwrap();

        assert!(config.keybindings.quick_terminal_enabled);
        assert_eq!(config.keybindings.quick_terminal, "cmd-\\");
    }
}
