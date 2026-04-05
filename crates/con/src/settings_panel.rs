use con_agent::{AgentConfig, ProviderConfig, ProviderKind, SuggestionModelConfig};
use con_core::Config;
use gpui::*;

use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::input::InputState;
use gpui_component::select::{SearchableVec, Select, SelectEvent, SelectState};
use gpui_component::switch::Switch;
use gpui_component::{ActiveTheme, Icon, IndexPath, Sizable as _, input::Input};

use crate::model_registry::ModelRegistry;

actions!(settings, [ToggleSettings, SaveSettings, DismissSettings]);

/// Emitted when the user selects a different terminal theme for live preview.
pub struct ThemePreview(pub String);

#[derive(Debug, Clone, Copy, PartialEq)]
enum SettingsSection {
    General,
    Appearance,
    Models,
    Keys,
}

impl SettingsSection {
    fn label(&self) -> &'static str {
        match self {
            Self::General => "General",
            Self::Appearance => "Appearance",
            Self::Models => "Models",
            Self::Keys => "Keys",
        }
    }

    fn icon(&self) -> &'static str {
        match self {
            Self::General => "phosphor/sliders.svg",
            Self::Appearance => "phosphor/sun.svg",
            Self::Models => "phosphor/robot.svg",
            Self::Keys => "phosphor/keyboard.svg",
        }
    }
}

const ALL_SECTIONS: &[SettingsSection] = &[
    SettingsSection::General,
    SettingsSection::Appearance,
    SettingsSection::Models,
    SettingsSection::Keys,
];

pub struct SettingsPanel {
    visible: bool,
    config: Config,
    focus_handle: FocusHandle,
    active_section: SettingsSection,

    selected_provider: ProviderKind,
    model_input: Entity<InputState>,
    model_select: Entity<SelectState<SearchableVec<String>>>,
    api_key_input: Entity<InputState>,
    base_url_input: Entity<InputState>,
    max_tokens_input: Entity<InputState>,
    max_turns_input: Entity<InputState>,
    temperature_input: Entity<InputState>,
    auto_approve: bool,

    suggestion_model_input: Entity<InputState>,

    font_size_input: Entity<InputState>,
    scrollback_input: Entity<InputState>,
    save_error: Option<String>,

    // Theme import
    custom_theme_name_input: Entity<InputState>,
    custom_theme_preview: Option<con_terminal::TerminalTheme>,
    custom_theme_status: Option<String>,

    // Keybindings — which binding is being recorded (field name, e.g. "new_tab")
    recording_key: Option<String>,
}

const ALL_PROVIDERS: &[ProviderKind] = &[
    ProviderKind::Anthropic,
    ProviderKind::OpenAI,
    ProviderKind::OpenAICompatible,
    ProviderKind::DeepSeek,
    ProviderKind::Groq,
    ProviderKind::Gemini,
    ProviderKind::Ollama,
    ProviderKind::OpenRouter,
    ProviderKind::Mistral,
    ProviderKind::Together,
    ProviderKind::Cohere,
    ProviderKind::Perplexity,
    ProviderKind::XAI,
];

impl SettingsPanel {
    pub fn new(config: &Config, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let agent = &config.agent;
        let pc = agent.providers.get_or_default(&agent.provider);

        let model_input = cx.new(|cx| {
            let mut s = InputState::new(window, cx);
            s.set_placeholder("Provider default", window, cx);
            s.set_value(&pc.model.clone().unwrap_or_default(), window, cx);
            s
        });
        let model_select = Self::make_model_select(
            &agent.provider,
            &pc.model,
            &registry,
            window,
            cx,
        );
        let api_key_input = cx.new(|cx| {
            let mut s = InputState::new(window, cx);
            s.set_placeholder("sk-... or env var like ANTHROPIC_API_KEY", window, cx);
            let val = pc
                .api_key
                .clone()
                .or_else(|| pc.api_key_env.clone())
                .unwrap_or_default();
            s.set_value(&val, window, cx);
            s
        });
        let base_url_input = cx.new(|cx| {
            let mut s = InputState::new(window, cx);
            s.set_placeholder("Default endpoint", window, cx);
            s.set_value(&pc.base_url.clone().unwrap_or_default(), window, cx);
            s
        });
        let max_tokens_input = cx.new(|cx| {
            let mut s = InputState::new(window, cx);
            s.set_placeholder("Provider default", window, cx);
            s.set_value(
                &pc.max_tokens.map(|t| t.to_string()).unwrap_or_default(),
                window,
                cx,
            );
            s
        });
        let max_turns_input = cx.new(|cx| {
            let mut s = InputState::new(window, cx);
            s.set_placeholder("10", window, cx);
            s.set_value(&agent.max_turns.to_string(), window, cx);
            s
        });
        let temperature_input = cx.new(|cx| {
            let mut s = InputState::new(window, cx);
            s.set_placeholder("Provider default", window, cx);
            s.set_value(
                &agent.temperature.map(|t| t.to_string()).unwrap_or_default(),
                window,
                cx,
            );
            s
        });
        let suggestion_model_input = cx.new(|cx| {
            let mut s = InputState::new(window, cx);
            s.set_placeholder("Same as agent model", window, cx);
            s.set_value(
                &agent.suggestion_model.model.clone().unwrap_or_default(),
                window,
                cx,
            );
            s
        });
        let font_size_input = cx.new(|cx| {
            let mut s = InputState::new(window, cx);
            s.set_placeholder("14.0", window, cx);
            s.set_value(&config.terminal.font_size.to_string(), window, cx);
            s
        });
        let scrollback_input = cx.new(|cx| {
            let mut s = InputState::new(window, cx);
            s.set_placeholder("10000", window, cx);
            s.set_value(&config.terminal.scrollback_lines.to_string(), window, cx);
            s
        });
        let custom_theme_name_input = cx.new(|cx| {
            let mut s = InputState::new(window, cx);
            s.set_placeholder("Save as, e.g. flexoki-amber", window, cx);
            s
        });

        Self {
            visible: false,
            config: config.clone(),
            focus_handle: cx.focus_handle(),
            active_section: SettingsSection::General,
            selected_provider: config.agent.provider.clone(),
            model_input,
            model_select,
            api_key_input,
            base_url_input,
            max_tokens_input,
            max_turns_input,
            temperature_input,
            auto_approve: config.agent.auto_approve_tools,
            suggestion_model_input,
            font_size_input,
            scrollback_input,
            save_error: None,
            custom_theme_name_input,
            custom_theme_preview: None,
            custom_theme_status: None,
            recording_key: None,
        }
    }

    pub fn toggle(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.visible = !self.visible;
        if self.visible {
            let agent = &self.config.agent;
            self.selected_provider = agent.provider.clone();
            let pc = agent.providers.get_or_default(&self.selected_provider);
            self.load_provider_inputs(&pc, window, cx);
            self.max_turns_input.update(cx, |s, cx| {
                s.set_value(&agent.max_turns.to_string(), window, cx)
            });
            self.temperature_input.update(cx, |s, cx| {
                s.set_value(
                    &agent.temperature.map(|t| t.to_string()).unwrap_or_default(),
                    window,
                    cx,
                )
            });
            self.suggestion_model_input.update(cx, |s, cx| {
                s.set_value(
                    &agent.suggestion_model.model.clone().unwrap_or_default(),
                    window,
                    cx,
                )
            });
            self.auto_approve = agent.auto_approve_tools;
            self.model_select =
                Self::make_model_select(&agent.provider, &pc.model, &self.registry, window, cx);
            self.font_size_input.update(cx, |s, cx| {
                s.set_value(&self.config.terminal.font_size.to_string(), window, cx)
            });
            self.scrollback_input.update(cx, |s, cx| {
                s.set_value(
                    &self.config.terminal.scrollback_lines.to_string(),
                    window,
                    cx,
                )
            });
            self.recording_key = None;
            self.focus_handle.focus(window, cx);
        }
        cx.notify();
    }

