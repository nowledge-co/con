use con_agent::{
    AgentConfig, OAuthDevicePrompt, ProviderConfig, ProviderKind, SuggestionModelConfig,
    authorize_oauth_provider,
};
use con_core::Config;
use futures::{FutureExt, StreamExt};
use gpui::*;

use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::clipboard::Clipboard;
use gpui_component::input::InputState;
use gpui_component::kbd::Kbd;
use gpui_component::select::{SearchableVec, Select, SelectEvent, SelectState};
use gpui_component::slider::{Slider, SliderEvent, SliderState};
use gpui_component::switch::Switch;
use gpui_component::{ActiveTheme, Disableable, Icon, IndexPath, Sizable as _, input::Input};

use crate::model_registry::ModelRegistry;
use crate::motion::{MotionValue, vertical_reveal_offset};
use std::collections::HashMap;
use std::sync::Arc;

actions!(settings, [ToggleSettings, SaveSettings, DismissSettings]);

/// Emitted when the user selects a different terminal theme for live preview.
pub struct ThemePreview(pub String);

#[derive(Debug, Clone, Default)]
struct ProviderOAuthState {
    in_progress: bool,
    connected: bool,
    prompt: Option<OAuthDevicePrompt>,
    status_message: Option<String>,
    error_message: Option<String>,
}

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
    registry: ModelRegistry,
    oauth_runtime: Arc<tokio::runtime::Runtime>,
    focus_handle: FocusHandle,
    active_section: SettingsSection,
    overlay_motion: MotionValue,

    selected_provider: ProviderKind,
    model_input: Entity<InputState>,
    model_select: Entity<SelectState<SearchableVec<String>>>,
    endpoint_preset_select: Entity<SelectState<Vec<String>>>,
    api_key_input: Entity<InputState>,
    base_url_input: Entity<InputState>,
    max_tokens_input: Entity<InputState>,
    max_turns_input: Entity<InputState>,
    temperature_input: Entity<InputState>,
    auto_approve: bool,

    suggestion_model_input: Entity<InputState>,
    oauth_states: HashMap<ProviderKind, ProviderOAuthState>,

    terminal_font_select: Entity<SelectState<SearchableVec<String>>>,
    ui_font_select: Entity<SelectState<SearchableVec<String>>>,
    font_size_input: Entity<InputState>,
    terminal_opacity_slider: Entity<SliderState>,
    ui_opacity_slider: Entity<SliderState>,
    background_image_input: Entity<InputState>,
    background_image_opacity_slider: Entity<SliderState>,
    background_image_position_select: Entity<SelectState<Vec<String>>>,
    background_image_fit_select: Entity<SelectState<Vec<String>>>,
    background_image_repeat: bool,
    save_error: Option<String>,

    // Theme import
    custom_theme_name_input: Entity<InputState>,
    custom_theme_preview: Option<con_terminal::TerminalTheme>,
    custom_theme_status: Option<String>,

    // Keybindings — which binding is being recorded (field name, e.g. "new_tab")
    recording_key: Option<String>,
}

