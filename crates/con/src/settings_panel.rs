use con_agent::{AgentConfig, ProviderConfig, ProviderKind, SuggestionModelConfig};
use con_core::Config;
use gpui::*;

use gpui_component::input::InputState;
use gpui_component::{ActiveTheme, input::Input};

actions!(
    settings,
    [ToggleSettings, SaveSettings, DismissSettings]
);

/// Emitted when the user selects a different terminal theme for live preview.
pub struct ThemePreview(pub String);

#[derive(Debug, Clone, Copy, PartialEq)]
enum SettingsSection {
    General,
    Appearance,
    AI,
    Keys,
}

impl SettingsSection {
    fn label(&self) -> &'static str {
        match self {
            Self::General => "General",
            Self::Appearance => "Appearance",
            Self::AI => "AI",
            Self::Keys => "Keys",
        }
    }

    fn icon(&self) -> &'static str {
        match self {
            Self::General => "phosphor/sliders.svg",
            Self::Appearance => "phosphor/sun.svg",
            Self::AI => "phosphor/robot.svg",
            Self::Keys => "phosphor/keyboard.svg",
        }
    }
}

const ALL_SECTIONS: &[SettingsSection] = &[
    SettingsSection::General,
    SettingsSection::Appearance,
    SettingsSection::AI,
    SettingsSection::Keys,
];

pub struct SettingsPanel {
    visible: bool,
    config: Config,
    focus_handle: FocusHandle,
    active_section: SettingsSection,

