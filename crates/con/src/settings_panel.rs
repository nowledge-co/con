use con_agent::{AgentConfig, ProviderKind};
use con_core::Config;
use gpui::*;

use gpui_component::ActiveTheme;

// ── Actions ────────────────────────────────────────────────────────

actions!(
    settings,
    [
        ToggleSettings,
        SaveSettings,
        DismissSettings,
        FocusNextField,
        FocusPrevField,
    ]
);

// ── Settings panel ─────────────────────────────────────────────────

/// Modal settings panel — Apple-style grouped sections.
/// Opened with Cmd+, — standard macOS convention.
pub struct SettingsPanel {
    visible: bool,
    config: Config,
    focus_handle: FocusHandle,

    // Editable state
    selected_provider: ProviderKind,
    model_text: String,
    api_key_env_text: String,
    base_url_text: String,
    max_tokens_text: String,
    max_turns_text: String,

    // Which field is active for editing
    active_field: SettingsField,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum SettingsField {
    Provider,
    Model,
    ApiKeyEnv,
    BaseUrl,
    MaxTokens,
    MaxTurns,
}

impl SettingsField {
    fn next(self) -> Self {
        match self {
            Self::Provider => Self::Model,
            Self::Model => Self::ApiKeyEnv,
            Self::ApiKeyEnv => Self::BaseUrl,
            Self::BaseUrl => Self::MaxTokens,
            Self::MaxTokens => Self::MaxTurns,
            Self::MaxTurns => Self::Provider,
        }
    }

    fn prev(self) -> Self {
        match self {
            Self::Provider => Self::MaxTurns,
            Self::Model => Self::Provider,
            Self::ApiKeyEnv => Self::Model,
            Self::BaseUrl => Self::ApiKeyEnv,
            Self::MaxTokens => Self::BaseUrl,
            Self::MaxTurns => Self::MaxTokens,
        }
    }
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
    pub fn new(config: &Config, cx: &mut Context<Self>) -> Self {
        let agent = &config.agent;
        Self {
            visible: false,
            config: config.clone(),
            focus_handle: cx.focus_handle(),
            selected_provider: agent.provider.clone(),
            model_text: agent.model.clone().unwrap_or_default(),
            api_key_env_text: agent.api_key_env.clone().unwrap_or_default(),
            base_url_text: agent.base_url.clone().unwrap_or_default(),
            max_tokens_text: agent.max_tokens.to_string(),
            max_turns_text: agent.max_turns.to_string(),
            active_field: SettingsField::Provider,
        }
    }

    pub fn toggle(&mut self, cx: &mut Context<Self>) {
        self.visible = !self.visible;
        if self.visible {
            // Reset to current config when opening
            let agent = &self.config.agent;
            self.selected_provider = agent.provider.clone();
            self.model_text = agent.model.clone().unwrap_or_default();
            self.api_key_env_text = agent.api_key_env.clone().unwrap_or_default();
            self.base_url_text = agent.base_url.clone().unwrap_or_default();
            self.max_tokens_text = agent.max_tokens.to_string();
            self.max_turns_text = agent.max_turns.to_string();
            self.active_field = SettingsField::Provider;
        }
        cx.notify();
    }

    pub fn is_visible(&self) -> bool {
        self.visible
    }

    /// Returns the current effective config (after any saves)
    pub fn config(&self) -> &Config {
        &self.config
    }

