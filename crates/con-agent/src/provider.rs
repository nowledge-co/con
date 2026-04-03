use anyhow::Result;
use crossbeam_channel::Sender;
use futures::StreamExt;
use rig::agent::{MultiTurnStreamItem, StreamingResult};
use rig::client::CompletionClient;
use rig::client::Nothing;
use rig::completion::CompletionModel as _;
use rig::providers::{
    anthropic, cohere, deepseek, gemini, groq, mistral, ollama, openai, openrouter, perplexity,
    together, xai,
};
use rig::streaming::{StreamedAssistantContent, StreamingPrompt};
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use crate::context::TerminalContext;
use crate::conversation::{AgentStep, Conversation, Message};
use crate::hook::{ConHook, ToolApprovalDecision};
use crate::tools::{
    BatchExecTool, EditFileTool, FileReadTool, FileWriteTool, ListFilesTool, ListPanesTool,
    PaneRequest, ReadPaneTool, SearchPanesTool, SearchTool, SendKeysTool, ShellExecTool,
    TerminalExecRequest, TerminalExecTool,
};

// ── Provider enum ───────────────────────────────────────────────────

/// Supported LLM providers.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum ProviderKind {
    Anthropic,
    OpenAI,
    #[serde(alias = "openai-compatible")]
    OpenAICompatible,
    DeepSeek,
    Groq,
    Cohere,
    Gemini,
    Ollama,
    OpenRouter,
    Perplexity,
    Mistral,
    Together,
    XAI,
}

impl Default for ProviderKind {
    fn default() -> Self {
        Self::Anthropic
    }
}

impl std::fmt::Display for ProviderKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Anthropic => write!(f, "anthropic"),
            Self::OpenAI => write!(f, "openai"),
            Self::OpenAICompatible => write!(f, "openai-compatible"),
            Self::DeepSeek => write!(f, "deepseek"),
            Self::Groq => write!(f, "groq"),
            Self::Cohere => write!(f, "cohere"),
            Self::Gemini => write!(f, "gemini"),
            Self::Ollama => write!(f, "ollama"),
            Self::OpenRouter => write!(f, "openrouter"),
            Self::Perplexity => write!(f, "perplexity"),
            Self::Mistral => write!(f, "mistral"),
            Self::Together => write!(f, "together"),
            Self::XAI => write!(f, "xai"),
        }
    }
}

impl ProviderKind {
    pub fn default_api_key_env(&self) -> &str {
        match self {
            Self::Anthropic => "ANTHROPIC_API_KEY",
            Self::OpenAICompatible => "OPENAI_API_KEY",
            Self::OpenAI => "OPENAI_API_KEY",
            Self::DeepSeek => "DEEPSEEK_API_KEY",
            Self::Groq => "GROQ_API_KEY",
            Self::Cohere => "COHERE_API_KEY",
            Self::Gemini => "GEMINI_API_KEY",
            Self::Ollama => "OLLAMA_API_KEY",
            Self::OpenRouter => "OPENROUTER_API_KEY",
            Self::Perplexity => "PERPLEXITY_API_KEY",
            Self::Mistral => "MISTRAL_API_KEY",
            Self::Together => "TOGETHER_API_KEY",
            Self::XAI => "XAI_API_KEY",
        }
    }

    pub fn default_model(&self) -> &str {
        match self {
            Self::Anthropic => "claude-sonnet-4-6",
            Self::OpenAICompatible => "gpt-4o",
            Self::OpenAI => "gpt-4o",
            Self::DeepSeek => "deepseek-chat",
            Self::Groq => "llama-3.3-70b-versatile",
            Self::Cohere => "command-a-03-2025",
            Self::Gemini => "gemini-2.5-flash",
            Self::Ollama => "llama3.2",
            Self::OpenRouter => "anthropic/claude-sonnet-4-6",
            Self::Perplexity => "sonar-pro",
            Self::Mistral => "mistral-large-latest",
            Self::Together => "meta-llama/Llama-3.3-70B-Instruct-Turbo",
            Self::XAI => "grok-3",
        }
    }
}