    /// Load a provider's config into the per-provider input fields.
    fn load_provider_inputs(
        &self,
        pc: &ProviderConfig,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.model_input.update(cx, |s, cx| {
            s.set_value(&pc.model.clone().unwrap_or_default(), window, cx)
        });
        let key_val = pc
            .api_key
            .clone()
            .or_else(|| pc.api_key_env.clone())
            .unwrap_or_default();
        self.api_key_input
            .update(cx, |s, cx| s.set_value(&key_val, window, cx));
        self.base_url_input.update(cx, |s, cx| {
            s.set_value(&pc.base_url.clone().unwrap_or_default(), window, cx)
        });
        self.max_tokens_input.update(cx, |s, cx| {
            s.set_value(
                &pc.max_tokens.map(|t| t.to_string()).unwrap_or_default(),
                window,
                cx,
            )
        });
    }

    /// Build a model select entity for the given provider.
    fn make_model_select(
        provider: &ProviderKind,
        current_model: &Option<String>,
        registry: &ModelRegistry,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<SelectState<SearchableVec<String>>> {
        let models: Vec<String> = registry.models_for(provider);
        let selected_index = current_model.as_ref().and_then(|m| {
            models.iter().position(|item| item == m).map(IndexPath::new)
        });
        let entity = cx.new(|cx| {
            SelectState::new(
                SearchableVec::new(models),
                selected_index,
                window,
                cx,
            )
            .searchable(true)
        });
        cx.subscribe_in(&entity, window, |this, _, ev: &SelectEvent<SearchableVec<String>>, window, cx| {
            if let SelectEvent::Confirm(Some(value)) = ev {
                this.model_input.update(cx, |s, cx| {
                    s.set_value(value, window, cx);
                });
                cx.notify();
            }
        }).detach();
        entity
    }

    /// Parse a ghostty config from clipboard and show a live preview.
    fn paste_theme_from_clipboard(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        let text = match cx
            .read_from_clipboard()
            .and_then(|c| c.text().map(|s| s.to_string()))
        {
            Some(t) if !t.trim().is_empty() => t,
            _ => {
                self.custom_theme_status =
                    Some("Error: clipboard is empty. Copy a Ghostty theme first.".into());
                cx.notify();
                return;
            }
        };

        // Get theme name from input, fallback to "custom"
        let name_raw = self.custom_theme_name_input.read(cx).value().to_string();
        let name = if name_raw.trim().is_empty() {
            "custom".to_string()
        } else {
            name_raw.trim().to_lowercase().replace(' ', "-")
        };

        match con_terminal::TerminalTheme::from_ghostty_format(&name, &text) {
            Some(theme) => {
                self.custom_theme_status = Some(format!(
                    "Loaded \"{}\" from the clipboard. {} ANSI colors are ready to preview.",
                    display_theme_name(&name),
                    theme.ansi.len()
                ));
                self.custom_theme_preview = Some(theme);
            }
            None => {
                self.custom_theme_status = Some(
                    "Error: couldn't read a Ghostty theme. Include background, foreground, and palette entries.".into()
                );
                self.custom_theme_preview = None;
            }
        }
        cx.notify();
    }

    /// Save the custom theme to the user themes directory and apply it.
    fn apply_custom_theme(&mut self, cx: &mut Context<Self>) {
        let preview = match &self.custom_theme_preview {
            Some(t) => t.clone(),
            None => return,
        };

        // Determine theme directory
        let theme_dir = if cfg!(target_os = "macos") {
            std::env::var("HOME")
                .ok()
                .map(|h| std::path::PathBuf::from(h).join("Library/Application Support/con/themes"))
        } else {
            std::env::var("XDG_CONFIG_HOME")
                .ok()
                .map(std::path::PathBuf::from)
                .or_else(|| {
                    std::env::var("HOME")
                        .ok()
                        .map(|h| std::path::PathBuf::from(h).join(".config"))
                })
                .map(|p| p.join("con/themes"))
        };

        let dir = match theme_dir {
            Some(d) => d,
            None => {
                self.custom_theme_status =
                    Some("Error: could not determine themes directory".into());
                cx.notify();
                return;
            }
        };

        // Create directory if needed
        if let Err(e) = std::fs::create_dir_all(&dir) {
            self.custom_theme_status = Some(format!("Error: {}", e));
            cx.notify();
            return;
        }

        // Read the config text from clipboard again for saving
        let text = match cx
            .read_from_clipboard()
            .and_then(|c| c.text().map(|s| s.to_string()))
        {
            Some(t) if !t.trim().is_empty() => t,
            _ => {
                self.custom_theme_status =
                    Some("Error: the clipboard changed before save. Load the theme again.".into());
                cx.notify();
                return;
            }
        };

        let file_path = dir.join(&preview.name);
        if let Err(e) = std::fs::write(&file_path, &text) {
            self.custom_theme_status = Some(format!("Error: {}", e));
            cx.notify();
            return;
        }

        // Apply the theme
        self.config.terminal.theme = preview.name.clone();
        cx.emit(ThemePreview(preview.name.clone()));
        self.custom_theme_status = Some(format!("Saved and applied: {}", file_path.display()));
        self.custom_theme_preview = None;
        cx.notify();
    }

    /// Read current per-provider input fields into a ProviderConfig.
    fn read_provider_inputs(&self, cx: &App) -> ProviderConfig {
        let key_text = self.api_key_input.read(cx).value().to_string();
        let is_env_var = !key_text.is_empty()
            && key_text
                .chars()
                .all(|c| c.is_ascii_uppercase() || c == '_' || c.is_ascii_digit());

        let (api_key, api_key_env) = if key_text.is_empty() {
            (None, None)
        } else if is_env_var {
            (None, Some(key_text))
        } else {
            (Some(key_text), None)
        };

        let model_text = self.model_input.read(cx).value().to_string();
        let base_url_text = self.base_url_input.read(cx).value().to_string();
        let max_tokens_text = self.max_tokens_input.read(cx).value().to_string();

        ProviderConfig {
            model: if model_text.is_empty() {
                None
            } else {
                Some(model_text)
            },
            api_key,
            api_key_env,
            base_url: if base_url_text.is_empty() {
                None
            } else {
                Some(base_url_text)
            },
            max_tokens: if max_tokens_text.is_empty() {
                None
            } else {
                max_tokens_text.parse().ok()
            },
        }
    }

    pub fn is_visible(&self) -> bool {
        self.visible
    }

    fn save(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        let max_turns_text = self.max_turns_input.read(cx).value().to_string();
        let temperature_text = self.temperature_input.read(cx).value().to_string();
        let suggestion_model_text = self.suggestion_model_input.read(cx).value().to_string();
        let font_size_text = self.font_size_input.read(cx).value().to_string();
        let scrollback_text = self.scrollback_input.read(cx).value().to_string();

        // Save current provider's per-provider fields into the map
        let pc = self.read_provider_inputs(cx);
        self.config.agent.providers.set(&self.selected_provider, pc);

        // Update global fields
        self.config.agent.provider = self.selected_provider.clone();
        self.config.agent.max_turns = max_turns_text.parse().unwrap_or(10);
        self.config.agent.temperature = if temperature_text.is_empty() {
            None
        } else {
            temperature_text.parse().ok()
        };
        self.config.agent.auto_approve_tools = self.auto_approve;
        self.config.agent.suggestion_model = SuggestionModelConfig {
            provider: None,
            model: if suggestion_model_text.is_empty() {
                None
            } else {
                Some(suggestion_model_text)
            },
        };
        self.config.terminal.font_size = font_size_text.parse().unwrap_or(14.0);
        self.config.terminal.scrollback_lines = scrollback_text.parse().unwrap_or(10_000);

        // Keybindings are updated directly via record_keystroke — no reading needed

        match self.persist_config() {
            Ok(()) => {
                self.save_error = None;
                self.visible = false;
                cx.emit(SaveSettings);
            }
            Err(e) => {
                log::error!("Failed to save config: {}", e);
                self.save_error = Some(e.to_string());
            }
        }
        cx.notify();
    }

    /// Record a keystroke for the binding currently being recorded.
    fn record_keystroke(&mut self, keystroke: &Keystroke, cx: &mut Context<Self>) {
        let field = match &self.recording_key {
            Some(f) => f.clone(),
            None => return,
        };

        // Don't record bare modifier keys or escape (used to cancel)
        let key = &keystroke.key;
        if matches!(
            key.as_str(),
            "shift" | "control" | "alt" | "meta" | "fn" | "escape"
        ) {
            if key == "escape" {
                self.recording_key = None;
                cx.notify();
            }
            return;
        }

        // Build GPUI binding format: cmd-shift-k
        let binding = keystroke_to_binding(keystroke);

        // Write directly into config
        match field.as_str() {
            "new_tab" => self.config.keybindings.new_tab = binding,
            "close_tab" => self.config.keybindings.close_tab = binding,
            "settings" => self.config.keybindings.settings = binding,
            "command_palette" => self.config.keybindings.command_palette = binding,
            "toggle_agent" => self.config.keybindings.toggle_agent = binding,
            "toggle_input_bar" => self.config.keybindings.toggle_input_bar = binding,
            "focus_input" => self.config.keybindings.focus_input = binding,
            "split_right" => self.config.keybindings.split_right = binding,
            "split_down" => self.config.keybindings.split_down = binding,
            "quit" => self.config.keybindings.quit = binding,
            _ => {}
        }
        self.recording_key = None;
        cx.notify();
    }

    /// Get the current value of a keybinding by field name.
    fn binding_value(&self, field: &str) -> &str {
        match field {
            "new_tab" => &self.config.keybindings.new_tab,
            "close_tab" => &self.config.keybindings.close_tab,
            "settings" => &self.config.keybindings.settings,
            "command_palette" => &self.config.keybindings.command_palette,
            "toggle_agent" => &self.config.keybindings.toggle_agent,
            "toggle_input_bar" => &self.config.keybindings.toggle_input_bar,
            "focus_input" => &self.config.keybindings.focus_input,
            "split_right" => &self.config.keybindings.split_right,
            "split_down" => &self.config.keybindings.split_down,
            "quit" => &self.config.keybindings.quit,
            _ => "",
        }
    }

    pub fn agent_config(&self) -> &AgentConfig {
        &self.config.agent
    }
    pub fn terminal_config(&self) -> &con_core::config::TerminalConfig {
        &self.config.terminal
    }
    pub fn keybinding_config(&self) -> &con_core::config::KeybindingConfig {
        &self.config.keybindings
    }
    pub fn skills_config(&self) -> &con_core::config::SkillsConfig {
        &self.config.skills
    }

    fn persist_config(&self) -> anyhow::Result<()> {
        let path = Config::config_path();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let content = toml::to_string_pretty(&self.config)?;
        std::fs::write(&path, &content)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;
        }
        Ok(())
    }