const SIDEBAR_PROVIDERS: &[ProviderKind] = &[
    ProviderKind::Anthropic,
    ProviderKind::OpenAI,
    ProviderKind::ChatGPT,
    ProviderKind::GitHubCopilot,
    ProviderKind::OpenAICompatible,
    ProviderKind::MiniMax,
    ProviderKind::Moonshot,
    ProviderKind::ZAI,
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

const BACKGROUND_IMAGE_POSITIONS: &[&str] = &[
    "top-left",
    "top-center",
    "top-right",
    "center-left",
    "center",
    "center-right",
    "bottom-left",
    "bottom-center",
    "bottom-right",
];

const BACKGROUND_IMAGE_FITS: &[&str] = &["contain", "cover", "stretch", "none"];
const ENDPOINT_DEFAULT_LABEL: &str = "Provider Default";
const ENDPOINT_CUSTOM_LABEL: &str = "Custom";

#[derive(Clone, Copy)]
struct EndpointPreset {
    label: &'static str,
    base_url: &'static str,
}

impl SettingsPanel {
    fn clamp_terminal_opacity(value: f32) -> f32 {
        value.clamp(0.25, 1.0)
    }

    fn clamp_ui_opacity(value: f32) -> f32 {
        value.clamp(0.35, 1.0)
    }

    fn terminal_opacity_value(&self) -> f32 {
        Self::clamp_terminal_opacity(self.config.appearance.terminal_opacity)
    }

    fn ui_opacity_value(&self) -> f32 {
        Self::clamp_ui_opacity(self.config.appearance.ui_opacity)
    }

    fn clamp_background_image_opacity(value: f32) -> f32 {
        value.clamp(0.0, 1.0)
    }

    fn background_image_opacity_value(&self) -> f32 {
        Self::clamp_background_image_opacity(self.config.appearance.background_image_opacity)
    }

    fn make_string_select(
        options: &[&str],
        current_value: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<SelectState<Vec<String>>> {
        let items: Vec<String> = options.iter().map(|value| (*value).to_string()).collect();
        let selected_index = items
            .iter()
            .position(|item| item == current_value)
            .map(IndexPath::new);
        cx.new(|cx| SelectState::new(items, selected_index, window, cx))
    }

    fn prepare_font_families(config: &Config, mut font_families: Vec<String>) -> Vec<String> {
        font_families.sort_by_key(|name| name.to_lowercase());
        font_families.dedup();

        let mut preferred = Vec::new();
        for family in [
            ".SystemUIFont",
            "Ioskeley Mono",
            config.terminal.font_family.as_str(),
            config.appearance.ui_font_family.as_str(),
        ] {
            if !family.is_empty() && !preferred.iter().any(|existing| existing == family) {
                preferred.push(family.to_string());
            }
        }

        for family in font_families {
            if !preferred.iter().any(|existing| existing == &family) {
                preferred.push(family);
            }
        }
        preferred
    }

    fn provider_api_key_placeholder(provider: &ProviderKind) -> &'static str {
        match provider {
            ProviderKind::ChatGPT | ProviderKind::GitHubCopilot => {
                "Optional override for advanced setups"
            }
            _ => "sk-... or env var like ANTHROPIC_API_KEY",
        }
    }

    fn provider_api_key_label(provider: &ProviderKind) -> &'static str {
        match provider {
            ProviderKind::ChatGPT => "Access Token Override",
            ProviderKind::GitHubCopilot => "API Key Override",
            _ => "API Key",
        }
    }

    fn provider_api_key_hint(provider: &ProviderKind) -> &'static str {
        match provider {
            ProviderKind::ChatGPT => {
                "Optional. Leave blank to use Con-managed ChatGPT device login."
            }
            ProviderKind::GitHubCopilot => {
                "Optional. Leave blank to use Con-managed GitHub device login."
            }
            _ => "Paste a key or an env var name like ANTHROPIC_API_KEY",
        }
    }

    fn provider_has_oauth(provider: &ProviderKind) -> bool {
        matches!(
            provider,
            ProviderKind::ChatGPT | ProviderKind::GitHubCopilot
        )
    }

    fn provider_oauth_label(provider: &ProviderKind) -> Option<&'static str> {
        match provider {
            ProviderKind::ChatGPT => Some("ChatGPT Subscription"),
            ProviderKind::GitHubCopilot => Some("GitHub Copilot"),
            _ => None,
        }
    }

    fn provider_oauth_button_label(provider: &ProviderKind) -> Option<&'static str> {
        match provider {
            ProviderKind::ChatGPT => Some("Sign In with ChatGPT"),
            ProviderKind::GitHubCopilot => Some("Sign In with GitHub"),
            _ => None,
        }
    }

    fn sidebar_provider_kind(provider: &ProviderKind) -> ProviderKind {
        match provider {
            ProviderKind::MiniMaxAnthropic => ProviderKind::MiniMax,
            ProviderKind::MoonshotAnthropic => ProviderKind::Moonshot,
            ProviderKind::ZAIAnthropic => ProviderKind::ZAI,
            _ => provider.clone(),
        }
    }

    fn protocol_pair(provider: &ProviderKind) -> Option<(ProviderKind, ProviderKind)> {
        match Self::sidebar_provider_kind(provider) {
            ProviderKind::MiniMax => Some((ProviderKind::MiniMax, ProviderKind::MiniMaxAnthropic)),
            ProviderKind::Moonshot => {
                Some((ProviderKind::Moonshot, ProviderKind::MoonshotAnthropic))
            }
            ProviderKind::ZAI => Some((ProviderKind::ZAI, ProviderKind::ZAIAnthropic)),
            _ => None,
        }
    }

    fn uses_anthropic_protocol(provider: &ProviderKind) -> bool {
        matches!(
            provider,
            ProviderKind::MiniMaxAnthropic
                | ProviderKind::MoonshotAnthropic
                | ProviderKind::ZAIAnthropic
        )
    }

    fn protocol_switch_label(provider: &ProviderKind) -> Option<&'static str> {
        Self::protocol_pair(provider).map(|_| "Anthropic API")
    }

    fn protocol_switch_hint(provider: &ProviderKind) -> Option<&'static str> {
        Self::protocol_pair(provider)
            .map(|_| "Switch between OpenAI and Anthropic API compatible transport")
    }

    fn sidebar_selection_target(
        clicked_provider: &ProviderKind,
        current_selected: &ProviderKind,
    ) -> ProviderKind {
        if Self::sidebar_provider_kind(current_selected) == *clicked_provider
            && Self::uses_anthropic_protocol(current_selected)
        {
            current_selected.clone()
        } else {
            clicked_provider.clone()
        }
    }

    fn protocol_toggled_provider(
        provider: &ProviderKind,
        use_anthropic: bool,
    ) -> Option<ProviderKind> {
        Self::protocol_pair(provider).map(|(openai_kind, anthropic_kind)| {
            if use_anthropic {
                anthropic_kind
            } else {
                openai_kind
            }
        })
    }

    fn oauth_state(&self, provider: &ProviderKind) -> Option<&ProviderOAuthState> {
        self.oauth_states.get(provider)
    }

    fn oauth_state_mut(&mut self, provider: &ProviderKind) -> &mut ProviderOAuthState {
        self.oauth_states.entry(provider.clone()).or_default()
    }

    fn start_provider_oauth(
        &mut self,
        provider: ProviderKind,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let state = self.oauth_state_mut(&provider);
        state.in_progress = true;
        state.connected = false;
        state.prompt = None;
        state.status_message = Some("Waiting for device authorization…".to_string());
        state.error_message = None;
        cx.notify();

        let runtime = self.oauth_runtime.clone();
        cx.spawn_in(window, async move |this, window| {
            let (prompt_tx, prompt_rx) = futures::channel::mpsc::unbounded::<OAuthDevicePrompt>();
            let (result_tx, result_rx) = futures::channel::oneshot::channel::<Result<(), String>>();

            runtime.spawn({
                let provider = provider.clone();
                async move {
                    let result = authorize_oauth_provider(provider, move |prompt| {
                        let _ = prompt_tx.unbounded_send(prompt);
                    })
                    .await
                    .map_err(|err| err.to_string());

                    let _ = result_tx.send(result);
                }
            });

            let mut prompt_rx = prompt_rx.fuse();
            let mut result_rx = result_rx.fuse();

            loop {
                futures::select! {
                    maybe_prompt = prompt_rx.next() => {
                        let Some(prompt) = maybe_prompt else {
                            continue;
                        };
                        let verification_uri = prompt.verification_uri.clone();
                        let _ = window.update(|window, cx| {
                            let _ = this.update(cx, |panel, cx| {
                                let state = panel.oauth_state_mut(&provider);
                                state.prompt = Some(prompt);
                                state.status_message = Some("Finish sign-in in your browser. Con will continue automatically.".to_string());
                                state.error_message = None;
                                cx.open_url(&verification_uri);
                                cx.notify();
                            });
                            let _ = window;
                        });
                    }
                    result = result_rx => {
                        let _ = window.update(|window, cx| {
                            let _ = this.update(cx, |panel, cx| {
                                let state = panel.oauth_state_mut(&provider);
                                state.in_progress = false;
                                match result {
                                    Ok(Ok(())) => {
                                        state.connected = true;
                                        state.prompt = None;
                                        state.status_message = Some("Authorized and stored in Con’s auth cache.".to_string());
                                        state.error_message = None;
                                    }
                                    Ok(Err(err)) => {
                                        state.connected = false;
                                        state.error_message = Some(err);
                                        state.status_message = None;
                                    }
                                    Err(_) => {
                                        state.connected = false;
                                        state.error_message = Some("OAuth flow ended unexpectedly.".to_string());
                                        state.status_message = None;
                                    }
                                }
                                cx.notify();
                            });
                            let _ = window;
                        });
                        break;
                    }
                }
            }
        })
        .detach();
    }

    fn provider_base_url_hint(provider: &ProviderKind) -> &'static str {
        if Self::provider_endpoint_presets(provider).is_empty() {
            "Leave blank for the default endpoint"
        } else {
            "Leave blank for the provider default, or choose a preset below"
        }
    }

    fn provider_endpoint_presets(provider: &ProviderKind) -> &'static [EndpointPreset] {
        match provider {
            ProviderKind::MiniMax => &[
                EndpointPreset {
                    label: "Global",
                    base_url: "https://api.minimax.io/v1",
                },
                EndpointPreset {
                    label: "China",
                    base_url: "https://api.minimaxi.com/v1",
                },
            ],
            ProviderKind::MiniMaxAnthropic => &[
                EndpointPreset {
                    label: "Global",
                    base_url: "https://api.minimax.io/anthropic",
                },
                EndpointPreset {
                    label: "China",
                    base_url: "https://api.minimaxi.com/anthropic",
                },
            ],
            ProviderKind::Moonshot => &[
                EndpointPreset {
                    label: "Global",
                    base_url: "https://api.moonshot.ai/v1",
                },
                EndpointPreset {
                    label: "China",
                    base_url: "https://api.moonshot.cn/v1",
                },
            ],
            ProviderKind::MoonshotAnthropic => &[EndpointPreset {
                label: "Global",
                base_url: "https://api.moonshot.ai/anthropic",
            }],
            ProviderKind::ZAI => &[
                EndpointPreset {
                    label: "General",
                    base_url: "https://api.z.ai/api/paas/v4",
                },
                EndpointPreset {
                    label: "Coding",
                    base_url: "https://api.z.ai/api/coding/paas/v4",
                },
            ],
            ProviderKind::ZAIAnthropic => &[EndpointPreset {
                label: "Anthropic",
                base_url: "https://api.z.ai/api/anthropic",
            }],
            _ => &[],
        }
    }

    fn endpoint_options(provider: &ProviderKind) -> Vec<String> {
        let presets = Self::provider_endpoint_presets(provider);
        if presets.is_empty() {
            return vec![ENDPOINT_DEFAULT_LABEL.to_string()];
        }

        let mut options = Vec::with_capacity(presets.len() + 2);
        options.push(ENDPOINT_DEFAULT_LABEL.to_string());
        options.extend(presets.iter().map(|preset| preset.label.to_string()));
        options.push(ENDPOINT_CUSTOM_LABEL.to_string());
        options
    }

    fn endpoint_label_for_base_url(
        provider: &ProviderKind,
        base_url: Option<&str>,
    ) -> &'static str {
        let Some(base_url) = base_url.map(str::trim).filter(|value| !value.is_empty()) else {
            return ENDPOINT_DEFAULT_LABEL;
        };

        for preset in Self::provider_endpoint_presets(provider) {
            if preset.base_url == base_url {
                return preset.label;
            }
        }

        ENDPOINT_CUSTOM_LABEL
    }

    fn make_endpoint_preset_select(
        provider: &ProviderKind,
        current_base_url: &Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<SelectState<Vec<String>>> {
        let options = Self::endpoint_options(provider);
        let selected_index = options
            .iter()
            .position(|item| {
                item == Self::endpoint_label_for_base_url(provider, current_base_url.as_deref())
            })
            .map(IndexPath::new);
        let entity = cx.new(|cx| SelectState::new(options, selected_index, window, cx));
        cx.subscribe_in(
            &entity,
            window,
            |this, _, ev: &SelectEvent<Vec<String>>, window, cx| {
                let SelectEvent::Confirm(Some(value)) = ev else {
                    return;
                };

                match value.as_str() {
                    ENDPOINT_DEFAULT_LABEL => {
                        this.base_url_input
                            .update(cx, |input, cx| input.set_value("", window, cx));
                    }
                    ENDPOINT_CUSTOM_LABEL => {}
                    _ => {
                        if let Some(preset) =
                            Self::provider_endpoint_presets(&this.selected_provider)
                                .iter()
                                .find(|preset| preset.label == value.as_str())
                        {
                            this.base_url_input.update(cx, |input, cx| {
                                input.set_value(preset.base_url, window, cx);
                            });
                        }
                    }
                }

                cx.notify();
            },
        )
        .detach();
        entity
    }

    fn sync_provider_placeholders(
        &self,
        provider: &ProviderKind,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.api_key_input.update(cx, |input, cx| {
            input.set_placeholder(Self::provider_api_key_placeholder(provider), window, cx);
        });
        self.base_url_input.update(cx, |input, cx| {
            input.set_placeholder("Default endpoint", window, cx);
        });
    }

    fn make_searchable_string_select(
        options: &[String],
        current_value: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<SelectState<SearchableVec<String>>> {
        let items = SearchableVec::new(options.to_vec());
        let selected_index = options
            .iter()
            .position(|item| item == current_value)
            .map(IndexPath::new);
        cx.new(|cx| SelectState::new(items, selected_index, window, cx).searchable(true))
    }

    fn card_opacity(&self) -> f32 {
        0.74
    }

    pub fn new(
        config: &Config,
        registry: ModelRegistry,
        oauth_runtime: Arc<tokio::runtime::Runtime>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let agent = &config.agent;
        let pc = agent.providers.get_or_default(&agent.provider);

        let model_input = cx.new(|cx| {
            let mut s = InputState::new(window, cx);
            s.set_placeholder("Provider default", window, cx);
            s.set_value(&pc.model.clone().unwrap_or_default(), window, cx);
            s
        });
        let model_select =
            Self::make_model_select(&agent.provider, &pc.model, &registry, window, cx);
        let endpoint_preset_select =
            Self::make_endpoint_preset_select(&agent.provider, &pc.base_url, window, cx);
        let api_key_input = cx.new(|cx| {
            let mut s = InputState::new(window, cx);
            s.set_placeholder(
                Self::provider_api_key_placeholder(&agent.provider),
                window,
                cx,
            );
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
        let font_families = Self::prepare_font_families(config, cx.text_system().all_font_names());
        let terminal_font_select = Self::make_searchable_string_select(
            &font_families,
            &config.terminal.font_family,
            window,
            cx,
        );
        let ui_font_select = Self::make_searchable_string_select(
            &font_families,
            &config.appearance.ui_font_family,
            window,
            cx,
        );
        let font_size_input = cx.new(|cx| {
            let mut s = InputState::new(window, cx);
            s.set_placeholder("14.0", window, cx);
            s.set_value(&config.terminal.font_size.to_string(), window, cx);
            s
        });
        let terminal_opacity_slider = cx.new(|_| {
            SliderState::new()
                .min(0.25)
                .max(1.0)
                .step(0.01)
                .default_value(Self::clamp_terminal_opacity(
                    config.appearance.terminal_opacity,
                ))
        });
        let ui_opacity_slider = cx.new(|_| {
            SliderState::new()
                .min(0.35)
                .max(1.0)
                .step(0.01)
                .default_value(Self::clamp_ui_opacity(config.appearance.ui_opacity))
        });
        let background_image_input = cx.new(|cx| {
            let mut s = InputState::new(window, cx);
            s.set_placeholder("~/Pictures/wallpaper.jpg", window, cx);
            s.set_value(
                &config
                    .appearance
                    .background_image
                    .clone()
                    .unwrap_or_default(),
                window,
                cx,
            );
            s
        });
        let background_image_opacity_slider = cx.new(|_| {
            SliderState::new()
                .min(0.0)
                .max(1.0)
                .step(0.01)
                .default_value(Self::clamp_background_image_opacity(
                    config.appearance.background_image_opacity,
                ))
        });
        let background_image_position_select = Self::make_string_select(
            BACKGROUND_IMAGE_POSITIONS,
            &config.appearance.background_image_position,
            window,
            cx,
        );
        let background_image_fit_select = Self::make_string_select(
            BACKGROUND_IMAGE_FITS,
            &config.appearance.background_image_fit,
            window,
            cx,
        );
        let custom_theme_name_input = cx.new(|cx| {
            let mut s = InputState::new(window, cx);
            s.set_placeholder("Save as, e.g. flexoki-amber", window, cx);
            s
        });

        cx.subscribe(
            &terminal_opacity_slider,
            |this, _, event: &SliderEvent, cx| match event {
                SliderEvent::Change(value) => {
                    this.config.appearance.terminal_opacity =
                        Self::clamp_terminal_opacity(value.end());
                    cx.notify();
                }
            },
        )
        .detach();
        cx.subscribe(
            &ui_opacity_slider,
            |this, _, event: &SliderEvent, cx| match event {
                SliderEvent::Change(value) => {
                    this.config.appearance.ui_opacity = Self::clamp_ui_opacity(value.end());
                    cx.notify();
                }
            },
        )
        .detach();
        cx.subscribe(
            &background_image_opacity_slider,
            |this, _, event: &SliderEvent, cx| match event {
                SliderEvent::Change(value) => {
                    this.config.appearance.background_image_opacity =
                        Self::clamp_background_image_opacity(value.end());
                    cx.notify();
                }
            },
        )
        .detach();
        cx.subscribe_in(
            &terminal_font_select,
            window,
            |this, _, ev: &SelectEvent<SearchableVec<String>>, _, cx| {
                if let SelectEvent::Confirm(Some(value)) = ev {
                    this.config.terminal.font_family = value.clone();
                    cx.notify();
                }
            },
        )
        .detach();
        cx.subscribe_in(
            &ui_font_select,
            window,
            |this, _, ev: &SelectEvent<SearchableVec<String>>, _, cx| {
                if let SelectEvent::Confirm(Some(value)) = ev {
                    this.config.appearance.ui_font_family = value.clone();
                    cx.notify();
                }
            },
        )
        .detach();
        cx.subscribe_in(
            &background_image_position_select,
            window,
            |this, _, ev: &SelectEvent<Vec<String>>, _, cx| {
                if let SelectEvent::Confirm(Some(value)) = ev {
                    this.config.appearance.background_image_position = value.clone();
                    cx.notify();
                }
            },
        )
        .detach();
        cx.subscribe_in(
            &background_image_fit_select,
            window,
            |this, _, ev: &SelectEvent<Vec<String>>, _, cx| {
                if let SelectEvent::Confirm(Some(value)) = ev {
                    this.config.appearance.background_image_fit = value.clone();
                    cx.notify();
                }
            },
        )
        .detach();

        Self {
            visible: false,
            config: config.clone(),
            registry,
            oauth_runtime,
            focus_handle: cx.focus_handle(),
            active_section: SettingsSection::General,
            overlay_motion: MotionValue::new(0.0),
            selected_provider: config.agent.provider.clone(),
            model_input,
            model_select,
            endpoint_preset_select,
            api_key_input,
            base_url_input,
            max_tokens_input,
            max_turns_input,
            temperature_input,
            auto_approve: config.agent.auto_approve_tools,
            suggestion_model_input,
            oauth_states: HashMap::new(),
            terminal_font_select,
            ui_font_select,
            font_size_input,
            terminal_opacity_slider,
            ui_opacity_slider,
            background_image_input,
            background_image_opacity_slider,
            background_image_position_select,
            background_image_fit_select,
            background_image_repeat: config.appearance.background_image_repeat,
            save_error: None,
            custom_theme_name_input,
            custom_theme_preview: None,
            custom_theme_status: None,
            recording_key: None,
        }
    }

    pub fn toggle(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.visible = !self.visible;
        self.overlay_motion.set_target(
            if self.visible { 1.0 } else { 0.0 },
            std::time::Duration::from_millis(if self.visible { 220 } else { 180 }),
        );
        if self.visible {
            let agent = &self.config.agent;
            self.selected_provider = agent.provider.clone();
            let pc = agent.providers.get_or_default(&self.selected_provider);
            self.load_provider_inputs(&pc, window, cx);
            self.sync_provider_placeholders(&self.selected_provider, window, cx);
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
            self.endpoint_preset_select =
                Self::make_endpoint_preset_select(&agent.provider, &pc.base_url, window, cx);
            self.terminal_font_select.update(cx, |select, cx| {
                select.set_selected_value(&self.config.terminal.font_family, window, cx);
            });
            self.ui_font_select.update(cx, |select, cx| {
                select.set_selected_value(&self.config.appearance.ui_font_family, window, cx);
            });
            self.font_size_input.update(cx, |s, cx| {
                s.set_value(&self.config.terminal.font_size.to_string(), window, cx)
            });
            self.terminal_opacity_slider.update(cx, |slider, cx| {
                slider.set_value(
                    Self::clamp_terminal_opacity(self.config.appearance.terminal_opacity),
                    window,
                    cx,
                );
            });
            self.ui_opacity_slider.update(cx, |slider, cx| {
                slider.set_value(
                    Self::clamp_ui_opacity(self.config.appearance.ui_opacity),
                    window,
                    cx,
                );
            });
            self.background_image_input.update(cx, |s, cx| {
                s.set_value(
                    &self
                        .config
                        .appearance
                        .background_image
                        .clone()
                        .unwrap_or_default(),
                    window,
                    cx,
                );
            });
            self.background_image_opacity_slider
                .update(cx, |slider, cx| {
                    slider.set_value(
                        Self::clamp_background_image_opacity(
                            self.config.appearance.background_image_opacity,
                        ),
                        window,
                        cx,
                    );
                });
            self.background_image_position_select
                .update(cx, |select, cx| {
                    select.set_selected_value(
                        &self.config.appearance.background_image_position,
                        window,
                        cx,
                    );
                });
            self.background_image_fit_select.update(cx, |select, cx| {
                select.set_selected_value(&self.config.appearance.background_image_fit, window, cx);
            });
            self.background_image_repeat = self.config.appearance.background_image_repeat;
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
        let endpoint_label =
            Self::endpoint_label_for_base_url(&self.selected_provider, pc.base_url.as_deref())
                .to_string();
        self.endpoint_preset_select.update(cx, |select, cx| {
            select.set_selected_value(&endpoint_label, window, cx);
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
        let selected_index = current_model
            .as_ref()
            .and_then(|m| models.iter().position(|item| item == m).map(IndexPath::new));
        let entity = cx.new(|cx| {
            SelectState::new(SearchableVec::new(models), selected_index, window, cx)
                .searchable(true)
        });
        cx.subscribe_in(
            &entity,
            window,
            |this, _, ev: &SelectEvent<SearchableVec<String>>, window, cx| {
                if let SelectEvent::Confirm(Some(value)) = ev {
                    this.model_input.update(cx, |s, cx| {
                        s.set_value(value, window, cx);
                    });
                    cx.notify();
                }
            },
        )
        .detach();
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

    fn browse_background_image(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let paths = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("Choose a background image".into()),
        });

        let input = self.background_image_input.clone();
        cx.spawn_in(window, async move |_, window| {
            let path = paths.await.ok()?.ok()??.into_iter().next()?;
            let path_text = path.to_string_lossy().to_string();

            window
                .update(|window, cx| {
                    _ = input.update(cx, |state, cx| {
                        state.set_value(&path_text, window, cx);
                    });
                })
                .ok()?;

            Some(())
        })
        .detach();
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
        self.visible || self.overlay_motion.is_animating()
    }

    fn save(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        let max_turns_text = self.max_turns_input.read(cx).value().to_string();
        let temperature_text = self.temperature_input.read(cx).value().to_string();
        let suggestion_model_text = self.suggestion_model_input.read(cx).value().to_string();
        let font_size_text = self.font_size_input.read(cx).value().to_string();

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
        self.config.appearance.terminal_opacity =
            Self::clamp_terminal_opacity(self.terminal_opacity_slider.read(cx).value().end());
        self.config.appearance.ui_opacity =
            Self::clamp_ui_opacity(self.ui_opacity_slider.read(cx).value().end());
        let background_image_text = self
            .background_image_input
            .read(cx)
            .value()
            .trim()
            .to_string();
        self.config.appearance.background_image = if background_image_text.is_empty() {
            None
        } else {
            Some(background_image_text)
        };
        self.config.appearance.background_image_opacity = Self::clamp_background_image_opacity(
            self.background_image_opacity_slider.read(cx).value().end(),
        );
        self.config.appearance.background_image_repeat = self.background_image_repeat;

        // Keybindings are updated directly via record_keystroke — no reading needed

        match self.persist_config() {
            Ok(()) => {
                self.save_error = None;
                self.visible = false;
                self.overlay_motion
                    .set_target(0.0, std::time::Duration::from_millis(180));
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
            "new_window" => self.config.keybindings.new_window = binding,
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
            "new_window" => &self.config.keybindings.new_window,
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
    pub fn appearance_config(&self) -> &con_core::config::AppearanceConfig {
        &self.config.appearance
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
        let provider = Self::sidebar_selection_target(&provider, &self.selected_provider);
        self.transition_provider(provider, window, cx);
    }

    fn transition_provider(
        &mut self,
        provider: ProviderKind,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let current_pc = self.read_provider_inputs(cx);
        self.config
            .agent
            .providers
            .set(&self.selected_provider, current_pc);

        if provider == self.selected_provider {
            cx.notify();
            return;
        }

        self.selected_provider = provider.clone();

        let pc = self.config.agent.providers.get_or_default(&provider);
        self.load_provider_inputs(&pc, window, cx);
        self.sync_provider_placeholders(&provider, window, cx);

        self.model_select =
            Self::make_model_select(&provider, &pc.model, &self.registry, window, cx);
        self.endpoint_preset_select =
            Self::make_endpoint_preset_select(&provider, &pc.base_url, window, cx);
        cx.notify();
    }

    fn toggle_selected_provider_protocol(
        &mut self,
        use_anthropic: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(provider) =
            Self::protocol_toggled_provider(&self.selected_provider, use_anthropic)
        else {
            return;
        };
        self.transition_provider(provider, window, cx);
    }

    // ── Section content ──────────────────────────────────────────

    fn render_general(&mut self, cx: &mut Context<Self>) -> Div {
        let card_opacity = self.card_opacity();

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
                (".agents/skills", "Agents"),
            ],
            cx,
        );
        let global_presets = self.render_path_presets(
            "global",
            &global_paths,
            &[
                ("~/.config/con/skills", "con"),
                ("~/.agents/skills", "Agents"),
            ],
            cx,
        );

        let theme = cx.theme();
        section_content(
            "General",
            "Terminal defaults, agent behavior, and skills.",
            theme,
        )
        .child(
            card(theme, card_opacity).child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .px(px(16.0))
                    .h(px(44.0))
                    .child(div().text_sm().child("Scrollback"))
                    .child(
                        div()
                            .text_size(px(11.0))
                            .text_color(theme.muted_foreground)
                            .child("Managed by Ghostty"),
                    ),
            ),
        )
        .child(
            div()
                .flex()
                .flex_col()
                .gap(px(8.0))
                .child(group_label("Agent", &theme))
                .child(
                    card(theme, card_opacity).child(
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
        // Skills paths
        .child(
            div()
                .flex()
                .flex_col()
                .gap(px(8.0))
                .child(group_label("Skills", &theme))
                .child(
                    card(theme, card_opacity)
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
        let terminal_font_select = self.terminal_font_select.clone();
        let ui_font_select = self.ui_font_select.clone();
        let font_size_input = self.font_size_input.clone();
        let terminal_opacity_slider = self.terminal_opacity_slider.clone();
        let ui_opacity_slider = self.ui_opacity_slider.clone();
        let background_image_input = self.background_image_input.clone();
        let background_image_opacity_slider = self.background_image_opacity_slider.clone();
        let background_image_position_select = self.background_image_position_select.clone();
        let background_image_fit_select = self.background_image_fit_select.clone();
        let terminal_opacity = self.terminal_opacity_value();
        let ui_opacity = self.ui_opacity_value();
        let background_image_opacity = self.background_image_opacity_value();
        let card_opacity = self.card_opacity();
        let image_repeat_toggle = Switch::new("background-image-repeat")
            .checked(self.background_image_repeat)
            .small()
            .on_click(cx.listener(|this, checked: &bool, _, cx| {
                this.background_image_repeat = *checked;
                this.config.appearance.background_image_repeat = *checked;
                cx.notify();
            }));
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
        let browse_background_image_btn = Button::new("browse-background-image")
            .label("Browse…")
            .icon(Icon::default().path("phosphor/folder-open.svg"))
            .small()
            .ghost()
            .on_click(cx.listener(|this, _, window, cx| {
                this.browse_background_image(window, cx);
            }));
        let open_catalog_btn = Button::new("theme-catalog-link")
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
                    .child("Visit the community-maintained Ghostty styles site, choose a theme, then copy its Ghostty-format contents here."),
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
            import_section = import_section.child(div().pt(px(4.0)).child(preview));
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

        let mut content = section_content(
            "Appearance",
            "Shape the terminal theme and how much of the desktop shows through.",
            theme,
        );

        content = content.child(
            div()
                .flex()
                .flex_col()
                .gap(px(8.0))
                .child(group_label("Fonts", &theme))
                .child(
                    card(theme, card_opacity)
                        .child(searchable_select_row(
                            "Terminal Font",
                            "Used for terminal text and mono UI surfaces such as code blocks.",
                            &terminal_font_select,
                            theme,
                        ))
                        .child(row_separator(theme))
                        .child(searchable_select_row(
                            "UI Font",
                            "Used for settings, prose, and the rest of the non-terminal interface.",
                            &ui_font_select,
                            theme,
                        ))
                        .child(row_separator(theme))
                        .child(row_field("Terminal Size", &font_size_input)),
                ),
        );

        content = content.child(
            div()
                .flex()
                .flex_col()
                .gap(px(8.0))
                .child(group_label("Transparency", &theme))
                .child(
                    card(theme, card_opacity)
                        .child(slider_row(
                            "Terminal Glass",
                            "Controls the terminal surface. On macOS, a relaunch gives the cleanest result.",
                            &terminal_opacity_slider,
                            terminal_opacity,
                            theme,
                        ))
                        .child(row_separator(theme))
                        .child(slider_row(
                            "Window Chrome",
                            "Controls tabs, the agent panel, the input bar, and command surfaces.",
                            &ui_opacity_slider,
                            ui_opacity,
                            theme,
                        )),
                ),
        );

        content = content.child(
            div()
                .flex()
                .flex_col()
                .gap(px(8.0))
                .child(group_label("Background Image", &theme))
                .child(
                    card(theme, card_opacity)
                        .child(
                            div()
                                .px(px(16.0))
                                .pt(px(12.0))
                                .child(
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
                                                        .child("Image Path"),
                                                )
                                                .child(
                                                    div()
                                                        .text_size(px(10.5))
                                                        .line_height(px(16.0))
                                                        .text_color(
                                                            theme.muted_foreground.opacity(0.65),
                                                        )
                                                        .child(
                                                            "Choose a PNG or JPEG. The image is applied per terminal, so splits will repeat it.",
                                                        ),
                                                ),
                                        )
                                        .child(
                                            div()
                                                .flex()
                                                .items_center()
                                                .gap(px(8.0))
                                                .child(
                                                    div()
                                                        .flex_1()
                                                        .child(Input::new(&background_image_input)),
                                                )
                                                .child(browse_background_image_btn),
                                        ),
                                ),
                        )
                        .child(row_separator(theme))
                        .child(
                            div()
                                .px(px(16.0))
                                .child(
                                    select_row(
                                        "Fit",
                                        "Choose how the image fills the terminal.",
                                        &background_image_fit_select,
                                        theme,
                                    ),
                                ),
                        )
                        .child(row_separator(theme))
                        .child(
                            div()
                                .px(px(16.0))
                                .child(
                                    select_row(
                                        "Position",
                                        "Anchor the image when it does not fill the full surface.",
                                        &background_image_position_select,
                                        theme,
                                    ),
                                ),
                        )
                        .child(row_separator(theme))
                        .child(
                            toggle_row(
                                "Repeat",
                                "Tile the image if the fit leaves empty space around it.",
                                image_repeat_toggle,
                                theme,
                            ),
                        )
                        .child(row_separator(theme))
                        .child(slider_row(
                            "Image Strength",
                            "Blend the image more softly or let it come forward behind the terminal.",
                            &background_image_opacity_slider,
                            background_image_opacity,
                            theme,
                        ))
                        .child(row_separator(theme))
                        .child(
                            div()
                                .px(px(16.0))
                                .pb(px(12.0))
                                .text_size(px(11.0))
                                .line_height(px(16.0))
                                .text_color(theme.muted_foreground.opacity(0.65))
                                .child(
                                    "Ghostty renders the image per terminal, so splits will each draw their own copy.",
                                ),
                        ),
                ),
        );

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
                    .child("Built-in themes below. You can also import community-maintained Ghostty styles."),
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
        content = content.child(card(theme, card_opacity).child(theme_card_inner));
        content = content.child(card(theme, card_opacity).child(import_section));

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
            .font_family(theme.mono_font_family.clone())
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
        let card_opacity = self.card_opacity();
        let model_input = self.model_input.clone();
        let api_key_input = self.api_key_input.clone();
        let base_url_input = self.base_url_input.clone();
        let max_tokens_input = self.max_tokens_input.clone();
        let max_turns_input = self.max_turns_input.clone();
        let temperature_input = self.temperature_input.clone();
        let suggestion_model_input = self.suggestion_model_input.clone();
        let models = self.registry.models_for(&self.selected_provider);
        let model_select = self.model_select.clone();
        let endpoint_preset_select = self.endpoint_preset_select.clone();
        let endpoint_presets = Self::provider_endpoint_presets(&self.selected_provider);
        let protocol_switch_label = Self::protocol_switch_label(&self.selected_provider);
        let protocol_switch_hint = Self::protocol_switch_hint(&self.selected_provider);
        let anthropic_protocol_enabled = Self::uses_anthropic_protocol(&self.selected_provider);

        let mut provider_list = div().flex().flex_col();
        let active_sidebar_provider = Self::sidebar_provider_kind(&self.selected_provider);
        for provider in SIDEBAR_PROVIDERS.iter() {
            let is_selected = *provider == active_sidebar_provider;
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

        let oauth_state = self.oauth_state(&self.selected_provider).cloned();
        let has_key_override = !self.api_key_input.read(cx).value().is_empty();
        let connection_ready = if Self::provider_has_oauth(&self.selected_provider) {
            oauth_state
                .as_ref()
                .map(|state| state.connected || state.in_progress)
                .unwrap_or(false)
        } else {
            has_key_override
        };
        let connection_label = if Self::provider_has_oauth(&self.selected_provider) {
            match oauth_state.as_ref() {
                Some(state) if state.in_progress => "Signing In",
                Some(state) if state.connected => "Signed In",
                _ => "OAuth",
            }
        } else if has_key_override {
            "Ready"
        } else {
            "No key"
        };

        // ── Model card — Select dropdown for known providers, text input for custom ──
        let model_card_content = card(theme, card_opacity).child(
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
                                .child(div().size(px(6.0)).rounded_full().bg(if connection_ready {
                                    theme.success
                                } else {
                                    theme.muted_foreground.opacity(0.2)
                                }))
                                .child(
                                    div()
                                        .text_size(px(10.0))
                                        .text_color(theme.muted_foreground.opacity(0.45))
                                        .child(connection_label),
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
                card(theme, card_opacity).child(
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
                        .children(protocol_switch_label.map(|switch_label| {
                            div()
                                .flex()
                                .flex_col()
                                .gap(px(8.0))
                                .child(
                                    div()
                                        .flex()
                                        .items_center()
                                        .justify_between()
                                        .gap(px(12.0))
                                        .child(
                                            div()
                                                .flex()
                                                .flex_col()
                                                .gap(px(2.0))
                                                .child(div().text_sm().child("Protocol"))
                                                .child(
                                                    div()
                                                        .text_size(px(11.0))
                                                        .text_color(theme.muted_foreground)
                                                        .child(protocol_switch_hint.unwrap_or("Choose the provider transport.")),
                                                ),
                                        )
                                        .child(
                                            Switch::new(format!(
                                                "provider-protocol-{}",
                                                provider_label(&Self::sidebar_provider_kind(&self.selected_provider))
                                            ))
                                            .checked(anthropic_protocol_enabled)
                                            .label(switch_label)
                                            .on_click(cx.listener(|this, checked: &bool, window, cx| {
                                                this.toggle_selected_provider_protocol(*checked, window, cx);
                                            })),
                                        ),
                                )
                                .into_any_element()
                        }))
                        .children(if protocol_switch_label.is_some() {
                            Some(div().child(row_separator(theme)))
                        } else {
                            None
                        })
                        .children(Self::provider_oauth_label(&self.selected_provider).map(|provider_name| {
                            let oauth = oauth_state.clone().unwrap_or_default();
                            let provider_for_click = self.selected_provider.clone();
                            let prompt = oauth.prompt.clone();
                            div()
                                .flex()
                                .flex_col()
                                .gap(px(10.0))
                                .child(
                                    div()
                                        .flex()
                                        .items_center()
                                        .justify_between()
                                        .gap(px(12.0))
                                        .child(
                                            div()
                                                .flex()
                                                .flex_col()
                                                .gap(px(2.0))
                                                .child(div().text_sm().child(format!("{provider_name} OAuth")))
                                                .child(
                                                    div()
                                                        .text_size(px(11.0))
                                                        .text_color(theme.muted_foreground)
                                                        .child("Use device login to authorize this subscription inside Con."),
                                                ),
                                        )
                                        .child(
                                            Button::new(format!("oauth-connect-{}", provider_label(&self.selected_provider)))
                                                .label(
                                                    if oauth.connected {
                                                        "Reconnect"
                                                    } else {
                                                        Self::provider_oauth_button_label(&self.selected_provider).unwrap_or("Sign In")
                                                    }
                                                )
                                                .small()
                                                .primary()
                                                .loading(oauth.in_progress)
                                                .disabled(oauth.in_progress)
                                                .on_click(cx.listener(move |this, _, window, cx| {
                                                    this.start_provider_oauth(provider_for_click.clone(), window, cx);
                                                })),
                                        ),
                                )
                                .children(oauth.status_message.as_ref().map(|message| {
                                    div()
                                        .text_size(px(11.0))
                                        .text_color(theme.muted_foreground)
                                        .child(message.clone())
                                }))
                                .children(oauth.error_message.as_ref().map(|message| {
                                    div()
                                        .text_size(px(11.0))
                                        .text_color(theme.danger)
                                        .child(message.clone())
                                }))
                                .children(prompt.map(|prompt| {
                                    div()
                                        .flex()
                                        .flex_col()
                                        .gap(px(8.0))
                                        .p(px(10.0))
                                        .rounded(px(8.0))
                                        .bg(theme.muted.opacity(0.05))
                                        .child(
                                            div()
                                                .text_size(px(11.0))
                                                .text_color(theme.muted_foreground)
                                                .child("Browser authorization"),
                                        )
                                        .child(
                                            div()
                                                .flex()
                                                .items_center()
                                                .justify_between()
                                                .gap(px(8.0))
                                                .child(
                                                    div()
                                                        .flex()
                                                        .flex_col()
                                                        .gap(px(2.0))
                                                        .child(div().text_size(px(11.0)).text_color(theme.muted_foreground).child("Code"))
                                                        .child(
                                                            div()
                                                                .font_weight(FontWeight::SEMIBOLD)
                                                                .child(prompt.user_code.clone()),
                                                        ),
                                                )
                                                .child(Clipboard::new(format!(
                                                    "oauth-code-{}",
                                                    provider_label(&self.selected_provider)
                                                ))
                                                .value(SharedString::from(prompt.user_code.clone()))),
                                        )
                                        .child(
                                            div()
                                                .flex()
                                                .items_center()
                                                .justify_between()
                                                .gap(px(8.0))
                                                .child(
                                                    div()
                                                        .flex_1()
                                                        .text_size(px(11.0))
                                                        .text_color(theme.muted_foreground)
                                                        .child(prompt.verification_uri.clone()),
                                                )
                                                .child(
                                                    Button::new(format!(
                                                        "oauth-open-{}",
                                                        provider_label(&self.selected_provider)
                                                    ))
                                                    .label("Open Browser")
                                                    .small()
                                                    .ghost()
                                                    .on_click(move |_, _, cx| {
                                                        cx.open_url(&prompt.verification_uri);
                                                    }),
                                                ),
                                        )
                                }))
                                .into_any_element()
                        }))
                        .children(if Self::provider_has_oauth(&self.selected_provider) {
                            Some(div().child(row_separator(theme)))
                        } else {
                            None
                        })
                        .child(stacked_input_field(
                            Self::provider_api_key_label(&self.selected_provider),
                            Self::provider_api_key_hint(&self.selected_provider),
                            &api_key_input,
                            theme,
                        ))
                        .child(stacked_input_field(
                            "Base URL",
                            Self::provider_base_url_hint(&self.selected_provider),
                            &base_url_input,
                            theme,
                        ))
                        .children(if endpoint_presets.is_empty() {
                            None
                        } else {
                            Some(
                                div().child(select_row(
                                    "Endpoint Preset",
                                    "Use an explicit regional or protocol endpoint, or keep the provider default.",
                                    &endpoint_preset_select,
                                    theme,
                                )),
                            )
                        }),
                ),
            )
            .child(
                card(theme, card_opacity).child(
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
                card(theme, card_opacity).child(div().px(px(4.0)).py(px(4.0)).child(provider_list)),
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
        let card_opacity = self.card_opacity();

        // Editable keybinding definitions: (label, field_name)
        let general_keys: &[(&str, &str)] = &[
            ("New Window", "new_window"),
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
            let mut c = card(theme, card_opacity);
            for (i, (label, field)) in keys.iter().enumerate() {
                if i > 0 {
                    c = c.child(row_separator(theme));
                }
                let value = this.binding_value(field).to_string();
                let is_recording = recording.as_deref() == Some(*field);
                let badge = if is_recording {
                    div()
                        .text_size(px(11.5))
                        .font_weight(FontWeight::MEDIUM)
                        .child("Press shortcut...")
                        .into_any_element()
                } else if let Ok(stroke) = Keystroke::parse(&value) {
                    Kbd::new(stroke).outline().into_any_element()
                } else {
                    div()
                        .text_size(px(11.5))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(theme.muted_foreground)
                        .child(value.clone())
                        .into_any_element()
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
                                .min_h(px(24.0))
                                .px(px(6.0))
                                .flex()
                                .items_center()
                                .rounded(px(5.0))
                                .cursor_pointer()
                                .bg(if is_recording {
                                    theme.primary.opacity(0.12)
                                } else {
                                    theme.transparent
                                })
                                .text_color(if is_recording {
                                    theme.primary
                                } else {
                                    theme.muted_foreground
                                })
                                .hover(|s| s.bg(theme.muted.opacity(0.08)))
                                .on_mouse_down(
                                    MouseButton::Left,
                                    cx.listener(move |this, _, _, cx| {
                                        this.recording_key = Some(field_str.clone());
                                        cx.notify();
                                    }),
                                )
                                .child(badge),
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
                .child(card(theme, card_opacity).child(key_row("Close Pane", "ctrl-d", theme))),
        )
        .child(
            div()
                .flex()
                .flex_col()
                .gap(px(8.0))
                .child(group_label("Terminal", &theme))
                .child(
                    card(theme, card_opacity)
                        .child(key_row("Copy", "cmd-c", theme))
                        .child(row_separator(theme))
                        .child(key_row("Paste", "cmd-v", theme))
                        .child(row_separator(theme))
                        .child(key_row("Select All", "cmd-a", theme)),
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
        let overlay_progress = self.overlay_motion.value(window);
        if overlay_progress <= 0.001 && !self.visible {
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
                nav_item = nav_item.justify_center().size(px(36.0)).mx_auto().child(
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
            .bg(theme.background.opacity(0.6 * overlay_progress))
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
            .opacity(overlay_progress)
            .child(
                div()
                    .pt(vertical_reveal_offset(overlay_progress, 18.0))
                    .px(px(0.0))
                    .opacity(overlay_progress)
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
                                                    .hover(|s| {
                                                        s.text_color(
                                                            theme.muted_foreground.opacity(0.7),
                                                        )
                                                    })
                                                    .on_mouse_down(
                                                        MouseButton::Left,
                                                        cx.listener(|_, _, _, cx| {
                                                            let path = Config::config_path();
                                                            // Ensure the file exists so the editor has something to open
                                                            if !path.exists() {
                                                                if let Some(parent) = path.parent()
                                                                {
                                                                    let _ = std::fs::create_dir_all(
                                                                        parent,
                                                                    );
                                                                }
                                                                let _ = std::fs::write(&path, "");
                                                            }
                                                            cx.open_url(&format!(
                                                                "file://{}",
                                                                path.display()
                                                            ));
                                                        }),
                                                    )
                                                    .child(
                                                        svg()
                                                            .path("phosphor/file-text.svg")
                                                            .size(px(12.0))
                                                            .text_color(
                                                                theme.muted_foreground.opacity(0.4),
                                                            ),
                                                    )
                                                    .child("config.toml"),
                                            ),
                                    ),
                            )
                            .child(div().h(px(1.0)).bg(theme.muted.opacity(0.10))),
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
                    ),
            );

        div()
            .id("settings-overlay")
            .absolute()
            .size_full()
            .font_family(theme.font_family.clone())
            .child(backdrop)
            .child(card)
    }
}

// ── Reusable building blocks ──────────────────────────────────────

fn section_content(title: &str, subtitle: &str, theme: &gpui_component::Theme) -> Div {
    div().flex().flex_col().gap(px(20.0)).child(
        div()
            .flex()
            .flex_col()
            .gap(px(6.0))
            .child(
                div()
                    .text_size(px(19.0))
                    .line_height(px(24.0))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(title.to_string()),
            )
            .child(
                div()
                    .max_w(px(520.0))
                    .text_size(px(12.0))
                    .line_height(px(19.0))
                    .text_color(theme.muted_foreground.opacity(0.68))
                    .child(subtitle.to_string()),
            ),
    )
}

fn group_label(text: &str, theme: &gpui_component::Theme) -> Div {
    div()
        .text_size(px(10.0))
        .font_weight(FontWeight::MEDIUM)
        .text_color(theme.muted_foreground.opacity(0.5))
        .px(px(2.0))
        .pb(px(2.0))
        .child(text.to_string())
}

fn card(theme: &gpui_component::Theme, opacity: f32) -> Div {
    div()
        .flex()
        .flex_col()
        .rounded(px(12.0))
        .overflow_hidden()
        .bg(theme.background.opacity(opacity.clamp(0.35, 0.98)))
}

fn row_separator(_theme: &gpui_component::Theme) -> Div {
    div().h(px(6.0))
}

fn row_field(label: &str, input: &Entity<InputState>) -> Div {
    div()
        .flex()
        .items_center()
        .justify_between()
        .gap(px(16.0))
        .px(px(16.0))
        .h(px(46.0))
        .child(
            div()
                .text_sm()
                .font_weight(FontWeight::MEDIUM)
                .flex_shrink_0()
                .child(label.to_string()),
        )
        .child(div().flex_1().min_w(px(160.0)).child(Input::new(input)))
}

fn slider_row(
    label: &str,
    hint: &str,
    slider: &Entity<SliderState>,
    value: f32,
    theme: &gpui_component::Theme,
) -> Div {
    div()
        .flex()
        .flex_col()
        .gap(px(12.0))
        .px(px(16.0))
        .py(px(12.0))
        .child(
            div()
                .flex()
                .items_start()
                .justify_between()
                .gap(px(16.0))
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(3.0))
                        .flex_1()
                        .max_w(px(380.0))
                        .child(
                            div()
                                .text_sm()
                                .font_weight(FontWeight::MEDIUM)
                                .child(label.to_string()),
                        )
                        .child(
                            div()
                                .text_size(px(11.5))
                                .line_height(px(17.0))
                                .text_color(theme.muted_foreground.opacity(0.65))
                                .child(hint.to_string()),
                        ),
                )
                .child(
                    div()
                        .flex_shrink_0()
                        .min_w(px(58.0))
                        .px(px(8.0))
                        .py(px(4.0))
                        .rounded(px(999.0))
                        .bg(theme.muted.opacity(0.10))
                        .text_size(px(11.0))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_align(TextAlign::Center)
                        .text_color(theme.foreground)
                        .child(format!("{:.0}%", value * 100.0)),
                ),
        )
        .child(
            div()
                .flex()
                .items_center()
                .w_full()
                .child(Slider::new(slider).w_full()),
        )
}

fn searchable_select_row(
    label: &str,
    hint: &str,
    select: &Entity<SelectState<SearchableVec<String>>>,
    theme: &gpui_component::Theme,
) -> Div {
    div()
        .flex()
        .items_start()
        .justify_between()
        .gap(px(16.0))
        .px(px(16.0))
        .py(px(12.0))
        .child(
            div()
                .flex()
                .flex_col()
                .gap(px(3.0))
                .flex_1()
                .max_w(px(340.0))
                .child(
                    div()
                        .text_sm()
                        .font_weight(FontWeight::MEDIUM)
                        .child(label.to_string()),
                )
                .child(
                    div()
                        .text_size(px(11.5))
                        .line_height(px(17.0))
                        .text_color(theme.muted_foreground.opacity(0.65))
                        .child(hint.to_string()),
                ),
        )
        .child(
            div()
                .w(px(236.0))
                .flex_shrink_0()
                .child(Select::new(select).placeholder("Search fonts…").small()),
        )
}

fn select_row(
    label: &str,
    hint: &str,
    select: &Entity<SelectState<Vec<String>>>,
    theme: &gpui_component::Theme,
) -> Div {
    div()
        .flex()
        .items_start()
        .justify_between()
        .gap(px(16.0))
        .py(px(12.0))
        .child(
            div()
                .flex()
                .flex_col()
                .gap(px(3.0))
                .flex_1()
                .max_w(px(320.0))
                .child(
                    div()
                        .text_sm()
                        .font_weight(FontWeight::MEDIUM)
                        .child(label.to_string()),
                )
                .child(
                    div()
                        .text_size(px(11.5))
                        .line_height(px(17.0))
                        .text_color(theme.muted_foreground.opacity(0.65))
                        .child(hint.to_string()),
                ),
        )
        .child(
            div()
                .w(px(188.0))
                .flex_shrink_0()
                .child(Select::new(select).small()),
        )
}

fn toggle_row(label: &str, hint: &str, toggle: Switch, theme: &gpui_component::Theme) -> Div {
    div()
        .flex()
        .items_start()
        .justify_between()
        .gap(px(16.0))
        .px(px(16.0))
        .py(px(12.0))
        .child(
            div()
                .flex()
                .flex_col()
                .gap(px(3.0))
                .flex_1()
                .max_w(px(360.0))
                .child(
                    div()
                        .text_sm()
                        .font_weight(FontWeight::MEDIUM)
                        .child(label.to_string()),
                )
                .child(
                    div()
                        .text_size(px(11.5))
                        .line_height(px(17.0))
                        .text_color(theme.muted_foreground.opacity(0.65))
                        .child(hint.to_string()),
                ),
        )
        .child(div().pt(px(2.0)).child(toggle))
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

fn key_row(action: &str, shortcut: &str, theme: &gpui_component::Theme) -> Div {
    div()
        .flex()
        .items_center()
        .justify_between()
        .px(px(16.0))
        .h(px(36.0))
        .child(div().text_sm().child(action.to_string()))
        .child(if let Ok(stroke) = Keystroke::parse(shortcut) {
            Kbd::new(stroke).outline().into_any_element()
        } else {
            div()
                .text_size(px(11.0))
                .text_color(theme.muted_foreground)
                .child(shortcut.to_string())
                .into_any_element()
        })
}

fn provider_label(provider: &ProviderKind) -> &'static str {
    match provider {
        ProviderKind::Anthropic => "Anthropic",
        ProviderKind::OpenAI => "OpenAI",
        ProviderKind::ChatGPT => "ChatGPT Subscription",
        ProviderKind::GitHubCopilot => "GitHub Copilot",
        ProviderKind::OpenAICompatible => "OpenAI Compatible",
        ProviderKind::MiniMax => "MiniMax",
        ProviderKind::MiniMaxAnthropic => "MiniMax / Anthropic",
        ProviderKind::Moonshot => "Moonshot",
        ProviderKind::MoonshotAnthropic => "Moonshot / Anthropic",
        ProviderKind::ZAI => "Z.AI",
        ProviderKind::ZAIAnthropic => "Z.AI / Anthropic",
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
        "paper-light" => "Paper Light".into(),
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