// ── Per-provider config ─────────────────────────────────────────────

/// Settings specific to a single provider — model, credentials, endpoint.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ProviderConfig {
    pub model: Option<String>,
    /// Direct API key value.
    pub api_key: Option<String>,
    /// Environment variable name containing the API key.
    pub api_key_env: Option<String>,
    /// Custom base URL override (most providers have sensible defaults in Rig).
    pub base_url: Option<String>,
    /// Max output tokens (provider-specific limits apply).
    pub max_tokens: Option<u64>,
}

/// Map of per-provider configurations.
/// Explicit fields (not HashMap) for clean TOML serialization.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ProviderMap {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub anthropic: Option<ProviderConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub openai: Option<ProviderConfig>,
    #[serde(skip_serializing_if = "Option::is_none", alias = "openai-compatible")]
    pub openaicompatible: Option<ProviderConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub deepseek: Option<ProviderConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub groq: Option<ProviderConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cohere: Option<ProviderConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gemini: Option<ProviderConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ollama: Option<ProviderConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub openrouter: Option<ProviderConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub perplexity: Option<ProviderConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mistral: Option<ProviderConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub together: Option<ProviderConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub xai: Option<ProviderConfig>,
}

impl ProviderMap {
    pub fn get(&self, kind: &ProviderKind) -> Option<&ProviderConfig> {
        match kind {
            ProviderKind::Anthropic => self.anthropic.as_ref(),
            ProviderKind::OpenAI => self.openai.as_ref(),
            ProviderKind::OpenAICompatible => self.openaicompatible.as_ref(),
            ProviderKind::DeepSeek => self.deepseek.as_ref(),
            ProviderKind::Groq => self.groq.as_ref(),
            ProviderKind::Cohere => self.cohere.as_ref(),
            ProviderKind::Gemini => self.gemini.as_ref(),
            ProviderKind::Ollama => self.ollama.as_ref(),
            ProviderKind::OpenRouter => self.openrouter.as_ref(),
            ProviderKind::Perplexity => self.perplexity.as_ref(),
            ProviderKind::Mistral => self.mistral.as_ref(),
            ProviderKind::Together => self.together.as_ref(),
            ProviderKind::XAI => self.xai.as_ref(),
        }
    }

    pub fn get_or_default(&self, kind: &ProviderKind) -> ProviderConfig {
        self.get(kind).cloned().unwrap_or_default()
    }

    pub fn set(&mut self, kind: &ProviderKind, config: ProviderConfig) {
        let slot = match kind {
            ProviderKind::Anthropic => &mut self.anthropic,
            ProviderKind::OpenAI => &mut self.openai,
            ProviderKind::OpenAICompatible => &mut self.openaicompatible,
            ProviderKind::DeepSeek => &mut self.deepseek,
            ProviderKind::Groq => &mut self.groq,
            ProviderKind::Cohere => &mut self.cohere,
            ProviderKind::Gemini => &mut self.gemini,
            ProviderKind::Ollama => &mut self.ollama,
            ProviderKind::OpenRouter => &mut self.openrouter,
            ProviderKind::Perplexity => &mut self.perplexity,
            ProviderKind::Mistral => &mut self.mistral,
            ProviderKind::Together => &mut self.together,
            ProviderKind::XAI => &mut self.xai,
        };
        *slot = Some(config);
    }
}

// ── Config ──────────────────────────────────────────────────────────

/// Optional overrides for the inline suggestion model.
/// API key and base_url are inherited from the provider's entry in `providers`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct SuggestionModelConfig {
    pub provider: Option<ProviderKind>,
    pub model: Option<String>,
}