    fn select_provider(
        &mut self,
        provider: ProviderKind,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // Save current provider's inputs into the providers map
        let current_pc = self.read_provider_inputs(cx);
        self.config
            .agent
            .providers
            .set(&self.selected_provider, current_pc);

        // Switch to new provider
        self.selected_provider = provider.clone();

        // Load new provider's saved config (or defaults)
        let pc = self.config.agent.providers.get_or_default(&provider);
        self.load_provider_inputs(&pc, window, cx);

        // Rebuild model select for new provider
        self.model_select = Self::make_model_select(&provider, &pc.model, &self.registry, window, cx);
        cx.notify();
    }

    // ── Section content ──────────────────────────────────────────

    fn render_general(&mut self, cx: &mut Context<Self>) -> Div {
        let font_size_input = self.font_size_input.clone();
        let scrollback_input = self.scrollback_input.clone();

        // Auto-approve toggle
        let is_on = self.auto_approve;
        let toggle = Switch::new("auto-approve-toggle")
            .checked(is_on)
            .small()
            .on_click(cx.listener(|this, checked: &bool, _, cx| {
                this.auto_approve = *checked;
                cx.notify();
            }));

        // --- Skills path chips (must render before borrowing theme) ---
        let project_paths = self.config.skills.project_paths.clone();
        let global_paths = self.config.skills.global_paths.clone();

        let project_chips = self.render_path_chips("project", &project_paths, cx);
        let global_chips = self.render_path_chips("global", &global_paths, cx);

        let project_presets = self.render_path_presets(
            "project",
            &project_paths,
            &[
                ("skills", "con"),
                (".con/skills", "con local"),
                (".claude/skills", "Claude Code"),
                (".agents/skills", "Agents"),
                (".codex/skills", "Codex"),
            ],
            cx,
        );
        let global_presets = self.render_path_presets(
            "global",
            &global_paths,
            &[
                ("~/.config/con/skills", "con"),
                ("~/.claude/skills", "Claude Code"),
                ("~/.agents/skills", "Agents"),
                ("~/.codex/skills", "Codex"),
            ],
            cx,
        );

        let theme = cx.theme();
        section_content("General", "Terminal and agent behavior.", theme)
            .child(
                card(theme)
                    .child(row_field("Font Size", &font_size_input))
                    .child(row_separator(theme))
                    .child(row_field("Scrollback Lines", &scrollback_input)),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(8.0))
                    .child(group_label("Agent", &theme))
                    .child(
                        card(theme).child(
                            div()
                                .flex()
                                .items_center()
                                .justify_between()
                                .px(px(16.0))
                                .h(px(44.0))
                                .child(
                                    div()
                                        .flex()
                                        .flex_col()
                                        .gap(px(2.0))
                                        .child(div().text_sm().child("Auto-approve tools"))
                                        .child(
                                            div()
                                                .text_size(px(11.0))
                                                .text_color(theme.muted_foreground)
                                                .child(
                                                    "Allow agent to run tools without confirmation",
                                                ),
                                        ),
                                )
                                .child(toggle),
                        ),
                    ),
            )
            // Skills paths
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(8.0))
                    .child(group_label("Skills", &theme))
                    .child(
                        card(theme)
                            // Project paths row
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .gap(px(6.0))
                                    .px(px(16.0))
                                    .py(px(12.0))
                                    .child(
                                        div()
                                            .flex()
                                            .items_center()
                                            .justify_between()
                                            .child(div().text_sm().child("Project paths"))
                                            .child(
                                                div()
                                                    .text_size(px(10.0))
                                                    .text_color(theme.muted_foreground.opacity(0.5))
                                                    .child("relative to cwd"),
                                            ),
                                    )
                                    .child(
                                        div()
                                            .flex()
                                            .flex_wrap()
                                            .gap(px(4.0))
                                            .children(project_chips)
                                            .children(project_presets),
                                    ),
                            )
                            .child(row_separator(theme))
                            // Global paths row
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .gap(px(6.0))
                                    .px(px(16.0))
                                    .py(px(12.0))
                                    .child(
                                        div()
                                            .flex()
                                            .items_center()
                                            .justify_between()
                                            .child(div().text_sm().child("Global paths"))
                                            .child(
                                                div()
                                                    .text_size(px(10.0))
                                                    .text_color(theme.muted_foreground.opacity(0.5))
                                                    .child("~ expanded to home"),
                                            ),
                                    )
                                    .child(
                                        div()
                                            .flex()
                                            .flex_wrap()
                                            .gap(px(4.0))
                                            .children(global_chips)
                                            .children(global_presets),
                                    ),
                            ),
                    ),
            )
    }

    /// Render removable path chips for a given path list.
    fn render_path_chips(
        &self,
        kind: &str,
        paths: &[String],
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let theme = cx.theme();
        let chip_bg = theme.muted.opacity(0.12);
        let fg = theme.foreground;
        let muted = theme.muted_foreground.opacity(0.5);
        let danger = theme.danger;

        paths
            .iter()
            .enumerate()
            .map(|(idx, path)| {
                let kind = kind.to_string();
                let path_display = path.clone();

                div()
                    .id(SharedString::from(format!("skill-{kind}-{idx}")))
                    .flex()
                    .items_center()
                    .gap(px(4.0))
                    .h(px(24.0))
                    .px(px(8.0))
                    .rounded(px(5.0))
                    .bg(chip_bg)
                    .child(
                        div()
                            .text_size(px(11.0))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(fg)
                            .child(path_display),
                    )
                    .child(
                        div()
                            .id(SharedString::from(format!("skill-rm-{kind}-{idx}")))
                            .flex()
                            .items_center()
                            .justify_center()
                            .size(px(14.0))
                            .rounded(px(3.0))
                            .cursor_pointer()
                            .text_color(muted)
                            .hover(|s| s.text_color(danger))
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(move |this, _, _, cx| {
                                    this.remove_skill_path(&kind, idx);
                                    cx.notify();
                                }),
                            )
                            .child(
                                svg()
                                    .path("phosphor/x.svg")
                                    .size(px(10.0))
                                    .text_color(muted),
                            ),
                    )
                    .into_any_element()
            })
            .collect()
    }

    /// Render preset quick-add buttons for paths not yet in the list.
    fn render_path_presets(
        &self,
        kind: &str,
        current_paths: &[String],
        presets: &[(&str, &str)],
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let theme = cx.theme();
        let muted_fg = theme.muted_foreground.opacity(0.4);
        let hover_bg = theme.muted.opacity(0.08);
        let hover_fg = theme.foreground;

        presets
            .iter()
            .filter(|(path, _)| !current_paths.iter().any(|p| p == path))
            .map(|(path, label)| {
                let kind = kind.to_string();
                let path = path.to_string();
                let label = label.to_string();

                div()
                    .id(SharedString::from(format!("skill-add-{kind}-{label}")))
                    .flex()
                    .items_center()
                    .gap(px(3.0))
                    .h(px(24.0))
                    .px(px(8.0))
                    .rounded(px(5.0))
                    .cursor_pointer()
                    .text_color(muted_fg)
                    .hover(|s| s.bg(hover_bg).text_color(hover_fg))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, _, _, cx| {
                            this.add_skill_path(&kind, &path);
                            cx.notify();
                        }),
                    )
                    .child(
                        svg()
                            .path("phosphor/plus.svg")
                            .size(px(10.0))
                            .text_color(muted_fg),
                    )
                    .child(div().text_size(px(10.0)).child(label))
                    .into_any_element()
            })
            .collect()
    }

    fn add_skill_path(&mut self, kind: &str, path: &str) {
        let paths = match kind {
            "project" => &mut self.config.skills.project_paths,
            "global" => &mut self.config.skills.global_paths,
            _ => return,
        };
        if !paths.iter().any(|p| p == path) {
            paths.push(path.to_string());
        }
    }

    fn remove_skill_path(&mut self, kind: &str, idx: usize) {
        let paths = match kind {
            "project" => &mut self.config.skills.project_paths,
            "global" => &mut self.config.skills.global_paths,
            _ => return,
        };
        if idx < paths.len() {
            paths.remove(idx);
        }
    }

    fn render_appearance(&self, cx: &mut Context<Self>) -> Div {
        let current_theme = self.config.terminal.theme.clone();
        let all_themes = con_terminal::TerminalTheme::all_available();

        // Split into built-in and user themes
        let builtin_names: Vec<&str> = con_terminal::TerminalTheme::available().to_vec();
        let mut builtin_themes = Vec::new();
        let mut user_themes = Vec::new();
        for t in all_themes.iter() {
            if builtin_names.contains(&t.name.as_str()) {
                builtin_themes.push(t);
            } else {
                user_themes.push(t);
            }
        }
        let has_user_themes = !user_themes.is_empty();
        let total_count = builtin_themes.len() + user_themes.len();

        // Build theme grids first (these need &mut cx for listeners)
        let builtin_grid = self.render_theme_grid(&builtin_themes, &current_theme, cx);
        let user_grid = if has_user_themes {
            Some(self.render_theme_grid(&user_themes, &current_theme, cx))
        } else {
            None
        };

        // Build import section
        let custom_theme_name_input = self.custom_theme_name_input.clone();
        let paste_btn = Button::new("paste-theme-btn")
            .label("Load from Clipboard")
            .icon(Icon::default().path("phosphor/clipboard-text.svg"))
            .small()
            .ghost()
            .on_click(cx.listener(|this, _, window, cx| {
                this.paste_theme_from_clipboard(window, cx);
            }));
        let open_catalog_btn = Button::new("ghostty-style-link")
            .label("Browse Themes")
            .icon(Icon::default().path("phosphor/arrow-square-out.svg"))
            .small()
            .ghost()
            .on_click(cx.listener(|_, _, _, cx| {
                cx.open_url("https://ghostty-style.vercel.app/");
            }));

        let preview_card = self
            .custom_theme_preview
            .as_ref()
            .map(|preview| self.render_single_theme_card(preview, false, cx));

        let preview_actions: Option<AnyElement> = if let Some(card) = preview_card {
            let apply_btn = Button::new("apply-custom-theme")
                .label("Save & Apply")
                .icon(Icon::default().path("phosphor/check.svg"))
                .small()
                .primary()
                .on_click(cx.listener(|this, _, _, cx| {
                    this.apply_custom_theme(cx);
                }));
            let preview_btn = Button::new("preview-custom-theme")
                .label("Preview")
                .icon(Icon::default().path("phosphor/eye.svg"))
                .small()
                .ghost()
                .on_click(cx.listener(|this, _, _, cx| {
                    if let Some(ref preview) = this.custom_theme_preview {
                        cx.emit(ThemePreview(preview.name.clone()));
                    }
                }));
            Some(
                div()
                    .flex()
                    .items_start()
                    .gap(px(12.0))
                    .child(card)
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(6.0))
                            .child(apply_btn)
                            .child(preview_btn),
                    )
                    .into_any_element(),
            )
        } else {
            None
        };

        // Now all mutable borrows are done — get theme for pure layout
        let theme = cx.theme();

        let mut import_section = div()
            .px(px(18.0))
            .py(px(16.0))
            .flex()
            .flex_col()
            .gap(px(12.0))
            .child(
                div()
                    .text_sm()
                    .font_weight(FontWeight::MEDIUM)
                    .child("Import Theme"),
            )
            .child(
                div()
                    .text_size(px(11.5))
                    .line_height(px(18.0))
                    .text_color(theme.muted_foreground.opacity(0.6))
                    .child("Paste a Ghostty-format theme from the clipboard. Browse 500+ community themes at ghostty-style.vercel.app."),
            )
            // Name input
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(4.0))
                    .child(
                        div()
                            .text_size(px(11.0))
                            .text_color(theme.muted_foreground.opacity(0.5))
                            .child("Theme name"),
                    )
                    .child(Input::new(&custom_theme_name_input)),
            )
            // Action buttons — compact row
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(6.0))
                    .child(paste_btn)
                    .child(open_catalog_btn),
            );

        // Preview card with save/preview actions
        if let Some(preview) = preview_actions {
            import_section = import_section.child(
                div()
                    .pt(px(4.0))
                    .child(preview),
            );
        }

        if let Some(ref status) = self.custom_theme_status {
            import_section = import_section.child(
                div()
                    .px(px(10.0))
                    .py(px(8.0))
                    .rounded(px(6.0))
                    .bg(if status.starts_with("Error") {
                        theme.danger.opacity(0.08)
                    } else {
                        theme.success.opacity(0.08)
                    })
                    .text_size(px(11.0))
                    .line_height(px(17.0))
                    .text_color(if status.starts_with("Error") {
                        theme.danger
                    } else {
                        theme.success
                    })
                    .child(status.clone()),
            );
        }

        let mut content = section_content("Appearance", "Customize the look and feel.", theme);

        // ── Built-in themes ──
        let mut theme_card_inner = div()
            .px(px(16.0))
            .py(px(12.0))
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .mb(px(12.0))
                    .child(
                        div()
                            .text_sm()
                            .font_weight(FontWeight::MEDIUM)
                            .child("Terminal Theme"),
                    )
                    .child(
                        div()
                            .text_size(px(10.0))
                            .text_color(theme.muted_foreground.opacity(0.5))
                            .child(format!("{total_count} themes")),
                    ),
            )
            .child(
                div()
                    .text_size(px(10.5))
                    .text_color(theme.muted_foreground.opacity(0.4))
                    .mb(px(10.0))
                    .child("Community themes from ghostty-style.vercel.app"),
            )
            .child(builtin_grid);

        // User-installed themes
        if let Some(user_grid) = user_grid {
            theme_card_inner = theme_card_inner.child(
                div()
                    .mt(px(16.0))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(6.0))
                            .mb(px(10.0))
                            .child(
                                svg()
                                    .path("phosphor/folder.svg")
                                    .size(px(12.0))
                                    .text_color(theme.muted_foreground.opacity(0.5)),
                            )
                            .child(
                                div()
                                    .text_size(px(10.5))
                                    .text_color(theme.muted_foreground.opacity(0.6))
                                    .child("Installed"),
                            ),
                    )
                    .child(user_grid),
            );
        }
        content = content.child(card(theme).child(theme_card_inner));
        content = content.child(card(theme).child(import_section));

        content
    }

    /// Render a grid of theme preview cards.
    fn render_theme_grid(
        &self,
        themes: &[&con_terminal::TerminalTheme],
        current_theme: &str,
        cx: &mut Context<Self>,
    ) -> Div {
        let mut grid = div().flex().flex_wrap().gap(px(10.0));
        for term_theme in themes.iter() {
            let is_sel = term_theme.name.as_str() == current_theme;
            grid = grid.child(self.render_single_theme_card(term_theme, is_sel, cx));
        }
        grid
    }

    /// Render a single theme preview card.
    fn render_single_theme_card(
        &self,
        term_theme: &con_terminal::TerminalTheme,
        is_sel: bool,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let theme = cx.theme();
        let name = term_theme.name.clone();
        let theme_name = name.clone();
        let bg = term_theme.background;
        let fg = term_theme.foreground;
        let bg_gpui = gpui::rgb(bg.to_u32());
        let fg_gpui = gpui::rgb(fg.to_u32());
        let green = gpui::rgb(term_theme.ansi[2].to_u32());
        let cyan = gpui::rgb(term_theme.ansi[6].to_u32());
        let blue = gpui::rgb(term_theme.ansi[4].to_u32());
        let yellow = gpui::rgb(term_theme.ansi[3].to_u32());
        let red = gpui::rgb(term_theme.ansi[1].to_u32());
        let magenta = gpui::rgb(term_theme.ansi[5].to_u32());

        let terminal_preview = div()
            .flex()
            .flex_col()
            .bg(bg_gpui)
            .rounded_t(px(8.0))
            .px(px(8.0))
            .pt(px(6.0))
            .pb(px(6.0))
            .gap(px(1.0))
            .font_family("Ioskeley Mono")
            .text_size(px(8.0))
            .line_height(px(12.0))
            .child(
                div()
                    .flex()
                    .gap(px(3.0))
                    .pb(px(4.0))
                    .child(div().size(px(5.0)).rounded_full().bg(red))
                    .child(div().size(px(5.0)).rounded_full().bg(yellow))
                    .child(div().size(px(5.0)).rounded_full().bg(green)),
            )
            .child(
                div()
                    .flex()
                    .gap(px(3.0))
                    .child(div().text_color(green).child("$"))
                    .child(div().text_color(cyan).child("git"))
                    .child(div().text_color(fg_gpui).child("log --oneline")),
            )
            .child(
                div()
                    .flex()
                    .gap(px(3.0))
                    .child(div().text_color(yellow).child("a1b2c3d"))
                    .child(div().text_color(fg_gpui).child("feat: init")),
            )
            .child(
                div()
                    .flex()
                    .gap(px(3.0))
                    .child(div().text_color(yellow).child("e4f5g6h"))
                    .child(div().text_color(fg_gpui).child("fix: theme")),
            )
            .child(
                div()
                    .flex()
                    .gap(px(3.0))
                    .child(div().text_color(green).child("$"))
                    .child(div().text_color(blue).child("ls"))
                    .child(div().text_color(fg_gpui).child("src/")),
            )
            .child(
                div()
                    .flex()
                    .gap(px(4.0))
                    .child(div().text_color(blue).child("lib/"))
                    .child(div().text_color(magenta).child("main.rs"))
                    .child(div().text_color(fg_gpui).child("README")),
            );

        let mut palette_strip = div().flex().h(px(4.0));
        for idx in 0..16 {
            let c = term_theme.ansi[idx];
            palette_strip = palette_strip.child(div().flex_1().h_full().bg(gpui::rgb(c.to_u32())));
        }

        let display_name = display_theme_name(&name);

        div()
            .id(SharedString::from(format!("term-theme-{name}")))
            .cursor_pointer()
            .w(px(150.0))
            .flex()
            .flex_col()
            .rounded(px(10.0))
            .overflow_hidden()
            .bg(if is_sel {
                theme.primary.opacity(0.10)
            } else {
                theme.muted.opacity(0.04)
            })
            .hover(|s| s.bg(theme.primary.opacity(0.06)))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, _, _, cx| {
                    this.config.terminal.theme = theme_name.clone();
                    cx.emit(ThemePreview(theme_name.clone()));
                    cx.notify();
                }),
            )
            .child(terminal_preview)
            .child(palette_strip)
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_center()
                    .gap(px(4.0))
                    .h(px(26.0))
                    .text_size(px(10.5))
                    .font_weight(if is_sel {
                        FontWeight::SEMIBOLD
                    } else {
                        FontWeight::MEDIUM
                    })
                    .text_color(if is_sel {
                        theme.primary
                    } else {
                        theme.muted_foreground
                    })
                    .children(if is_sel {
                        Some(
                            svg()
                                .path("phosphor/check.svg")
                                .size(px(10.0))
                                .text_color(theme.primary),
                        )
                    } else {
                        None
                    })
                    .child(display_name),
            )
    }

    fn render_ai(&mut self, cx: &mut Context<Self>) -> Div {
        let theme = cx.theme();
        let model_input = self.model_input.clone();
        let api_key_input = self.api_key_input.clone();
        let base_url_input = self.base_url_input.clone();
        let max_tokens_input = self.max_tokens_input.clone();
        let max_turns_input = self.max_turns_input.clone();
        let temperature_input = self.temperature_input.clone();
        let suggestion_model_input = self.suggestion_model_input.clone();
        let models = self.registry.models_for(&self.selected_provider);
        let model_select = self.model_select.clone();

        let mut provider_list = div().flex().flex_col();
        for provider in ALL_PROVIDERS.iter() {
            let is_selected = *provider == self.selected_provider;
            let label = provider_label(provider);
            let provider_clone = provider.clone();

            provider_list = provider_list.child(
                div()
                    .id(SharedString::from(format!("prov-{label}")))
                    .h(px(32.0))
                    .px(px(10.0))
                    .flex()
                    .items_center()
                    .gap(px(8.0))
                    .rounded(px(7.0))
                    .cursor_pointer()
                    .bg(if is_selected {
                        theme.primary.opacity(0.08)
                    } else {
                        theme.transparent
                    })
                    .hover(|s| {
                        if is_selected {
                            s
                        } else {
                            s.bg(theme.muted.opacity(0.06))
                        }
                    })
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, _, window, cx| {
                            this.select_provider(provider_clone.clone(), window, cx);
                        }),
                    )
                    .child(
                        div()
                            .w(px(2.0))
                            .h(px(14.0))
                            .rounded(px(1.0))
                            .bg(if is_selected {
                                theme.primary
                            } else {
                                theme.transparent
                            }),
                    )
                    .child(
                        div()
                            .flex_1()
                            .text_size(px(12.0))
                            .font_weight(if is_selected {
                                FontWeight::MEDIUM
                            } else {
                                FontWeight::NORMAL
                            })
                            .text_color(if is_selected {
                                theme.foreground
                            } else {
                                theme.muted_foreground
                            })
                            .child(label),
                    ),
            );
        }

        let has_key = !self.api_key_input.read(cx).value().is_empty();

        // ── Model card — Select dropdown for known providers, text input for custom ──
        let model_card_content = card(theme).child(
            div()
                .px(px(14.0))
                .py(px(12.0))
                .flex()
                .flex_col()
                .gap(px(8.0))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .justify_between()
                        .child(
                            div()
                                .text_sm()
                                .font_weight(FontWeight::MEDIUM)
                                .child("Model"),
                        )
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap(px(6.0))
                                .child(
                                    div()
                                        .size(px(6.0))
                                        .rounded_full()
                                        .bg(if has_key {
                                            theme.success
                                        } else {
                                            theme.muted_foreground.opacity(0.2)
                                        }),
                                )
                                .child(
                                    div()
                                        .text_size(px(10.0))
                                        .text_color(theme.muted_foreground.opacity(0.45))
                                        .child(if has_key { "Ready" } else { "No key" }),
                                ),
                        ),
                )
                .child(if models.is_empty() {
                    // Custom providers — free-form text input
                    div().child(Input::new(&model_input))
                } else {
                    // Known providers — searchable Select dropdown
                    div().child(
                        Select::new(&model_select)
                            .placeholder("Select a model…")
                            .small(),
                    )
                }),
        );

        let right_col = div()
            .flex()
            .flex_col()
            .flex_1()
            .gap(px(12.0))
            .child(model_card_content)
            .child(
                card(theme).child(
                    div()
                        .px(px(14.0))
                        .py(px(12.0))
                        .flex()
                        .flex_col()
                        .gap(px(12.0))
                        .child(
                            div()
                                .text_sm()
                                .font_weight(FontWeight::MEDIUM)
                                .child("Connection"),
                        )
                        .child(stacked_input_field(
                            "API Key",
                            "Paste a key or an env var name like ANTHROPIC_API_KEY",
                            &api_key_input,
                            theme,
                        ))
                        .child(stacked_input_field(
                            "Base URL",
                            "Leave blank for the default endpoint",
                            &base_url_input,
                            theme,
                        )),
                ),
            )
            .child(
                card(theme).child(
                    div()
                        .px(px(14.0))
                        .py(px(12.0))
                        .flex()
                        .flex_col()
                        .gap(px(12.0))
                        .child(
                            div()
                                .text_sm()
                                .font_weight(FontWeight::MEDIUM)
                                .child("Tuning"),
                        )
                        .child(stacked_input_field(
                            "Max Tokens",
                            "Per-provider ceiling for generated tokens",
                            &max_tokens_input,
                            theme,
                        ))
                        .child(stacked_input_field(
                            "Max Turns",
                            "How many tool-use turns before the agent must stop",
                            &max_turns_input,
                            theme,
                        ))
                        .child(stacked_input_field(
                            "Temperature",
                            "Blank for default — lower is steadier, higher is looser",
                            &temperature_input,
                            theme,
                        ))
                        .child(stacked_input_field(
                            "Suggestion Model",
                            "Optional lightweight model for shell suggestions",
                            &suggestion_model_input,
                            theme,
                        )),
                ),
            );

        let provider_column = div()
            .flex()
            .flex_col()
            .gap(px(6.0))
            .w(px(180.0))
            .flex_shrink_0()
            .child(
                card(theme).child(
                    div()
                        .px(px(4.0))
                        .py(px(4.0))
                        .child(provider_list),
                ),
            );

        let models_layout = div()
            .flex()
            .flex_1()
            .gap(px(16.0))
            .child(provider_column)
            .child(right_col);

        section_content(
            "Models",
            "Choose a provider, lock in a model, and tune the endpoint details.",
            theme,
        )
        .child(models_layout)
    }

    fn render_keys(&mut self, cx: &mut Context<Self>) -> Div {
        let recording = self.recording_key.clone();

        // Editable keybinding definitions: (label, field_name)
        let general_keys: &[(&str, &str)] = &[
            ("New Tab", "new_tab"),
            ("Close Tab", "close_tab"),
            ("Settings", "settings"),
            ("Command Palette", "command_palette"),
            ("Toggle Agent", "toggle_agent"),
            ("Toggle Input Bar", "toggle_input_bar"),
            ("Focus Input", "focus_input"),
            ("Quit", "quit"),
        ];

        let pane_keys: &[(&str, &str)] =
            &[("Split Right", "split_right"), ("Split Down", "split_down")];

        let build_card = |keys: &[(&str, &str)],
                          recording: &Option<String>,
                          this: &mut Self,
                          cx: &mut Context<Self>|
         -> Div {
            let theme = cx.theme();
            let mut c = card(theme);
            for (i, (label, field)) in keys.iter().enumerate() {
                if i > 0 {
                    c = c.child(row_separator(theme));
                }
                let value = this.binding_value(field).to_string();
                let is_recording = recording.as_deref() == Some(*field);
                let display = if is_recording {
                    "Press shortcut...".to_string()
                } else {
                    format_keybinding_display(&value)
                };
                let field_str = field.to_string();
                c = c.child(
                    div()
                        .id(SharedString::from(format!("key-{field}")))
                        .flex()
                        .items_center()
                        .justify_between()
                        .px(px(16.0))
                        .h(px(36.0))
                        .child(div().text_sm().child(label.to_string()))
                        .child(
                            div()
                                .id(SharedString::from(format!("key-badge-{field}")))
                                .h(px(24.0))
                                .px(px(9.0))
                                .flex()
                                .items_center()
                                .rounded(px(5.0))
                                .cursor_pointer()
                                .bg(if is_recording {
                                    theme.primary.opacity(0.12)
                                } else {
                                    theme.muted.opacity(0.15)
                                })
                                .text_size(px(11.5))
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(if is_recording {
                                    theme.primary
                                } else {
                                    theme.muted_foreground
                                })
                                .hover(|s| {
                                    s.bg(theme.primary.opacity(0.10)).text_color(theme.primary)
                                })
                                .on_mouse_down(
                                    MouseButton::Left,
                                    cx.listener(move |this, _, _, cx| {
                                        this.recording_key = Some(field_str.clone());
                                        cx.notify();
                                    }),
                                )
                                .child(display),
                        ),
                );
            }
            c
        };

        let general_card = build_card(general_keys, &recording, self, cx);
        let pane_card_keys = pane_keys;
        let pane_card = build_card(pane_card_keys, &recording, self, cx);

        let theme = cx.theme();
        section_content(
            "Keyboard Shortcuts",
            "Click a shortcut to record a new key combination.",
            theme,
        )
        .child(
            div()
                .flex()
                .flex_col()
                .gap(px(8.0))
                .child(group_label("General", &theme))
                .child(general_card),
        )
        .child(
            div()
                .flex()
                .flex_col()
                .gap(px(8.0))
                .child(group_label("Panes", &theme))
                .child(pane_card)
                .child(card(theme).child(key_row("Close Pane", "⌃D", theme))),
        )
        .child(
            div()
                .flex()
                .flex_col()
                .gap(px(8.0))
                .child(group_label("Terminal", &theme))
                .child(
                    card(theme)
                        .child(key_row("Copy", "⌘C", theme))
                        .child(row_separator(theme))
                        .child(key_row("Paste", "⌘V", theme))
                        .child(row_separator(theme))
                        .child(key_row("Select All", "⌘A", theme)),
                ),
        )
    }
}