    selected_provider: ProviderKind,
    model_input: Entity<InputState>,
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
        let api_key_input = cx.new(|cx| {
            let mut s = InputState::new(window, cx);
            s.set_placeholder("sk-... or env var like ANTHROPIC_API_KEY", window, cx);
            let val = pc.api_key.clone().or_else(|| pc.api_key_env.clone()).unwrap_or_default();
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
            s.set_value(&pc.max_tokens.map(|t| t.to_string()).unwrap_or_default(), window, cx);
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

        Self {
            visible: false,
            config: config.clone(),
            focus_handle: cx.focus_handle(),
            active_section: SettingsSection::General,
            selected_provider: config.agent.provider.clone(),
            model_input,
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
        }
    }

    pub fn toggle(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.visible = !self.visible;
        if self.visible {
            let agent = &self.config.agent;
            self.selected_provider = agent.provider.clone();
            let pc = agent.providers.get_or_default(&self.selected_provider);
            self.load_provider_inputs(&pc, window, cx);
            self.max_turns_input.update(cx, |s, cx| s.set_value(&agent.max_turns.to_string(), window, cx));
            self.temperature_input.update(cx, |s, cx| s.set_value(&agent.temperature.map(|t| t.to_string()).unwrap_or_default(), window, cx));
            self.suggestion_model_input.update(cx, |s, cx| s.set_value(&agent.suggestion_model.model.clone().unwrap_or_default(), window, cx));
            self.auto_approve = agent.auto_approve_tools;
            self.font_size_input.update(cx, |s, cx| s.set_value(&self.config.terminal.font_size.to_string(), window, cx));
            self.scrollback_input.update(cx, |s, cx| s.set_value(&self.config.terminal.scrollback_lines.to_string(), window, cx));
            self.focus_handle.focus(window, cx);
        }
        cx.notify();
    }

    /// Load a provider's config into the per-provider input fields.
    fn load_provider_inputs(&self, pc: &ProviderConfig, window: &mut Window, cx: &mut Context<Self>) {
        self.model_input.update(cx, |s, cx| s.set_value(&pc.model.clone().unwrap_or_default(), window, cx));
        let key_val = pc.api_key.clone().or_else(|| pc.api_key_env.clone()).unwrap_or_default();
        self.api_key_input.update(cx, |s, cx| s.set_value(&key_val, window, cx));
        self.base_url_input.update(cx, |s, cx| s.set_value(&pc.base_url.clone().unwrap_or_default(), window, cx));
        self.max_tokens_input.update(cx, |s, cx| s.set_value(&pc.max_tokens.map(|t| t.to_string()).unwrap_or_default(), window, cx));
    }

    /// Read current per-provider input fields into a ProviderConfig.
    fn read_provider_inputs(&self, cx: &App) -> ProviderConfig {
        let key_text = self.api_key_input.read(cx).value().to_string();
        let is_env_var = !key_text.is_empty()
            && key_text.chars().all(|c| c.is_ascii_uppercase() || c == '_' || c.is_ascii_digit());

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
            model: if model_text.is_empty() { None } else { Some(model_text) },
            api_key,
            api_key_env,
            base_url: if base_url_text.is_empty() { None } else { Some(base_url_text) },
            max_tokens: if max_tokens_text.is_empty() { None } else { max_tokens_text.parse().ok() },
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
        self.config.agent.temperature = if temperature_text.is_empty() { None } else { temperature_text.parse().ok() };
        self.config.agent.auto_approve_tools = self.auto_approve;
        self.config.agent.suggestion_model = SuggestionModelConfig {
            provider: None,
            model: if suggestion_model_text.is_empty() { None } else { Some(suggestion_model_text) },
        };
        self.config.terminal.font_size = font_size_text.parse().unwrap_or(14.0);
        self.config.terminal.scrollback_lines = scrollback_text.parse().unwrap_or(10_000);

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

    pub fn agent_config(&self) -> &AgentConfig { &self.config.agent }
    pub fn terminal_config(&self) -> &con_core::config::TerminalConfig { &self.config.terminal }

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
        self.config.agent.providers.set(&self.selected_provider, current_pc);

        // Switch to new provider
        self.selected_provider = provider.clone();

        // Load new provider's saved config (or defaults)
        let pc = self.config.agent.providers.get_or_default(&provider);
        self.load_provider_inputs(&pc, window, cx);
        cx.notify();
    }

    // ── Section content ──────────────────────────────────────────

    fn render_general(&mut self, cx: &mut Context<Self>) -> Div {
        let theme = cx.theme();
        let font_size_input = self.font_size_input.clone();
        let scrollback_input = self.scrollback_input.clone();

        // Auto-approve toggle
        let is_on = self.auto_approve;
        let toggle = div()
            .id("auto-approve-toggle")
            .w(px(40.0))
            .h(px(22.0))
            .rounded(px(11.0))
            .cursor_pointer()
            .bg(if is_on { theme.primary } else { theme.muted.opacity(0.25) })
            .child(
                div()
                    .size(px(18.0))
                    .rounded_full()
                    .bg(gpui::white())
                    .mt(px(2.0))
                    .ml(if is_on { px(20.0) } else { px(2.0) }),
            )
            .on_mouse_down(MouseButton::Left, cx.listener(|this, _, _, cx| {
                this.auto_approve = !this.auto_approve;
                cx.notify();
            }));

        let theme = cx.theme();
        section_content("General", "Terminal and editor settings.", theme)
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
                    .child(group_label("AGENT"))
                    .child(
                        card(theme)
                            .child(
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
                                                    .child("Allow agent to run tools without confirmation"),
                                            ),
                                    )
                                    .child(toggle),
                            ),
                    ),
            )
    }

    fn render_appearance(&self, cx: &mut Context<Self>) -> Div {
        let theme = cx.theme();
        let current_theme = &self.config.terminal.theme;
        let all_themes = con_terminal::TerminalTheme::all_available();

        section_content("Appearance", "Customize the look and feel.", &theme)
            .child(
                card(&theme)
                    .child(
                        div()
                            .px(px(16.0))
                            .py(px(12.0))
                            .child(
                                div()
                                    .text_sm()
                                    .mb(px(12.0))
                                    .child("Terminal Theme"),
                            )
                            .child({
                                let mut grid = div()
                                    .flex()
                                    .flex_wrap()
                                    .gap(px(10.0));

                                for term_theme in all_themes.iter() {
                                    let name = term_theme.name.as_str();
                                    let is_sel = name == current_theme.as_str();
                                    let theme_name = name.to_string();
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

                                    // Mini terminal preview — ghostty.style inspired
                                    // Simulated terminal with multiple output lines
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
                                        // Title bar dots
                                        .child(
                                            div()
                                                .flex()
                                                .gap(px(3.0))
                                                .pb(px(4.0))
                                                .child(div().size(px(5.0)).rounded_full().bg(red))
                                                .child(div().size(px(5.0)).rounded_full().bg(yellow))
                                                .child(div().size(px(5.0)).rounded_full().bg(green)),
                                        )
                                        // Line 1: prompt + command
                                        .child(
                                            div().flex().gap(px(3.0))
                                                .child(div().text_color(green).child("$"))
                                                .child(div().text_color(cyan).child("git"))
                                                .child(div().text_color(fg_gpui).child("log --oneline")),
                                        )
                                        // Line 2: git output
                                        .child(
                                            div().flex().gap(px(3.0))
                                                .child(div().text_color(yellow).child("a1b2c3d"))
                                                .child(div().text_color(fg_gpui).child("feat: init")),
                                        )
                                        // Line 3: another line
                                        .child(
                                            div().flex().gap(px(3.0))
                                                .child(div().text_color(yellow).child("e4f5g6h"))
                                                .child(div().text_color(fg_gpui).child("fix: theme")),
                                        )
                                        // Line 4: new prompt
                                        .child(
                                            div().flex().gap(px(3.0))
                                                .child(div().text_color(green).child("$"))
                                                .child(div().text_color(blue).child("ls"))
                                                .child(div().text_color(fg_gpui).child("src/")),
                                        )
                                        // Line 5: ls output
                                        .child(
                                            div().flex().gap(px(4.0))
                                                .child(div().text_color(blue).child("lib/"))
                                                .child(div().text_color(magenta).child("main.rs"))
                                                .child(div().text_color(fg_gpui).child("README")),
                                        );

                                    // Palette strip — all 16 colors as continuous blocks
                                    let mut palette_strip = div()
                                        .flex()
                                        .h(px(4.0));

                                    for idx in 0..16 {
                                        let c = term_theme.ansi[idx];
                                        palette_strip = palette_strip.child(
                                            div()
                                                .flex_1()
                                                .h_full()
                                                .bg(gpui::rgb(c.to_u32())),
                                        );
                                    }

                                    grid = grid.child(
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
                                            // Terminal preview area
                                            .child(terminal_preview)
                                            // Color palette strip
                                            .child(palette_strip)
                                            // Label row
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
                                                    .child(display_theme_name(name)),
                                            ),
                                    );
                                }
                                grid
                            }),
                    ),
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

        // Provider list — compact rows
        let mut provider_list = card(theme);
        let provider_count = ALL_PROVIDERS.len();
        for (idx, provider) in ALL_PROVIDERS.iter().enumerate() {
            let is_selected = *provider == self.selected_provider;
            let label = provider_label(provider);
            let provider_clone = provider.clone();

            let indicator = div()
                .size(px(14.0))
                .rounded_full()
                .flex_shrink_0()
                .bg(if is_selected { theme.primary } else { theme.muted.opacity(0.12) })
                .flex()
                .items_center()
                .justify_center()
                .child(
                    div()
                        .size(px(5.0))
                        .rounded_full()
                        .bg(if is_selected { gpui::white() } else { theme.transparent }),
                );

            let row = div()
                .id(SharedString::from(format!("prov-{label}")))
                .h(px(32.0))
                .px(px(12.0))
                .flex()
                .items_center()
                .gap(px(8.0))
                .cursor_pointer()
                .bg(if is_selected { theme.primary.opacity(0.08) } else { theme.transparent })
                .hover(|s| s.bg(theme.muted.opacity(0.1)))
                .on_mouse_down(MouseButton::Left, cx.listener(move |this, _, window, cx| {
                    this.select_provider(provider_clone.clone(), window, cx);
                }))
                .child(indicator)
                .child(
                    div()
                        .text_size(px(12.0))
                        .font_weight(if is_selected { FontWeight::MEDIUM } else { FontWeight::NORMAL })
                        .text_color(theme.foreground)
                        .child(label),
                );

            provider_list = provider_list.child(row);
            if idx + 1 < provider_count {
                provider_list = provider_list.child(row_separator(theme));
            }
        }

        // Model: text input + clickable suggestion chips
        let models = provider_models(&self.selected_provider);
        let current_model = self.model_input.read(cx).value().to_string();

        let mut model_card_content = card(theme)
            .child(
                div()
                    .px(px(12.0))
                    .py(px(10.0))
                    .child(Input::new(&model_input)),
            );

        // Clickable suggestion chips for providers with known models
        if !models.is_empty() {
            let mut chips = div()
                .flex()
                .flex_wrap()
                .gap(px(5.0))
                .px(px(12.0))
                .pb(px(10.0));

            for model in models {
                let model_name = model.to_string();
                let model_clone = model_name.clone();
                let is_active = current_model == model_name;
                chips = chips.child(
                    div()
                        .id(SharedString::from(format!("model-{model_name}")))
                        .px(px(8.0))
                        .h(px(26.0))
                        .flex()
                        .items_center()
                        .rounded(px(5.0))
                        .text_size(px(11.0))
                        .cursor_pointer()
                        .bg(if is_active { theme.primary.opacity(0.15) } else { theme.muted.opacity(0.06) })
                        .text_color(if is_active { theme.foreground } else { theme.muted_foreground })
                        .hover(|s| s.bg(theme.muted.opacity(0.18)).text_color(theme.foreground))
                        .on_mouse_down(MouseButton::Left, cx.listener(move |this, _, window, cx| {
                            this.model_input.update(cx, |s, cx| {
                                s.set_value(&model_clone, window, cx);
                            });
                            cx.notify();
                        }))
                        .child(model_name),
                );
            }
            model_card_content = model_card_content.child(chips);
        };

        let model_section = div()
            .flex()
            .flex_col()
            .gap(px(8.0))
            .child(group_label("MODEL"))
            .child(model_card_content);

        // Right column — model + config + advanced
        let right_col = div()
            .flex()
            .flex_col()
            .flex_1()
            .gap(px(16.0))
            .child(model_section)
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(8.0))
                    .child(group_label("CONFIGURATION"))
                    .child(
                        card(theme)
                            .child(row_field("API Key", &api_key_input))
                            .child(row_separator(theme))
                            .child(row_field("Base URL", &base_url_input)),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(8.0))
                    .child(group_label("ADVANCED"))
                    .child(
                        card(theme)
                            .child(row_field("Max Tokens", &max_tokens_input))
                            .child(row_separator(theme))
                            .child(row_field("Max Turns", &max_turns_input))
                            .child(row_separator(theme))
                            .child(row_field("Temperature", &temperature_input)),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(8.0))
                    .child(group_label("SUGGESTIONS"))
                    .child(
                        card(theme)
                            .child(row_field("Model", &suggestion_model_input)),
                    ),
            );

        // Two-column layout: providers left, config right
        section_content("AI", "Configure your AI provider and model.", theme)
            .child(
                div()
                    .flex()
                    .flex_1()
                    .gap(px(20.0))
                    // Left: provider list
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(8.0))
                            .w(px(200.0))
                            .flex_shrink_0()
                            .child(group_label("PROVIDER"))
                            .child(provider_list),
                    )
                    // Right: model + config + advanced
                    .child(right_col),
            )
    }

    fn render_keys(&self, theme: &gpui_component::Theme) -> Div {
        section_content("Keyboard Shortcuts", "View and customize key bindings.", theme)
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(8.0))
                    .child(group_label("GENERAL"))
                    .child(
                        card(theme)
                            .child(key_row("New Tab", "⌘T", theme))
                            .child(row_separator(theme))
                            .child(key_row("Close Tab", "⌘W", theme))
                            .child(row_separator(theme))
                            .child(key_row("Settings", "⌘,", theme))
                            .child(row_separator(theme))
                            .child(key_row("Command Palette", "⇧⌘P", theme))
                            .child(row_separator(theme))
                            .child(key_row("Toggle Agent", "⌘L", theme)),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(8.0))
                    .child(group_label("PANES"))
                    .child(
                        card(theme)
                            .child(key_row("Split Right", "⌘D", theme))
                            .child(row_separator(theme))
                            .child(key_row("Split Down", "⇧⌘D", theme))
                            .child(row_separator(theme))
                            .child(key_row("Close Pane", "⌃D", theme)),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(8.0))
                    .child(group_label("TERMINAL"))
                    .child(
                        card(theme)
                            .child(key_row("Clear", "⌘K", theme))
                            .child(row_separator(theme))
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
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if !self.visible {
            return div().id("settings-overlay");
        }

        let active = self.active_section;

        // Render content first (AI needs &mut self)
        let content = match active {
            SettingsSection::General => self.render_general(cx),
            SettingsSection::Appearance => {
                self.render_appearance(cx)
            }
            SettingsSection::AI => self.render_ai(cx),
            SettingsSection::Keys => {
                let theme = cx.theme();
                self.render_keys(theme)
            }
        };

        let theme = cx.theme();

        // Sidebar
        let mut sidebar = div()
            .flex()
            .flex_col()
            .w(px(160.0))
            .pt(px(8.0))
            .pb(px(12.0))
            .px(px(8.0))
            .gap(px(1.0))
            .flex_shrink_0();

        for section in ALL_SECTIONS {
            let is_active = *section == active;
            let section_val = *section;
            sidebar = sidebar.child(
                div()
                    .id(SharedString::from(format!("nav-{}", section.label())))
                    .flex()
                    .items_center()
                    .gap(px(8.0))
                    .h(px(30.0))
                    .px(px(10.0))
                    .rounded(px(6.0))
                    .cursor_pointer()
                    .text_size(px(13.0))
                    .bg(if is_active { theme.muted.opacity(0.15) } else { theme.transparent })
                    .text_color(if is_active { theme.foreground } else { theme.muted_foreground })
                    .font_weight(if is_active { FontWeight::MEDIUM } else { FontWeight::NORMAL })
                    .hover(|s| if is_active { s } else { s.bg(theme.muted.opacity(0.08)) })
                    .on_mouse_down(MouseButton::Left, cx.listener(move |this, _, _, cx| {
                        this.active_section = section_val;
                        cx.notify();
                    }))
                    .child(
                        svg()
                            .path(section.icon())
                            .size(px(15.0))
                            .text_color(if is_active { theme.foreground } else { theme.muted_foreground }),
                    )
                    .child(section.label()),
            );
        }

        let content_scroll = div()
            .id("settings-content-scroll")
            .flex_1()
            .overflow_y_scroll()
            .p(px(24.0))
            .child(content);

        let backdrop = div()
            .id("settings-backdrop")
            .occlude()
            .absolute()
            .size_full()
            .bg(rgba(0x00000055))
            .on_mouse_down(MouseButton::Left, cx.listener(|this, _, _, cx| {
                this.visible = false;
                cx.notify();
            }));

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
                    .w(relative(0.70))
                    .min_w(px(600.0))
                    .max_w(px(960.0))
                    .h(relative(0.75))
                    .min_h(px(400.0))
                    .max_h(px(720.0))
                    .rounded(px(12.0))
                    .bg(theme.title_bar)
                    .overflow_hidden()
                    .flex()
                    .flex_col()
                    .occlude()
                    .track_focus(&self.focus_handle)
                    .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                        match event.keystroke.key.as_str() {
                            "escape" => {
                                this.visible = false;
                                cx.notify();
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
                            .items_center()
                            .justify_between()
                            .h(px(48.0))
                            .px(px(20.0))
                            .flex_shrink_0()
                            .child(
                                div()
                                    .text_size(px(13.0))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(theme.foreground)
                                    .child("Settings"),
                            )
                            .child(
                                div()
                                    .text_size(px(11.0))
                                    .text_color(theme.muted_foreground.opacity(0.5))
                                    .child("⌘↵ save · Esc close"),
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
                            .bg(gpui::red())
                            .text_color(gpui::white())
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
            .child(backdrop)
            .child(card)
    }
}

// ── Reusable building blocks ──────────────────────────────────────

fn section_content(title: &str, subtitle: &str, theme: &gpui_component::Theme) -> Div {
    div()
        .flex()
        .flex_col()
        .gap(px(16.0))
        .child(
            div()
                .flex()
                .flex_col()
                .gap(px(2.0))
                .child(
                    div()
                        .text_base()
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(title.to_string()),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(theme.muted_foreground)
                        .child(subtitle.to_string()),
                ),
        )
}

fn group_label(text: &str) -> Div {
    div()
        .text_size(px(10.0))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(gpui::hsla(0.0, 0.0, 0.50, 0.7))
        .px(px(4.0))
        .pt(px(4.0))
        .child(text.to_string())
}

fn card(theme: &gpui_component::Theme) -> Div {
    div()
        .flex()
        .flex_col()
        .rounded(px(8.0))
        .bg(theme.muted.opacity(0.08))
}

fn row_separator(_theme: &gpui_component::Theme) -> Div {
    // Borderless design — use vertical spacing instead of separator lines
    div().h(px(1.0))
}

fn row_field(label: &str, input: &Entity<InputState>) -> Div {
    div()
        .flex()
        .items_center()
        .justify_between()
        .gap(px(12.0))
        .px(px(16.0))
        .h(px(44.0))
        .child(
            div().text_sm().flex_shrink_0().child(label.to_string()),
        )
        .child(
            div().flex_1().min_w(px(160.0)).child(Input::new(input)),
        )
}

fn key_row(action: &str, shortcut: &str, theme: &gpui_component::Theme) -> Div {
    div()
        .flex()
        .items_center()
        .justify_between()
        .px(px(16.0))
        .h(px(36.0))
        .child(
            div().text_sm().child(action.to_string()),
        )
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

/// Model lists sourced from models.dev (3pp/models.dev/providers/*/models/).
fn provider_models(provider: &ProviderKind) -> &'static [&'static str] {
    match provider {
        ProviderKind::Anthropic => &[
            "claude-opus-4-6",
            "claude-sonnet-4-6",
            "claude-opus-4-5",
            "claude-sonnet-4-5",
            "claude-haiku-4-5",
            "claude-opus-4-1",
            "claude-sonnet-4-0",
            "claude-3-5-sonnet-20241022",
            "claude-3-5-haiku-20241022",
        ],
        ProviderKind::OpenAI => &[
            "o4-mini",
            "o3",
            "o3-pro",
            "o3-mini",
            "gpt-4o",
            "gpt-4o-mini",
            "gpt-4.1",
            "gpt-4.1-mini",
            "gpt-4.1-nano",
            "o1",
            "o1-pro",
        ],
        ProviderKind::DeepSeek => &[
            "deepseek-chat",
            "deepseek-reasoner",
        ],
        ProviderKind::Groq => &[
            "llama-3.3-70b-versatile",
            "llama-3.1-8b-instant",
            "deepseek-r1-distill-llama-70b",
            "qwen-qwq-32b",
            "mistral-saba-24b",
            "gemma2-9b-it",
            "llama3-70b-8192",
        ],
        ProviderKind::Gemini => &[
            "gemini-2.5-pro",
            "gemini-2.5-flash",
            "gemini-2.5-flash-lite",
            "gemini-2.0-flash",
            "gemini-1.5-pro",
            "gemini-1.5-flash",
        ],
        ProviderKind::Mistral => &[
            "mistral-large-latest",
            "mistral-medium-latest",
            "mistral-small-latest",
            "codestral-latest",
            "devstral-small-2505",
            "mistral-nemo",
        ],
        ProviderKind::Cohere => &[
            "command-a-03-2025",
            "command-a-reasoning-08-2025",
            "command-r-plus-08-2024",
            "command-r-08-2024",
        ],
        ProviderKind::Perplexity => &[
            "sonar-pro",
            "sonar-reasoning-pro",
            "sonar-deep-research",
            "sonar",
        ],
        ProviderKind::XAI => &[
            "grok-4",
            "grok-4-fast",
            "grok-3",
            "grok-3-fast",
            "grok-3-mini",
            "grok-2",
        ],
        ProviderKind::Ollama => &[
            "llama3.2",
            "llama3.1",
            "qwen2.5-coder",
            "deepseek-v3",
            "mistral",
            "gemma3",
            "codellama",
        ],
        // OpenAICompatible, OpenRouter, Together — user provides custom model
        _ => &[],
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
        "one-half-dark" => "One Half Dark".into(),
        "kanagawa-wave" => "Kanagawa Wave".into(),
        "everforest-dark" => "Everforest Dark".into(),
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