/// Agent configuration from config.toml
///
/// ```toml
/// [agent]
/// provider = "anthropic"
/// max_turns = 10
/// temperature = 0.7
///
/// [agent.providers.anthropic]
/// model = "claude-sonnet-4-6"
/// api_key = "sk-ant-..."
/// max_tokens = 8192
///
/// [agent.providers.groq]
/// model = "llama-3.3-70b-versatile"
/// api_key_env = "GROQ_API_KEY"
/// max_tokens = 16384
///
/// [agent.suggestion_model]
/// provider = "groq"
/// model = "llama-3.1-8b-instant"
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AgentConfig {
    /// Active provider selection.
    pub provider: ProviderKind,
    /// Per-provider settings (model, key, endpoint, max_tokens).
    pub providers: ProviderMap,
    /// Global: max agent turns per request.
    pub max_turns: usize,
    /// Global: sampling temperature (applied to all providers).
    pub temperature: Option<f64>,
    pub auto_context: bool,
    pub auto_approve_tools: bool,
    pub suggestion_model: SuggestionModelConfig,

    // Legacy flat fields — deserialize from old configs, never written back.
    #[serde(default, skip_serializing)]
    model: Option<String>,
    #[serde(default, skip_serializing)]
    api_key: Option<String>,
    #[serde(default, skip_serializing)]
    api_key_env: Option<String>,
    #[serde(default, skip_serializing)]
    base_url: Option<String>,
    #[serde(default, skip_serializing)]
    max_tokens: Option<u64>,
}

impl Default for AgentConfig {
    fn default() -> Self {
        Self {
            provider: ProviderKind::default(),
            providers: ProviderMap::default(),
            max_turns: 10,
            temperature: None,
            auto_context: true,
            auto_approve_tools: false,
            suggestion_model: SuggestionModelConfig::default(),
            // Legacy
            model: None,
            api_key: None,
            api_key_env: None,
            base_url: None,
            max_tokens: None,
        }
    }
}

impl AgentConfig {
    /// Migrate legacy flat fields into the per-provider map.
    /// Called once after deserialization. Safe to call multiple times.
    pub fn migrate_legacy(&mut self) {
        let has_legacy = self.model.is_some()
            || self.api_key.is_some()
            || self.api_key_env.is_some()
            || self.base_url.is_some()
            || self.max_tokens.is_some();

        if has_legacy && self.providers.get(&self.provider).is_none() {
            self.providers.set(
                &self.provider,
                ProviderConfig {
                    model: self.model.take(),
                    api_key: self.api_key.take(),
                    api_key_env: self.api_key_env.take(),
                    base_url: self.base_url.take(),
                    max_tokens: self.max_tokens.take(),
                },
            );
        }
    }

    /// Effective model for the given provider.
    pub fn effective_model<'a>(&'a self, kind: &'a ProviderKind) -> &'a str {
        self.providers
            .get(kind)
            .and_then(|p| p.model.as_deref())
            .unwrap_or_else(move || kind.default_model())
    }

    /// Effective base URL override for the given provider.
    pub fn effective_base_url(&self, kind: &ProviderKind) -> Option<&str> {
        self.providers
            .get(kind)
            .and_then(|p| p.base_url.as_deref())
    }

    /// Effective max tokens for the given provider.
    pub fn effective_max_tokens(&self, kind: &ProviderKind) -> Option<u64> {
        self.providers.get(kind).and_then(|p| p.max_tokens)
    }

    /// Build a lightweight config for inline suggestions.
    /// Uses the suggestion provider's credentials from the providers map.
    pub fn suggestion_agent_config(&self) -> AgentConfig {
        let suggestion_provider = self
            .suggestion_model
            .provider
            .clone()
            .unwrap_or_else(|| self.provider.clone());

        // Build a minimal providers map with just the suggestion provider's config
        let mut providers = ProviderMap::default();
        if let Some(pc) = self.providers.get(&suggestion_provider) {
            let mut pc = pc.clone();
            // Override model if suggestion_model specifies one
            if let Some(ref model) = self.suggestion_model.model {
                pc.model = Some(model.clone());
            }
            pc.max_tokens = Some(100);
            providers.set(&suggestion_provider, pc);
        } else if let Some(ref model) = self.suggestion_model.model {
            providers.set(
                &suggestion_provider,
                ProviderConfig {
                    model: Some(model.clone()),
                    max_tokens: Some(100),
                    ..Default::default()
                },
            );
        }

        AgentConfig {
            provider: suggestion_provider,
            providers,
            max_turns: 1,
            temperature: Some(0.0),
            auto_context: false,
            auto_approve_tools: false,
            suggestion_model: SuggestionModelConfig::default(),
            // No legacy
            model: None,
            api_key: None,
            api_key_env: None,
            base_url: None,
            max_tokens: None,
        }
    }
}