impl EventEmitter<SaveSettings> for SettingsPanel {}
impl EventEmitter<ThemePreview> for SettingsPanel {}

impl Focusable for SettingsPanel {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for SettingsPanel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if !self.visible {
            return div().id("settings-overlay");
        }

        let active = self.active_section;

        // Render content first (AI needs &mut self)
        let content = match active {
            SettingsSection::General => self.render_general(cx),
            SettingsSection::Appearance => self.render_appearance(cx),
            SettingsSection::Models => self.render_ai(cx),
            SettingsSection::Keys => self.render_keys(cx),
        };

        let theme = cx.theme();
        let viewport = window.viewport_size();
        let viewport_w = viewport.width.as_f32();
        let viewport_h = viewport.height.as_f32();
        let compact = viewport_w < 980.0;
        let narrow = viewport_w < 840.0;
        let sidebar_w = if narrow {
            px(48.0)
        } else if compact {
            px(144.0)
        } else {
            px(160.0)
        };
        let content_pad = if narrow {
            px(14.0)
        } else if compact {
            px(18.0)
        } else {
            px(24.0)
        };
        // Uniform width for all sections — prevents position jumping when switching tabs
        let card_width = px(((viewport_w * 0.76).clamp(680.0, 980.0)).min(viewport_w - 32.0));
        let card_height = {
            let target = match active {
                SettingsSection::Appearance => (viewport_h * 0.82).clamp(440.0, 780.0),
                _ => (viewport_h * 0.76).clamp(420.0, 720.0),
            };
            px(target.min(viewport_h - 32.0))
        };

