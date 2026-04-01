use anyhow::Result;
use crossbeam_channel::Sender;
use futures::StreamExt;
use rig::agent::{MultiTurnStreamItem, StreamingResult};
use rig::client::CompletionClient;
use rig::completion::CompletionModel as _;
use rig::providers::{anthropic, openai};
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
            Self::Anthropic | Self::OpenAICompatible => "ANTHROPIC_API_KEY",
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
            Self::Anthropic | Self::OpenAICompatible => "claude-sonnet-4-0",
            Self::OpenAI => "gpt-4o",
            Self::DeepSeek => "deepseek-chat",
            Self::Groq => "llama-3.3-70b-versatile",
            Self::Cohere => "command-r-plus",
            Self::Gemini => "gemini-2.0-flash",
            Self::Ollama => "llama3.2",
            Self::OpenRouter => "anthropic/claude-sonnet-4",
            Self::Perplexity => "sonar-pro",
            Self::Mistral => "mistral-large-latest",
            Self::Together => "meta-llama/Llama-3-70b-chat-hf",
            Self::XAI => "grok-3",
        }
    }
}

// ── Config ──────────────────────────────────────────────────────────

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
/// auto_approve_tools = false
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
    pub auto_context: bool,
    pub auto_approve_tools: bool,
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
            auto_context: true,
            auto_approve_tools: false,
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
        let agent = $client
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
            .default_max_turns($cfg.max_turns)
            .build();

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

        let response = match self.config.provider {
            ProviderKind::Anthropic | ProviderKind::OpenAICompatible => {
                let client = self.build_anthropic_client()?;
                build_and_stream!(
                    client, self.config, &system_prompt, &last_user_msg,
                    chat_history, hook, terminal_exec_tx, &event_tx, &cancelled
                )?
            }
            ProviderKind::OpenAI => {
                let client = self.build_openai_client(None)?;
                build_and_stream!(
                    client, self.config, &system_prompt, &last_user_msg,
                    chat_history, hook, terminal_exec_tx, &event_tx, &cancelled
                )?
            }
            _ => {
                let base_url = self
                    .default_base_url()
                    .ok_or_else(|| {
                        anyhow::anyhow!(
                            "No base_url configured for provider {:?}. Set [agent] base_url in config.toml.",
                            self.config.provider
                        )
                    })?;
                let client = self.build_openai_client(Some(&base_url))?;
                build_and_stream!(
                    client, self.config, &system_prompt, &last_user_msg,
                    chat_history, hook, terminal_exec_tx, &event_tx, &cancelled
                )?
            }
        };

        let message = Message::assistant(&response);
        let _ = event_tx.send(AgentEvent::Done(message.clone()));

        Ok(message)
    }

    // Note: Tool registration is centralized via the `build_and_stream!` macro below.
    // A generic `stream_agent(client: impl CompletionClient, ...)` approach doesn't work
    // because Rig's CompletionClient has associated types with complex lifetime bounds
    // that can't be easily expressed in a generic context. The macro achieves the same
    // deduplication without fighting the type system.

    fn build_anthropic_client(&self) -> Result<anthropic::Client> {
        let api_key = self.resolve_api_key()?;
        let mut builder = anthropic::Client::builder().api_key(&api_key);
        if let Some(ref base_url) = self.config.base_url {
            builder = builder.base_url(base_url);
        }
        builder
            .build()
            .map_err(|e| anyhow::anyhow!("Anthropic client error: {e}"))
    }

    fn build_openai_client(&self, base_url: Option<&str>) -> Result<openai::Client> {
        let api_key = self.resolve_api_key()?;
        let mut builder = openai::Client::builder().api_key(&api_key);
        if let Some(url) = base_url.or(self.config.base_url.as_deref()) {
            builder = builder.base_url(url);
        }
        builder
            .build()
            .map_err(|e| anyhow::anyhow!("OpenAI client error: {e}"))
    }

    fn default_base_url(&self) -> Option<String> {
        match self.config.provider {
            ProviderKind::DeepSeek => Some("https://api.deepseek.com".into()),
            ProviderKind::Groq => Some("https://api.groq.com/openai".into()),
            ProviderKind::Together => Some("https://api.together.xyz".into()),
            ProviderKind::Perplexity => Some("https://api.perplexity.ai".into()),
            ProviderKind::Mistral => Some("https://api.mistral.ai".into()),
            ProviderKind::XAI => Some("https://api.x.ai".into()),
            ProviderKind::Ollama => Some("http://localhost:11434".into()),
            ProviderKind::OpenRouter => Some("https://openrouter.ai/api".into()),
            _ => None,
        }
    }

    /// Lightweight completion — no tools, no history, just a simple prompt→response.
    /// Used for suggestions and other quick completions.
    pub async fn complete(&self, prompt: &str) -> Result<String> {
        use rig::completion::AssistantContent;

        let api_key = self.resolve_api_key()?;
        let preamble = "You are a shell command completion assistant. Be extremely concise.";

        match self.config.provider {
            ProviderKind::Anthropic | ProviderKind::OpenAICompatible => {
                let mut builder = anthropic::Client::builder().api_key(&api_key);
                if let Some(ref base_url) = self.config.base_url {
                    builder = builder.base_url(base_url);
                }
                let client = builder
                    .build()
                    .map_err(|e| anyhow::anyhow!("Client error: {e}"))?;

                let model = client.completion_model(self.config.effective_model());
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
            }
            _ => {
                let base_url = self
                    .config
                    .base_url
                    .clone()
                    .or_else(|| self.default_base_url())
                    .unwrap_or_else(|| "https://api.openai.com/v1".into());

                let client = openai::Client::builder()
                    .api_key(&api_key)
                    .base_url(&base_url)
                    .build()
                    .map_err(|e| anyhow::anyhow!("Client error: {e}"))?;

                let model = client.completion_model(self.config.effective_model());
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
            }
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