// ── Events ──────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub enum AgentEvent {
    Thinking,
    /// Incremental extended thinking/reasoning text from the model
    ThinkingDelta(String),
    Token(String),
    Step(AgentStep),
    ToolCallStart {
        call_id: String,
        tool_name: String,
        args: String,
    },
    ToolCallComplete {
        call_id: String,
        tool_name: String,
        result: String,
    },
    Done(Message),
    Error(String),
}

// ── Agent builder macro ─────────────────────────────────────────────

/// Build an agent with all tools, stream the response, and consume it.
///
/// Rig's `CompletionClient` trait has associated types (`CompletionModel`,
/// `StreamingResponse`) whose lifetime bounds prevent a clean generic function.
/// This macro expands identically at each call site, giving the compiler
/// concrete types while keeping tool registration in one place.
macro_rules! build_and_stream {
    ($client:expr, $cfg:expr, $kind:expr, $system_prompt:expr, $prompt:expr,
     $history:expr, $hook:expr, $terminal_exec_tx:expr, $pane_tx:expr,
     $event_tx:expr, $cancelled:expr, $workspace_root:expr) => {{
        let root: std::path::PathBuf = $workspace_root;
        let mut builder = $client
            .agent($cfg.effective_model(&$kind))
            .preamble($system_prompt)
            .tool(TerminalExecTool::new($terminal_exec_tx.clone()))
            .tool(ShellExecTool)
            .tool(FileReadTool::new(root.clone()))
            .tool(FileWriteTool::new(root.clone()))
            .tool(EditFileTool::new(root.clone()))
            .tool(ListFilesTool::new(root.clone()))
            .tool(SearchTool::new(root))
            .tool(ListPanesTool::new($pane_tx.clone()))
            .tool(ReadPaneTool::new($pane_tx.clone()))
            .tool(SendKeysTool::new($pane_tx.clone()))
            .tool(SearchPanesTool::new($pane_tx))
            .tool(BatchExecTool::new($terminal_exec_tx))
            .default_max_turns($cfg.max_turns);

        if let Some(max_tokens) = $cfg.effective_max_tokens(&$kind) {
            builder = builder.max_tokens(max_tokens);
        }
        if let Some(temp) = $cfg.temperature {
            builder = builder.temperature(temp);
        }

        let agent = builder.build();

        // Diagnostic: log registered tools by querying the tool server
        match agent.tool_server_handle.get_tool_defs(None).await {
            Ok(defs) => {
                let names: Vec<&str> = defs.iter().map(|d| d.name.as_str()).collect();
                log::info!(
                    "[agent] Registered {} tools for {:?}/{}: {:?}",
                    defs.len(),
                    $kind,
                    $cfg.effective_model(&$kind),
                    names,
                );
            }
            Err(e) => {
                log::error!("[agent] Failed to query tool definitions: {}", e);
            }
        }

        let stream = agent
            .stream_prompt($prompt)
            .with_hook($hook)
            .with_history($history)
            .await;

        consume_stream(stream, $event_tx, $cancelled).await
    }};
}

// ── Provider ────────────────────────────────────────────────────────

pub struct AgentProvider {
    config: AgentConfig,
}

impl AgentProvider {
    pub fn new(config: AgentConfig) -> Self {
        Self { config }
    }

    pub fn config(&self) -> &AgentConfig {
        &self.config
    }