        // Sidebar
        let mut sidebar = div()
            .flex()
            .flex_col()
            .w(sidebar_w)
            .pt(px(8.0))
            .pb(px(12.0))
            .px(if narrow { px(4.0) } else { px(8.0) })
            .gap(px(2.0))
            .flex_shrink_0();

        for section in ALL_SECTIONS {
            let is_active = *section == active;
            let section_val = *section;
            let mut nav_item = div()
                .id(SharedString::from(format!("nav-{}", section.label())))
                .flex()
                .items_center()
                .h(px(32.0))
                .rounded(px(8.0))
                .cursor_pointer()
                .bg(if is_active {
                    theme.muted.opacity(0.15)
                } else {
                    theme.transparent
                })
                .text_color(if is_active {
                    theme.foreground
                } else {
                    theme.muted_foreground
                })
                .font_weight(if is_active {
                    FontWeight::MEDIUM
                } else {
                    FontWeight::NORMAL
                })
                .hover(|s| {
                    if is_active {
                        s
                    } else {
                        s.bg(theme.muted.opacity(0.08))
                    }
                })
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, _, _, cx| {
                        this.active_section = section_val;
                        cx.notify();
                    }),
                );

            if narrow {
                // Icon-only mode: centered icon, no label
                nav_item = nav_item
                    .justify_center()
                    .size(px(36.0))
                    .mx_auto()
                    .child(
                        svg()
                            .path(section.icon())
                            .size(px(16.0))
                            .text_color(if is_active {
                                theme.foreground
                            } else {
                                theme.muted_foreground
                            }),
                    );
            } else {
                nav_item = nav_item
                    .gap(px(8.0))
                    .px(px(10.0))
                    .text_size(px(13.0))
                    .child(
                        svg()
                            .path(section.icon())
                            .size(px(15.0))
                            .text_color(if is_active {
                                theme.foreground
                            } else {
                                theme.muted_foreground
                            }),
                    )
                    .child(section.label());
            }

            sidebar = sidebar.child(nav_item);
        }

        let content_scroll = div()
            .id("settings-content-scroll")
            .flex_1()
            .overflow_y_scroll()
            .p(content_pad)
            .child(content);

        let backdrop = div()
            .id("settings-backdrop")
            .occlude()
            .absolute()
            .size_full()
            .bg(theme.background.opacity(0.6))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, window, cx| {
                    this.save(window, cx);
                }),
            );

        // Card — centered with flex centering
        let card = div()
            .id("settings-card")
            .absolute()
            .inset_0()
            .flex()
            .items_center()
            .justify_center()
            .child(
                div()
                    .w(card_width)
                    .h(card_height)
                    .rounded(px(12.0))
                    .bg(theme.title_bar)
                    .overflow_hidden()
                    .flex()
                    .flex_col()
                    .occlude()
                    .track_focus(&self.focus_handle)
                    .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                        // If recording a keybinding, capture the keystroke
                        if this.recording_key.is_some() {
                            this.record_keystroke(&event.keystroke, cx);
                            return;
                        }
                        match event.keystroke.key.as_str() {
                            "escape" => {
                                this.save(window, cx);
                            }
                            "enter" if event.keystroke.modifiers.platform => {
                                this.save(window, cx);
                            }
                            _ => {}
                        }
                    }))
                    // Header
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .flex_shrink_0()
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .justify_between()
                                    .h(px(44.0))
                                    .px(px(20.0))
                                    .child(
                                        div()
                                            .text_size(px(13.0))
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .text_color(theme.foreground)
                                            .child("Settings"),
                                    )
                                    .child(
                                        div()
                                            .flex()
                                            .items_center()
                                            .gap(px(8.0))
                                            // Open config file link
                                            .child(
                                                div()
                                                    .id("open-config-file")
                                                    .flex()
                                                    .items_center()
                                                    .gap(px(3.0))
                                                    .cursor_pointer()
                                                    .text_size(px(10.5))
                                                    .text_color(theme.muted_foreground.opacity(0.4))
                                                    .hover(|s| s.text_color(theme.muted_foreground.opacity(0.7)))
                                                    .on_mouse_down(
                                                        MouseButton::Left,
                                                        cx.listener(|_, _, _, cx| {
                                                            let path = Config::config_path();
                                                            // Ensure the file exists so the editor has something to open
                                                            if !path.exists() {
                                                                if let Some(parent) = path.parent() {
                                                                    let _ = std::fs::create_dir_all(parent);
                                                                }
                                                                let _ = std::fs::write(&path, "");
                                                            }
                                                            cx.open_url(&format!("file://{}", path.display()));
                                                        }),
                                                    )
                                                    .child(
                                                        svg()
                                                            .path("phosphor/file-text.svg")
                                                            .size(px(12.0))
                                                            .text_color(theme.muted_foreground.opacity(0.4)),
                                                    )
                                                    .child("config.toml"),
                                            )
                                            // Close button
                                            .child(
                                                div()
                                                    .id("close-settings")
                                                    .flex()
                                                    .items_center()
                                                    .justify_center()
                                                    .size(px(22.0))
                                                    .rounded(px(6.0))
                                                    .cursor_pointer()
                                                    .hover(|s| s.bg(theme.muted.opacity(0.10)))
                                                    .on_mouse_down(
                                                        MouseButton::Left,
                                                        cx.listener(|this, _, window, cx| {
                                                            this.save(window, cx);
                                                            this.toggle(window, cx);
                                                        }),
                                                    )
                                                    .child(
                                                        svg()
                                                            .path("phosphor/x.svg")
                                                            .size(px(12.0))
                                                            .text_color(theme.muted_foreground.opacity(0.5)),
                                                    ),
                                            ),
                                    ),
                            )
                            .child(
                                div()
                                    .h(px(1.0))
                                    .bg(theme.muted.opacity(0.10)),
                            ),
                    )
                    // Error banner
                    .children(self.save_error.as_ref().map(|err| {
                        div()
                            .px_4()
                            .py_2()
                            .mx_4()
                            .mt_2()
                            .rounded_md()
                            .bg(theme.danger)
                            .text_color(theme.danger_foreground)
                            .text_xs()
                            .child(format!("Save failed: {}", err))
                    }))
                    // Body
                    .child(
                        div()
                            .flex()
                            .flex_1()
                            .min_h_0()
                            .child(sidebar)
                            .child(content_scroll),
                    ),
            );

        div()
            .id("settings-overlay")
            .absolute()
            .size_full()
            .font_family(".SystemUIFont")
            .child(backdrop)
            .child(card)
    }
}