    fn save(&mut self, cx: &mut Context<Self>) {
        self.config.agent = AgentConfig {
            provider: self.selected_provider.clone(),
            model: if self.model_text.is_empty() {
                None
            } else {
                Some(self.model_text.clone())
            },
            api_key_env: if self.api_key_env_text.is_empty() {
                None
            } else {
                Some(self.api_key_env_text.clone())
            },
            base_url: if self.base_url_text.is_empty() {
                None
            } else {
                Some(self.base_url_text.clone())
            },
            max_tokens: self.max_tokens_text.parse().unwrap_or(4096),
            max_turns: self.max_turns_text.parse().unwrap_or(10),
            auto_context: self.config.agent.auto_context,
        };

        // Persist to disk
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

    fn handle_key(&mut self, event: &KeyDownEvent, cx: &mut Context<Self>) {
        match &event.keystroke.key {
            key if key == "tab" => {
                if event.keystroke.modifiers.shift {
                    self.active_field = self.active_field.prev();
                } else {
                    self.active_field = self.active_field.next();
                }
                cx.notify();
            }
            key if key == "escape" => {
                self.visible = false;
                cx.notify();
            }
            key if key == "enter" => {
                if event.keystroke.modifiers.platform {
                    self.save(cx);
                }
            }
            key if key == "backspace" => {
                self.active_text_mut().pop();
                cx.notify();
            }
            key if key.len() == 1 && !event.keystroke.modifiers.platform => {
                let ch = if event.keystroke.modifiers.shift {
                    key.to_uppercase()
                } else {
                    key.to_string()
                };
                // For numeric fields, only accept digits
                match self.active_field {
                    SettingsField::MaxTokens | SettingsField::MaxTurns => {
                        if ch.chars().all(|c| c.is_ascii_digit()) {
                            self.active_text_mut().push_str(&ch);
                        }
                    }
                    SettingsField::Provider => {
                        // Provider uses arrow keys / click, not text
                    }
                    _ => {
                        self.active_text_mut().push_str(&ch);
                    }
                }
                cx.notify();
            }
            key if (key == "left" || key == "right") && self.active_field == SettingsField::Provider => {
                let current_idx = ALL_PROVIDERS
                    .iter()
                    .position(|p| p == &self.selected_provider)
                    .unwrap_or(0);
                let new_idx = if key == "right" {
                    (current_idx + 1) % ALL_PROVIDERS.len()
                } else if current_idx == 0 {
                    ALL_PROVIDERS.len() - 1
                } else {
                    current_idx - 1
                };
                self.selected_provider = ALL_PROVIDERS[new_idx].clone();
                cx.notify();
            }
            _ => {}
        }
    }

    fn active_text_mut(&mut self) -> &mut String {
        match self.active_field {
            SettingsField::Model => &mut self.model_text,
            SettingsField::ApiKeyEnv => &mut self.api_key_env_text,
            SettingsField::BaseUrl => &mut self.base_url_text,
            SettingsField::MaxTokens => &mut self.max_tokens_text,
            SettingsField::MaxTurns => &mut self.max_turns_text,
            SettingsField::Provider => &mut self.model_text, // no-op target
        }
    }

    // ── Render helpers ─────────────────────────────────────────────

    fn render_provider_grid(&self, cx: &App) -> Div {
        let theme = cx.theme();
        let mut grid = div().flex().flex_wrap().gap(px(6.0));

        for provider in ALL_PROVIDERS {
            let is_selected = *provider == self.selected_provider;
            let label = provider_label(provider);

            let chip = div()
                .px(px(10.0))
                .py(px(6.0))
                .rounded(px(8.0))
                .text_xs()
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
                .child(label);

            grid = grid.child(chip);
        }

        grid
    }

    fn render_field(&self, label: &str, hint: &str, value: &str, field: SettingsField, cx: &App) -> Div {
        let theme = cx.theme();
        let is_active = self.active_field == field;

        div()
            .flex()
            .flex_col()
            .gap(px(4.0))
            // Label row
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .text_xs()
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(theme.secondary_foreground)
                            .child(label.to_string()),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(theme.muted_foreground)
                            .child(hint.to_string()),
                    ),
            )
            // Value field
            .child(
                div()
                    .h(px(34.0))
                    .px(px(10.0))
                    .flex()
                    .items_center()
                    .rounded(px(8.0))
                    .bg(theme.background)
                    .border_1()
                    .border_color(if is_active {
                        theme.primary
                    } else {
                        theme.border
                    })
                    .child(if value.is_empty() {
                        div()
                            .text_sm()
                            .text_color(theme.muted_foreground)
                            .child("Default".to_string())
                    } else {
                        div()
                            .text_sm()
                            .text_color(theme.foreground)
                            .child(format!(
                                "{}{}",
                                value,
                                if is_active { "▎" } else { "" }
                            ))
                    }),
            )
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

        let theme = cx.theme();

        // Backdrop
        let backdrop = div()
            .absolute()
            .size_full()
            .bg(rgba(0x00000088));

        // Card
        let card = div()
            .absolute()
            .top(px(60.0))
            .left_auto()
            .right_auto()
            .mx_auto()
            // Center horizontally with margins
            .ml(px(200.0))
            .w(px(520.0))
            .max_h(px(600.0))
            .rounded(px(14.0))
            .bg(theme.title_bar)
            .border_1()
            .border_color(theme.border)
            .flex()
            .flex_col()
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, _window, cx| {
                this.handle_key(event, cx);
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
                    .child(self.render_provider_grid(cx)),
            )
            // Model section
            .child(
                div()
                    .flex()
                    .flex_col()
                    .px(px(20.0))
                    .py(px(8.0))
                    .gap(px(12.0))
                    .child(
                        div()
                            .h(px(1.0))
                            .bg(theme.border),
                    )
                    .child(
                        div()
                            .text_xs()
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(theme.secondary_foreground)
                            .child("MODEL CONFIGURATION"),
                    )
                    .child(self.render_field(
                        "Model",
                        "Leave empty for provider default",
                        &self.model_text.clone(),
                        SettingsField::Model,
                        cx,
                    ))
                    .child(self.render_field(
                        "API Key Environment Variable",
                        "e.g. ANTHROPIC_API_KEY",
                        &self.api_key_env_text.clone(),
                        SettingsField::ApiKeyEnv,
                        cx,
                    ))
                    .child(self.render_field(
                        "Base URL",
                        "For custom/proxy endpoints",
                        &self.base_url_text.clone(),
                        SettingsField::BaseUrl,
                        cx,
                    )),
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
                    .child(
                        div()
                            .h(px(1.0))
                            .bg(theme.border),
                    )
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
                                div().flex_1().child(self.render_field(
                                    "Max Tokens",
                                    "",
                                    &self.max_tokens_text.clone(),
                                    SettingsField::MaxTokens,
                                    cx,
                                )),
                            )
                            .child(
                                div().flex_1().child(self.render_field(
                                    "Max Turns",
                                    "",
                                    &self.max_turns_text.clone(),
                                    SettingsField::MaxTurns,
                                    cx,
                                )),
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