    pub async fn send(
        &self,
        conversation: &Conversation,
        context: &TerminalContext,
        event_tx: Sender<AgentEvent>,
        approval_rx: crossbeam_channel::Receiver<ToolApprovalDecision>,
        terminal_exec_tx: crossbeam_channel::Sender<TerminalExecRequest>,
        pane_tx: crossbeam_channel::Sender<PaneRequest>,
        cancelled: Arc<AtomicBool>,
    ) -> Result<Message> {
        let _ = event_tx.send(AgentEvent::Thinking);
        let kind = &self.config.provider;

        let system_prompt = context.to_system_prompt();
        let chat_history = conversation.to_rig_history();
        let last_user_msg = conversation
            .last_user_message()
            .map(|m| m.content.clone())
            .unwrap_or_default();

        log::info!(
            "[agent] Sending to {:?}/{}: system_prompt={} chars, history={} msgs, user_msg={} chars, panes={}",
            kind,
            self.config.effective_model(kind),
            system_prompt.len(),
            chat_history.len(),
            last_user_msg.len(),
            1 + context.other_panes.len(),
        );

        let _ = event_tx.send(AgentEvent::Step(AgentStep::Thinking(format!(
            "{}:{}",
            kind,
            self.config.effective_model(kind),
        ))));

        let hook = ConHook::new(
            event_tx.clone(),
            approval_rx,
            self.config.auto_approve_tools,
        );

        // Each provider dispatches to its native Rig client — exhaustive match
        // prevents silent misrouting.
        // Derive workspace root from terminal context cwd, falling back to $HOME or /tmp
        let workspace_root = context
            .cwd
            .as_ref()
            .map(std::path::PathBuf::from)
            .filter(|p| p.is_dir())
            .or_else(|| dirs::home_dir())
            .unwrap_or_else(|| std::path::PathBuf::from("/tmp"));

        macro_rules! stream_with {
            ($client:expr) => {
                build_and_stream!(
                    $client, self.config, kind, &system_prompt, &last_user_msg,
                    chat_history, hook, terminal_exec_tx, pane_tx, &event_tx, &cancelled,
                    workspace_root.clone()
                )?
            };
        }

        let response = match *kind {
            ProviderKind::Anthropic => stream_with!(self.build_anthropic_client()?),
            ProviderKind::OpenAI | ProviderKind::OpenAICompatible => {
                stream_with!(self.build_openai_client()?)
            }
            ProviderKind::DeepSeek => stream_with!(self.build_deepseek_client()?),
            ProviderKind::Groq => stream_with!(self.build_groq_client()?),
            ProviderKind::Cohere => stream_with!(self.build_cohere_client()?),
            ProviderKind::Gemini => stream_with!(self.build_gemini_client()?),
            ProviderKind::Ollama => stream_with!(self.build_ollama_client()?),
            ProviderKind::OpenRouter => stream_with!(self.build_openrouter_client()?),
            ProviderKind::Perplexity => stream_with!(self.build_perplexity_client()?),
            ProviderKind::Mistral => stream_with!(self.build_mistral_client()?),
            ProviderKind::Together => stream_with!(self.build_together_client()?),
            ProviderKind::XAI => stream_with!(self.build_xai_client()?),
        };

        let message = Message::assistant(&response);
        let _ = event_tx.send(AgentEvent::Done(message.clone()));

        Ok(message)
    }

    // ── Client builders ──────────────────────────────────────────
    //
    // Each provider uses its native Rig client with per-provider config
    // from the providers map. Tool registration is centralized via the
    // `build_and_stream!` macro above.

    fn build_anthropic_client(&self) -> Result<anthropic::Client> {
        let api_key = self.resolve_api_key(&ProviderKind::Anthropic)?;
        let mut builder = anthropic::Client::builder().api_key(&api_key);
        if let Some(url) = self.config.effective_base_url(&ProviderKind::Anthropic) {
            builder = builder.base_url(url);
        }
        builder
            .build()
            .map_err(|e| anyhow::anyhow!("Anthropic client error: {e}"))
    }