// ── Reusable building blocks ──────────────────────────────────────

fn section_content(title: &str, subtitle: &str, theme: &gpui_component::Theme) -> Div {
    div().flex().flex_col().gap(px(18.0)).child(
        div()
            .flex()
            .flex_col()
            .gap(px(4.0))
            .child(
                div()
                    .text_size(px(18.0))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(title.to_string()),
            )
            .child(
                div()
                    .text_size(px(11.5))
                    .line_height(px(18.0))
                    .text_color(theme.muted_foreground.opacity(0.72))
                    .child(subtitle.to_string()),
            ),
    )
}

fn group_label(text: &str, theme: &gpui_component::Theme) -> Div {
    div()
        .text_size(px(10.5))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(theme.muted_foreground.opacity(0.52))
        .px(px(2.0))
        .pb(px(2.0))
        .child(text.to_string())
}

fn card(theme: &gpui_component::Theme) -> Div {
    div()
        .flex()
        .flex_col()
        .rounded(px(10.0))
        .overflow_hidden()
        .bg(theme.background.opacity(0.74))
}

fn row_separator(_theme: &gpui_component::Theme) -> Div {
    div().h(px(6.0))
}

fn row_field(label: &str, input: &Entity<InputState>) -> Div {
    div()
        .flex()
        .items_center()
        .justify_between()
        .gap(px(12.0))
        .px(px(16.0))
        .h(px(44.0))
        .child(div().text_sm().flex_shrink_0().child(label.to_string()))
        .child(div().flex_1().min_w(px(160.0)).child(Input::new(input)))
}

