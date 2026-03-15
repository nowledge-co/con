use con_agent::{AgentConfig, ProviderKind};
use con_core::Config;
use gpui::*;

use gpui_component::input::InputState;
use gpui_component::{ActiveTheme, input::Input};

// ── Actions ────────────────────────────────────────────────────────

actions!(
    settings,
    [ToggleSettings, SaveSettings, DismissSettings]
);

// ── Settings panel ─────────────────────────────────────────────────

/// Modal settings panel with real text inputs.
/// Opened with Cmd+, — standard macOS convention.
pub struct SettingsPanel {
    visible: bool,
    config: Config,
    focus_handle: FocusHandle,

    selected_provider: ProviderKind,
    model_input: Entity<InputState>,
    api_key_env_input: Entity<InputState>,
    base_url_input: Entity<InputState>,
    max_tokens_input: Entity<InputState>,
    max_turns_input: Entity<InputState>,
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
        let agent = config.agent.clone();

        let model_val = agent.model.clone().unwrap_or_default();
        let api_key_val = agent.api_key_env.clone().unwrap_or_default();
        let base_url_val = agent.base_url.clone().unwrap_or_default();
        let max_tokens_val = agent.max_tokens.to_string();
        let max_turns_val = agent.max_turns.to_string();

        let model_input = cx.new(|cx| {
            let mut state = InputState::new(window, cx);
            state.set_placeholder("Provider default", window, cx);
            state.set_value(&model_val, window, cx);
            state
        });

        let api_key_env_input = cx.new(|cx| {
            let mut state = InputState::new(window, cx);
            state.set_placeholder("e.g. ANTHROPIC_API_KEY", window, cx);
            state.set_value(&api_key_val, window, cx);
            state
        });

        let base_url_input = cx.new(|cx| {
            let mut state = InputState::new(window, cx);
            state.set_placeholder("Default endpoint", window, cx);
            state.set_value(&base_url_val, window, cx);
            state
        });

        let max_tokens_input = cx.new(|cx| {
            let mut state = InputState::new(window, cx);
            state.set_placeholder("4096", window, cx);
            state.set_value(&max_tokens_val, window, cx);
            state
        });

        let max_turns_input = cx.new(|cx| {
            let mut state = InputState::new(window, cx);
            state.set_placeholder("10", window, cx);
            state.set_value(&max_turns_val, window, cx);
            state
        });

        Self {
            visible: false,
            config: config.clone(),
            focus_handle: cx.focus_handle(),
            selected_provider: config.agent.provider.clone(),
            model_input,
            api_key_env_input,
            base_url_input,
            max_tokens_input,
            max_turns_input,
        }
    }

    pub fn toggle(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.visible = !self.visible;
        if self.visible {
            // Reset inputs to current config
            let agent = self.config.agent.clone();
            self.selected_provider = agent.provider.clone();

            let model_val = agent.model.unwrap_or_default();
            let api_key_val = agent.api_key_env.unwrap_or_default();
            let base_url_val = agent.base_url.unwrap_or_default();
            let max_tokens_val = agent.max_tokens.to_string();
            let max_turns_val = agent.max_turns.to_string();

            self.model_input.update(cx, |s, cx| {
                s.set_value(&model_val, window, cx);
            });
            self.api_key_env_input.update(cx, |s, cx| {
                s.set_value(&api_key_val, window, cx);
            });
            self.base_url_input.update(cx, |s, cx| {
                s.set_value(&base_url_val, window, cx);
            });
            self.max_tokens_input.update(cx, |s, cx| {
                s.set_value(&max_tokens_val, window, cx);
            });
            self.max_turns_input.update(cx, |s, cx| {
                s.set_value(&max_turns_val, window, cx);
            });
            self.focus_handle.focus(window, cx);
        }
        cx.notify();
    }

    pub fn is_visible(&self) -> bool {
        self.visible
    }

    fn save(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        let model_text = self.model_input.read(cx).value().to_string();
        let api_key_text = self.api_key_env_input.read(cx).value().to_string();
        let base_url_text = self.base_url_input.read(cx).value().to_string();
        let max_tokens_text = self.max_tokens_input.read(cx).value().to_string();
        let max_turns_text = self.max_turns_input.read(cx).value().to_string();

        self.config.agent = AgentConfig {
            provider: self.selected_provider.clone(),
            model: if model_text.is_empty() { None } else { Some(model_text) },
            api_key_env: if api_key_text.is_empty() { None } else { Some(api_key_text) },
            base_url: if base_url_text.is_empty() { None } else { Some(base_url_text) },
            max_tokens: max_tokens_text.parse().unwrap_or(4096),
            max_turns: max_turns_text.parse().unwrap_or(10),
            auto_context: self.config.agent.auto_context,
        };

        if let Err(e) = self.persist_config() {
            log::error!("Failed to save config: {}", e);
        }

        self.visible = false;
        cx.notify();
    }

    fn persist_config(&self) -> anyhow::Result<()> {
        let path = Config::config_path();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let content = toml::to_string_pretty(&self.config)?;
        std::fs::write(&path, content)?;
        Ok(())
    }

    fn select_provider(&mut self, provider: ProviderKind, cx: &mut Context<Self>) {
        self.selected_provider = provider;
        cx.notify();
    }

    fn render_provider_grid(&self, cx: &mut Context<Self>) -> Div {
        let theme = cx.theme();
        let mut grid = div().flex().flex_wrap().gap(px(6.0));

        for provider in ALL_PROVIDERS {
            let is_selected = *provider == self.selected_provider;
            let label = provider_label(provider);
            let provider_clone = provider.clone();

            let chip = div()
                .id(SharedString::from(format!("provider-{label}")))
                .px(px(10.0))
                .py(px(6.0))
                .rounded(px(8.0))
                .text_xs()
                .cursor_pointer()
                .font_weight(if is_selected {
                    FontWeight::SEMIBOLD
                } else {
                    FontWeight::NORMAL
                })
                .bg(if is_selected {
                    theme.primary
                } else {
                    theme.secondary
                })
                .text_color(if is_selected {
                    theme.primary_foreground
                } else {
                    theme.secondary_foreground
                })
                .on_mouse_down(MouseButton::Left, cx.listener(move |this, _, _window, cx| {
                    this.select_provider(provider_clone.clone(), cx);
                }))
                .child(label);

            grid = grid.child(chip);
        }

        grid
    }

    fn render_field(
        label: &str,
        input_state: &Entity<InputState>,
    ) -> Div {
        div()
            .flex()
            .flex_col()
            .gap(px(4.0))
            .child(
                div()
                    .text_xs()
                    .font_weight(FontWeight::MEDIUM)
                    .child(label.to_string()),
            )
            .child(Input::new(input_state))
    }
}