    fn build_openai_client(&self) -> Result<openai::Client> {
        let kind = &self.config.provider; // OpenAI or OpenAICompatible
        let api_key = self.resolve_api_key(kind)?;
        let mut builder = openai::Client::builder().api_key(&api_key);
        if let Some(url) = self.config.effective_base_url(kind) {
            builder = builder.base_url(url);
        }
        builder
            .build()
            .map_err(|e| anyhow::anyhow!("OpenAI client error: {e}"))
    }

    fn build_deepseek_client(&self) -> Result<deepseek::Client> {
        let api_key = self.resolve_api_key(&ProviderKind::DeepSeek)?;
        let mut builder = deepseek::Client::builder().api_key(&api_key);
        if let Some(url) = self.config.effective_base_url(&ProviderKind::DeepSeek) {
            builder = builder.base_url(url);
        }
        builder
            .build()
            .map_err(|e| anyhow::anyhow!("DeepSeek client error: {e}"))
    }

    fn build_groq_client(&self) -> Result<groq::Client> {
        let api_key = self.resolve_api_key(&ProviderKind::Groq)?;
        let mut builder = groq::Client::builder().api_key(&api_key);
        if let Some(url) = self.config.effective_base_url(&ProviderKind::Groq) {
            builder = builder.base_url(url);
        }
        builder
            .build()
            .map_err(|e| anyhow::anyhow!("Groq client error: {e}"))
    }

    fn build_cohere_client(&self) -> Result<cohere::Client> {
        let api_key = self.resolve_api_key(&ProviderKind::Cohere)?;
        let mut builder = cohere::Client::builder().api_key(&api_key);
        if let Some(url) = self.config.effective_base_url(&ProviderKind::Cohere) {
            builder = builder.base_url(url);
        }
        builder
            .build()
            .map_err(|e| anyhow::anyhow!("Cohere client error: {e}"))
    }

    fn build_gemini_client(&self) -> Result<gemini::Client> {
        let api_key = self.resolve_api_key(&ProviderKind::Gemini)?;
        let mut builder = gemini::Client::builder().api_key(&api_key);
        if let Some(url) = self.config.effective_base_url(&ProviderKind::Gemini) {
            builder = builder.base_url(url);
        }
        builder
            .build()
            .map_err(|e| anyhow::anyhow!("Gemini client error: {e}"))
    }

    fn build_ollama_client(&self) -> Result<ollama::Client> {
        let mut builder = ollama::Client::builder().api_key(Nothing);
        if let Some(url) = self.config.effective_base_url(&ProviderKind::Ollama) {
            builder = builder.base_url(url);
        }
        builder
            .build()
            .map_err(|e| anyhow::anyhow!("Ollama client error: {e}"))
    }

    fn build_openrouter_client(&self) -> Result<openrouter::Client> {
        let api_key = self.resolve_api_key(&ProviderKind::OpenRouter)?;
        let mut builder = openrouter::Client::builder().api_key(&api_key);
        if let Some(url) = self.config.effective_base_url(&ProviderKind::OpenRouter) {
            builder = builder.base_url(url);
        }
        builder
            .build()
            .map_err(|e| anyhow::anyhow!("OpenRouter client error: {e}"))
    }

    fn build_perplexity_client(&self) -> Result<perplexity::Client> {
        let api_key = self.resolve_api_key(&ProviderKind::Perplexity)?;
        let mut builder = perplexity::Client::builder().api_key(&api_key);
        if let Some(url) = self.config.effective_base_url(&ProviderKind::Perplexity) {
            builder = builder.base_url(url);
        }
        builder
            .build()
            .map_err(|e| anyhow::anyhow!("Perplexity client error: {e}"))
    }

    fn build_mistral_client(&self) -> Result<mistral::Client> {
        let api_key = self.resolve_api_key(&ProviderKind::Mistral)?;
        let mut builder = mistral::Client::builder().api_key(&api_key);
        if let Some(url) = self.config.effective_base_url(&ProviderKind::Mistral) {
            builder = builder.base_url(url);
        }
        builder
            .build()
            .map_err(|e| anyhow::anyhow!("Mistral client error: {e}"))
    }