fn stacked_input_field(
    label: &str,
    hint: &str,
    input: &Entity<InputState>,
    theme: &gpui_component::Theme,
) -> Div {
    div()
        .flex()
        .flex_col()
        .gap(px(6.0))
        .child(
            div()
                .flex()
                .flex_col()
                .gap(px(2.0))
                .child(
                    div()
                        .text_size(px(11.5))
                        .font_weight(FontWeight::MEDIUM)
                        .child(label.to_string()),
                )
                .child(
                    div()
                        .text_size(px(10.5))
                        .line_height(px(16.0))
                        .text_color(theme.muted_foreground.opacity(0.65))
                        .child(hint.to_string()),
                ),
        )
        .child(Input::new(input))
}


/// Convert a GPUI Keystroke to the binding format string (e.g. "cmd-shift-d").
fn keystroke_to_binding(ks: &gpui::Keystroke) -> String {
    let mut parts = Vec::new();
    if ks.modifiers.platform {
        parts.push("cmd");
    }
    if ks.modifiers.control {
        parts.push("ctrl");
    }
    if ks.modifiers.alt {
        parts.push("alt");
    }
    if ks.modifiers.shift {
        parts.push("shift");
    }
    parts.push(&ks.key);
    parts.join("-")
}

/// Convert a GPUI binding string (e.g. "cmd-shift-d") to display format ("⇧⌘D").
fn format_keybinding_display(binding: &str) -> String {
    let parts: Vec<&str> = binding.split('-').collect();
    if parts.is_empty() {
        return binding.to_string();
    }
    let mut display = String::new();
    // Modifier order for display: ⌃⌥⇧⌘ (standard macOS ordering)
    let modifiers = &parts[..parts.len() - 1];
    if modifiers.contains(&"ctrl") {
        display.push('⌃');
    }
    if modifiers.contains(&"alt") {
        display.push('⌥');
    }
    if modifiers.contains(&"shift") {
        display.push('⇧');
    }
    if modifiers.contains(&"cmd") {
        display.push('⌘');
    }
    if let Some(key) = parts.last() {
        // Special key display names
        let display_key = match *key {
            "backspace" => "⌫",
            "delete" => "⌦",
            "enter" => "↵",
            "tab" => "⇥",
            "space" => "Space",
            "up" => "↑",
            "down" => "↓",
            "left" => "←",
            "right" => "→",
            _ => "",
        };
        if display_key.is_empty() {
            display.push_str(&key.to_uppercase());
        } else {
            display.push_str(display_key);
        }
    }
    display
}

