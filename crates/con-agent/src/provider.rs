use anyhow::Result;
use crossbeam_channel::Sender;
use futures::StreamExt;
use rig::agent::{MultiTurnStreamItem, StreamingResult};
use rig::client::CompletionClient;
use rig::completion::CompletionModel as _;
use rig::client::Nothing;
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
    EditFileTool, FileReadTool, FileWriteTool, ListFilesTool, SearchTool, ShellExecTool,
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
    fn default_api_key_env(&self) -> &str {
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

    fn default_model(&self) -> &str {
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

// ── Config ──────────────────────────────────────────────────────────

/// Optional overrides for the inline suggestion model.
/// Any field left `None` inherits from the parent `AgentConfig`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct SuggestionModelConfig {
    pub provider: Option<ProviderKind>,
    pub model: Option<String>,
    pub api_key_env: Option<String>,
    pub base_url: Option<String>,
}

/// Agent configuration from config.toml
///
/// ```toml
/// [agent]
/// provider = "anthropic"
/// model = "claude-sonnet-4-0"
/// api_key_env = "ANTHROPIC_API_KEY"
/// base_url = "https://my-proxy.example.com/v1"
/// max_tokens = 4096
/// max_turns = 10
/// temperature = 0.7
/// auto_approve_tools = false
///
/// [agent.suggestion_model]
/// model = "claude-3-5-haiku-20241022"
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AgentConfig {
    pub provider: ProviderKind,
    pub model: Option<String>,
    pub api_key_env: Option<String>,
    pub base_url: Option<String>,
    pub max_tokens: u64,
    pub max_turns: usize,
    pub temperature: Option<f64>,
    pub auto_context: bool,
    pub auto_approve_tools: bool,
    pub suggestion_model: SuggestionModelConfig,
}

impl Default for AgentConfig {
    fn default() -> Self {
        Self {
            provider: ProviderKind::default(),
            model: None,
            api_key_env: None,
            base_url: None,
            max_tokens: 4096,
            max_turns: 10,
            temperature: None,
            auto_context: true,
            auto_approve_tools: false,
            suggestion_model: SuggestionModelConfig::default(),
        }
    }
}

impl AgentConfig {
    fn effective_model(&self) -> &str {
        self.model
            .as_deref()
            .unwrap_or_else(|| self.provider.default_model())
    }

    fn effective_api_key_env(&self) -> &str {
        self.api_key_env
            .as_deref()
            .unwrap_or_else(|| self.provider.default_api_key_env())
    }

    /// Build a lightweight config for inline suggestions.
    /// Falls back to the main config for any field not overridden in `suggestion_model`.
    pub fn suggestion_agent_config(&self) -> AgentConfig {
        AgentConfig {
            provider: self
                .suggestion_model
                .provider
                .clone()
                .unwrap_or_else(|| self.provider.clone()),
            model: self
                .suggestion_model
                .model
                .clone()
                .or_else(|| self.model.clone()),
            api_key_env: self
                .suggestion_model
                .api_key_env
                .clone()
                .or_else(|| self.api_key_env.clone()),
            base_url: self
                .suggestion_model
                .base_url
                .clone()
                .or_else(|| self.base_url.clone()),
            max_tokens: 100,
            max_turns: 1,
            temperature: Some(0.0),
            auto_context: false,
            auto_approve_tools: false,
            suggestion_model: SuggestionModelConfig::default(),
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
    ($client:expr, $cfg:expr, $system_prompt:expr, $prompt:expr,
     $history:expr, $hook:expr, $terminal_exec_tx:expr,
     $event_tx:expr, $cancelled:expr) => {{
        let mut builder = $client
            .agent($cfg.effective_model())
            .preamble($system_prompt)
            .tool(TerminalExecTool::new($terminal_exec_tx))
            .tool(ShellExecTool)
            .tool(FileReadTool)
            .tool(FileWriteTool)
            .tool(EditFileTool)
            .tool(ListFilesTool)
            .tool(SearchTool)
            .max_tokens($cfg.max_tokens)
            .default_max_turns($cfg.max_turns);

        if let Some(temp) = $cfg.temperature {
            builder = builder.temperature(temp);
        }

        let agent = builder.build();

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
        cancelled: Arc<AtomicBool>,
    ) -> Result<Message> {
        let _ = event_tx.send(AgentEvent::Thinking);

        let system_prompt = context.to_system_prompt();
        let chat_history = conversation.to_rig_history();
        let last_user_msg = conversation
            .last_user_message()
            .map(|m| m.content.clone())
            .unwrap_or_default();

        let _ = event_tx.send(AgentEvent::Step(AgentStep::Thinking(format!(
            "{}:{}",
            self.config.provider,
            self.config.effective_model(),
        ))));

        let hook = ConHook::new(
            event_tx.clone(),
            approval_rx,
            self.config.auto_approve_tools,
        );

        // Each provider dispatches to its native Rig client — exhaustive match
        // prevents silent misrouting.
        macro_rules! stream_with {
            ($client:expr) => {
                build_and_stream!(
                    $client, self.config, &system_prompt, &last_user_msg,
                    chat_history, hook, terminal_exec_tx, &event_tx, &cancelled
                )?
            };
        }

        let response = match self.config.provider {
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
    // Each provider uses its native Rig client. Tool registration is
    // centralized via the `build_and_stream!` macro above. A generic
    // `stream_agent(impl CompletionClient, ...)` approach doesn't work
    // because Rig's CompletionClient trait has associated types with
    // complex lifetime bounds. The macro achieves the same deduplication
    // without fighting the type system.

    fn build_anthropic_client(&self) -> Result<anthropic::Client> {
        let api_key = self.resolve_api_key()?;
        let mut builder = anthropic::Client::builder().api_key(&api_key);
        if let Some(ref url) = self.config.base_url {
            builder = builder.base_url(url);
        }
        builder
            .build()
            .map_err(|e| anyhow::anyhow!("Anthropic client error: {e}"))
    }

    fn build_openai_client(&self) -> Result<openai::Client> {
        let api_key = self.resolve_api_key()?;
        let mut builder = openai::Client::builder().api_key(&api_key);
        if let Some(ref url) = self.config.base_url {
            builder = builder.base_url(url);
        }
        builder
            .build()
            .map_err(|e| anyhow::anyhow!("OpenAI client error: {e}"))
    }

    fn build_deepseek_client(&self) -> Result<deepseek::Client> {
        let api_key = self.resolve_api_key()?;
        let mut builder = deepseek::Client::builder().api_key(&api_key);
        if let Some(ref url) = self.config.base_url {
            builder = builder.base_url(url);
        }
        builder
            .build()
            .map_err(|e| anyhow::anyhow!("DeepSeek client error: {e}"))
    }

    fn build_groq_client(&self) -> Result<groq::Client> {
        let api_key = self.resolve_api_key()?;
        let mut builder = groq::Client::builder().api_key(&api_key);
        if let Some(ref url) = self.config.base_url {
            builder = builder.base_url(url);
        }
        builder
            .build()
            .map_err(|e| anyhow::anyhow!("Groq client error: {e}"))
    }

    fn build_cohere_client(&self) -> Result<cohere::Client> {
        let api_key = self.resolve_api_key()?;
        let mut builder = cohere::Client::builder().api_key(&api_key);
        if let Some(ref url) = self.config.base_url {
            builder = builder.base_url(url);
        }
        builder
            .build()
            .map_err(|e| anyhow::anyhow!("Cohere client error: {e}"))
    }

    fn build_gemini_client(&self) -> Result<gemini::Client> {
        let api_key = self.resolve_api_key()?;
        let mut builder = gemini::Client::builder().api_key(&api_key);
        if let Some(ref url) = self.config.base_url {
            builder = builder.base_url(url);
        }
        builder
            .build()
            .map_err(|e| anyhow::anyhow!("Gemini client error: {e}"))
    }

    fn build_ollama_client(&self) -> Result<ollama::Client> {
        let mut builder = ollama::Client::builder().api_key(Nothing);
        if let Some(ref url) = self.config.base_url {
            builder = builder.base_url(url);
        }
        builder
            .build()
            .map_err(|e| anyhow::anyhow!("Ollama client error: {e}"))
    }

    fn build_openrouter_client(&self) -> Result<openrouter::Client> {
        let api_key = self.resolve_api_key()?;
        let mut builder = openrouter::Client::builder().api_key(&api_key);
        if let Some(ref url) = self.config.base_url {
            builder = builder.base_url(url);
        }
        builder
            .build()
            .map_err(|e| anyhow::anyhow!("OpenRouter client error: {e}"))
    }

    fn build_perplexity_client(&self) -> Result<perplexity::Client> {
        let api_key = self.resolve_api_key()?;
        let mut builder = perplexity::Client::builder().api_key(&api_key);
        if let Some(ref url) = self.config.base_url {
            builder = builder.base_url(url);
        }
        builder
            .build()
            .map_err(|e| anyhow::anyhow!("Perplexity client error: {e}"))
    }

    fn build_mistral_client(&self) -> Result<mistral::Client> {
        let api_key = self.resolve_api_key()?;
        let mut builder = mistral::Client::builder().api_key(&api_key);
        if let Some(ref url) = self.config.base_url {
            builder = builder.base_url(url);
        }
        builder
            .build()
            .map_err(|e| anyhow::anyhow!("Mistral client error: {e}"))
    }

    fn build_together_client(&self) -> Result<together::Client> {
        let api_key = self.resolve_api_key()?;
        let mut builder = together::Client::builder().api_key(&api_key);
        if let Some(ref url) = self.config.base_url {
            builder = builder.base_url(url);
        }
        builder
            .build()
            .map_err(|e| anyhow::anyhow!("Together client error: {e}"))
    }

    fn build_xai_client(&self) -> Result<xai::Client> {
        let api_key = self.resolve_api_key()?;
        let mut builder = xai::Client::builder().api_key(&api_key);
        if let Some(ref url) = self.config.base_url {
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

        let preamble = "You are a shell command completion assistant. Be extremely concise.";

        macro_rules! do_complete {
            ($client:expr) => {{
                let model = $client.completion_model(self.config.effective_model());
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

        match self.config.provider {
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

    fn resolve_api_key(&self) -> Result<String> {
        let env_var = self.config.effective_api_key_env();
        if self.config.provider == ProviderKind::Ollama {
            return Ok(std::env::var(env_var).unwrap_or_else(|_| "ollama".into()));
        }
        std::env::var(env_var).map_err(|_| {
            anyhow::anyhow!(
                "No API key found. Set the {} environment variable.",
                env_var
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
                _ => {}
            },
            Ok(_) => {}
            Err(e) => return Err(anyhow::anyhow!("Streaming error: {e}")),
        }
    }
    Ok(response_text)
}