    fn build_together_client(&self) -> Result<together::Client> {
        let api_key = self.resolve_api_key(&ProviderKind::Together)?;
        let mut builder = together::Client::builder().api_key(&api_key);
        if let Some(url) = self.config.effective_base_url(&ProviderKind::Together) {
            builder = builder.base_url(url);
        }
        builder
            .build()
            .map_err(|e| anyhow::anyhow!("Together client error: {e}"))
    }

    fn build_xai_client(&self) -> Result<xai::Client> {
        let api_key = self.resolve_api_key(&ProviderKind::XAI)?;
        let mut builder = xai::Client::builder().api_key(&api_key);
        if let Some(url) = self.config.effective_base_url(&ProviderKind::XAI) {
            builder = builder.base_url(url);
        }
        builder
            .build()
            .map_err(|e| anyhow::anyhow!("xAI client error: {e}"))
    }

    /// Lightweight completion — no tools, no history, just a simple prompt→response.
    /// Used for suggestions and other quick completions.
    pub async fn complete(&self, prompt: &str) -> Result<String> {
        use rig::completion::AssistantContent;

        let kind = &self.config.provider;
        let preamble = "You are a shell command completion assistant. Be extremely concise.";

        macro_rules! do_complete {
            ($client:expr) => {{
                let model = $client.completion_model(self.config.effective_model(kind));
                let response = model
                    .completion_request(prompt)
                    .preamble(preamble.to_string())
                    .max_tokens(100)
                    .send()
                    .await
                    .map_err(|e| anyhow::anyhow!("Completion error: {e}"))?;
                match response.choice.first() {
                    AssistantContent::Text(t) => Ok(t.text.clone()),
                    _ => Ok(String::new()),
                }
            }};
        }

        match *kind {
            ProviderKind::Anthropic => do_complete!(self.build_anthropic_client()?),
            ProviderKind::OpenAI | ProviderKind::OpenAICompatible => {
                do_complete!(self.build_openai_client()?)
            }
            ProviderKind::DeepSeek => do_complete!(self.build_deepseek_client()?),
            ProviderKind::Groq => do_complete!(self.build_groq_client()?),
            ProviderKind::Cohere => do_complete!(self.build_cohere_client()?),
            ProviderKind::Gemini => do_complete!(self.build_gemini_client()?),
            ProviderKind::Ollama => do_complete!(self.build_ollama_client()?),
            ProviderKind::OpenRouter => do_complete!(self.build_openrouter_client()?),
            ProviderKind::Perplexity => do_complete!(self.build_perplexity_client()?),
            ProviderKind::Mistral => do_complete!(self.build_mistral_client()?),
            ProviderKind::Together => do_complete!(self.build_together_client()?),
            ProviderKind::XAI => do_complete!(self.build_xai_client()?),
        }
    }

    /// Resolve API key for a specific provider from the providers map.
    fn resolve_api_key(&self, kind: &ProviderKind) -> Result<String> {
        let pc = self.config.providers.get(kind);

        // 1. Direct api_key from provider config
        if let Some(key) = pc.and_then(|p| p.api_key.as_ref()).filter(|k| !k.is_empty()) {
            return Ok(key.clone());
        }

        // 2. api_key_env — could be env var name or direct key (legacy compat)
        if let Some(key_or_env) = pc
            .and_then(|p| p.api_key_env.as_ref())
            .filter(|k| !k.is_empty())
        {
            let is_env_var_name = key_or_env
                .chars()
                .all(|c| c.is_ascii_uppercase() || c == '_' || c.is_ascii_digit());
            if is_env_var_name {
                if let Ok(val) = std::env::var(key_or_env) {
                    return Ok(val);
                }
            } else {
                return Ok(key_or_env.clone());
            }
        }

        // 3. Fall back to provider's default env var
        let default_env = kind.default_api_key_env();
        if *kind == ProviderKind::Ollama {
            return Ok(std::env::var(default_env).unwrap_or_else(|_| "ollama".into()));
        }
        std::env::var(default_env).map_err(|_| {
            anyhow::anyhow!(
                "No API key found for {}. Set {} or configure api_key in settings.",
                kind,
                default_env
            )
        })
    }
}