fn key_row(action: &str, shortcut: &str, theme: &gpui_component::Theme) -> Div {
    div()
        .flex()
        .items_center()
        .justify_between()
        .px(px(16.0))
        .h(px(36.0))
        .child(div().text_sm().child(action.to_string()))
        .child(
            div()
                .h(px(22.0))
                .px(px(7.0))
                .flex()
                .items_center()
                .rounded(px(4.0))
                .bg(theme.muted.opacity(0.15))
                .text_size(px(11.0))
                .text_color(theme.muted_foreground)
                .child(shortcut.to_string()),
        )
}

fn provider_label(provider: &ProviderKind) -> &'static str {
    match provider {
        ProviderKind::Anthropic => "Anthropic",
        ProviderKind::OpenAI => "OpenAI",
        ProviderKind::OpenAICompatible => "OpenAI Compatible",
        ProviderKind::DeepSeek => "DeepSeek",
        ProviderKind::Groq => "Groq",
        ProviderKind::Cohere => "Cohere",
        ProviderKind::Gemini => "Gemini",
        ProviderKind::Ollama => "Ollama",
        ProviderKind::OpenRouter => "OpenRouter",
        ProviderKind::Perplexity => "Perplexity",
        ProviderKind::Mistral => "Mistral",
        ProviderKind::Together => "Together",
        ProviderKind::XAI => "xAI",
    }
}

fn display_theme_name(name: &str) -> String {
    match name {
        "flexoki-dark" => "Flexoki Dark".into(),
        "flexoki-light" => "Flexoki Light".into(),
        "catppuccin-mocha" => "Catppuccin".into(),
        "tokyonight" => "Tokyo Night".into(),
        "rose-pine" => "Rose Pine".into(),
        "gruvbox-dark" => "Gruvbox Dark".into(),
        "solarized-dark" => "Solarized Dark".into(),
        "solarized-light" => "Solarized Light".into(),
        "one-half-dark" => "One Half Dark".into(),
        "kanagawa-wave" => "Kanagawa Wave".into(),
        "everforest-dark" => "Everforest Dark".into(),
        "everforest-light" => "Everforest Light".into(),
        "claude-code-light" => "Claude Code Light".into(),
        // User themes: convert kebab-case to Title Case
        other => other
            .split('-')
            .map(|word| {
                let mut c = word.chars();
                match c.next() {
                    None => String::new(),
                    Some(f) => f.to_uppercase().to_string() + c.as_str(),
                }
            })
            .collect::<Vec<_>>()
            .join(" "),
    }
}