impl Focusable for SettingsPanel {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for SettingsPanel {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if !self.visible {
            return div();
        }

        // Build mutable-borrow components first (needs cx.listener)
        let provider_grid = self.render_provider_grid(cx);

        // Clone input state refs for render_field (immutable)
        let model_input = self.model_input.clone();
        let api_key_env_input = self.api_key_env_input.clone();
        let base_url_input = self.base_url_input.clone();
        let max_tokens_input = self.max_tokens_input.clone();
        let max_turns_input = self.max_turns_input.clone();

        let theme = cx.theme();

        let backdrop = div()
            .absolute()
            .size_full()
            .bg(rgba(0x00000088));

        let card = div()
            .absolute()
            .top(px(60.0))
            .left_auto()
            .right_auto()
            .mx_auto()
            .ml(px(200.0))
            .w(px(520.0))
            .max_h(px(600.0))
            .rounded(px(14.0))
            .bg(theme.title_bar)
            .border_1()
            .border_color(theme.border)
            .flex()
            .flex_col()
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
            .track_focus(&self.focus_handle)
            // Header
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .px(px(20.0))
                    .py(px(16.0))
                    .border_b_1()
                    .border_color(theme.border)
                    .child(
                        div()
                            .text_base()
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(theme.foreground)
                            .child("Provider Settings"),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(theme.muted_foreground)
                            .child("⌘Enter to save · Esc to close"),
                    ),
            )
            // Provider section
            .child(
                div()
                    .flex()
                    .flex_col()
                    .px(px(20.0))
                    .pt(px(16.0))
                    .pb(px(12.0))
                    .gap(px(8.0))
                    .child(
                        div()
                            .text_xs()
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(theme.secondary_foreground)
                            .child("PROVIDER"),
                    )
                    .child(provider_grid),
            )
            // Model section
            .child(
                div()
                    .flex()
                    .flex_col()
                    .px(px(20.0))
                    .py(px(8.0))
                    .gap(px(12.0))
                    .child(div().h(px(1.0)).bg(theme.border))
                    .child(
                        div()
                            .text_xs()
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(theme.secondary_foreground)
                            .child("MODEL CONFIGURATION"),
                    )
                    .child(Self::render_field("Model", &model_input))
                    .child(Self::render_field("API Key Env Var", &api_key_env_input))
                    .child(Self::render_field("Base URL", &base_url_input)),
            )
            // Advanced section
            .child(
                div()
                    .flex()
                    .flex_col()
                    .px(px(20.0))
                    .pt(px(8.0))
                    .pb(px(20.0))
                    .gap(px(12.0))
                    .child(div().h(px(1.0)).bg(theme.border))
                    .child(
                        div()
                            .text_xs()
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(theme.secondary_foreground)
                            .child("ADVANCED"),
                    )
                    .child(
                        div()
                            .flex()
                            .gap(px(12.0))
                            .child(
                                div()
                                    .flex_1()
                                    .child(Self::render_field("Max Tokens", &max_tokens_input)),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .child(Self::render_field("Max Turns", &max_turns_input)),
                            ),
                    ),
            );

        div()
            .absolute()
            .size_full()
            .child(backdrop)
            .child(card)
    }
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