/// Consume a streaming response, accumulating the full text.
/// Hook callbacks (on_text_delta, on_tool_call, on_tool_result) fire
/// as the stream is consumed — this function just collects the result.
///
/// Emits `ThinkingDelta` events for extended thinking/reasoning blocks,
/// allowing the UI to display the model's reasoning process.
///
/// Checks the cancellation flag between stream items. When cancelled,
/// returns the partial response accumulated so far.
///
/// **Important:** We break on `FinalResponse` rather than waiting for
/// the stream to yield `None`. Rig's `async_stream` generator may not
/// terminate promptly after yielding `FinalResponse` (tracing
/// instrumentation, async cleanup), causing an indefinite hang.
async fn consume_stream<R: Send + 'static>(
    mut stream: StreamingResult<R>,
    event_tx: &Sender<AgentEvent>,
    cancelled: &AtomicBool,
) -> Result<String> {
    let mut response_text = String::new();
    // Track whether we received streaming reasoning deltas.
    // If so, the final Reasoning block is redundant (it contains the same text).
    let mut had_reasoning_deltas = false;

    while let Some(item) = stream.next().await {
        if cancelled.load(Ordering::Relaxed) {
            log::info!("[agent] Stream cancelled by user");
            break;
        }
        match item {
            Ok(MultiTurnStreamItem::StreamAssistantItem(content)) => match content {
                StreamedAssistantContent::Text(text) => {
                    response_text.push_str(&text.text);
                }
                StreamedAssistantContent::ReasoningDelta { reasoning, .. } => {
                    had_reasoning_deltas = true;
                    let _ = event_tx.send(AgentEvent::ThinkingDelta(reasoning));
                }
                StreamedAssistantContent::Reasoning(reasoning) => {
                    // Only emit if we didn't get streaming deltas (avoids duplication).
                    // Some providers send only the full block without deltas.
                    if !had_reasoning_deltas {
                        for part in &reasoning.content {
                            if let rig::completion::message::ReasoningContent::Text {
                                text, ..
                            } = part
                            {
                                let _ =
                                    event_tx.send(AgentEvent::ThinkingDelta(text.clone()));
                            }
                        }
                    }
                }
                StreamedAssistantContent::ToolCall { tool_call, .. } => {
                    log::info!("[agent] Stream: tool_call: {}", tool_call.function.name);
                }
                _ => {}
            },
            Ok(MultiTurnStreamItem::StreamUserItem(user_item)) => {
                log::info!("[agent] Stream: tool result: {:?}", user_item);
            }
            Ok(MultiTurnStreamItem::FinalResponse(final_resp)) => {
                log::info!(
                    "[agent] Stream: final response ({} chars accumulated, {} chars in FinalResponse)",
                    response_text.len(),
                    final_resp.response().len(),
                );
                // Use FinalResponse text if we somehow missed streaming deltas.
                // FinalResponse contains the last turn's text; response_text has
                // all turns. Prefer response_text when available.
                if response_text.is_empty() && !final_resp.response().is_empty() {
                    response_text = final_resp.response().to_string();
                }
                // FinalResponse is the terminal item — do NOT wait for None.
                // Rig's async_stream generator may not yield None promptly after
                // FinalResponse due to tracing instrumentation and async cleanup,
                // causing the stream to hang indefinitely.
                break;
            }
            Ok(_) => {}
            Err(e) => {
                log::error!("[agent] Stream error: {e}");
                return Err(anyhow::anyhow!("Streaming error: {e}"));
            }
        }
    }
    log::info!(
        "[agent] Stream consumption complete: {} chars",
        response_text.len(),
    );
    Ok(response_text)
}
