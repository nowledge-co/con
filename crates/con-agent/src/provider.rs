use anyhow::Result;
use crossbeam_channel::Sender;
use rig::client::CompletionClient;
use rig::completion::Prompt;
use rig::providers::{anthropic, openai};
use serde::{Deserialize, Serialize};

use crate::context::TerminalContext;
use crate::conversation::{AgentStep, Conversation, Message};
use crate::hook::{ConHook, ToolApprovalDecision};
use crate::tools::{FileReadTool, FileWriteTool, SearchTool, ShellExecTool};

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
    ) -> Result<Message> {
        let _ = event_tx.send(AgentEvent::Thinking);

        let system_prompt = context.to_system_prompt();
        let mut chat_history = conversation.to_rig_history();
        let last_user_msg = conversation
            .last_user_message()
            .map(|m| m.content.clone())
            .unwrap_or_default();

        let _ = event_tx.send(AgentEvent::Step(AgentStep::Thinking(format!(
            "{}:{}",
            serde_json::to_string(&self.config.provider)
                .unwrap_or_default()
                .trim_matches('"'),
            self.config.effective_model(),
        ))));

        let hook = ConHook::new(
            event_tx.clone(),
            approval_rx,
            self.config.auto_approve_tools,
        );

        let response = match self.config.provider {
            ProviderKind::Anthropic | ProviderKind::OpenAICompatible => {
                self.send_anthropic(&system_prompt, &last_user_msg, &mut chat_history, hook)
                    .await?
            }
            ProviderKind::OpenAI => {
                self.send_openai(&system_prompt, &last_user_msg, &mut chat_history, hook)
                    .await?
            }
            _ => {
                self.send_openai_compat(&system_prompt, &last_user_msg, &mut chat_history, hook)
                    .await?
            }
        };

        let message = Message::assistant(&response);
        let _ = event_tx.send(AgentEvent::Done(message.clone()));

        Ok(message)
    }

    async fn send_anthropic(
        &self,
        system_prompt: &str,
        prompt: &str,
        history: &mut Vec<rig::message::Message>,
        hook: ConHook,
    ) -> Result<String> {
        let api_key = self.resolve_api_key()?;
        let mut builder = anthropic::Client::builder().api_key(&api_key);
        if let Some(ref base_url) = self.config.base_url {
            builder = builder.base_url(base_url);
        }
        let client = builder
            .build()
            .map_err(|e| anyhow::anyhow!("Anthropic client error: {e}"))?;

        let agent = client
            .agent(self.config.effective_model())
            .preamble(system_prompt)
            .tool(ShellExecTool)
            .tool(FileReadTool)
            .tool(FileWriteTool)
            .tool(SearchTool)
            .max_tokens(self.config.max_tokens)
            .default_max_turns(self.config.max_turns)
            .build();

        agent
            .prompt(prompt)
            .with_hook(hook)
            .with_history(history)
            .await
            .map_err(|e| anyhow::anyhow!("Agent error: {e}"))
    }

    async fn send_openai(
        &self,
        system_prompt: &str,
        prompt: &str,
        history: &mut Vec<rig::message::Message>,
        hook: ConHook,
    ) -> Result<String> {
        let api_key = self.resolve_api_key()?;
        let mut builder = openai::Client::builder().api_key(&api_key);
        if let Some(ref base_url) = self.config.base_url {
            builder = builder.base_url(base_url);
        }
        let client = builder
            .build()
            .map_err(|e| anyhow::anyhow!("OpenAI client error: {e}"))?;

        let agent = client
            .agent(self.config.effective_model())
            .preamble(system_prompt)
            .tool(ShellExecTool)
            .tool(FileReadTool)
            .tool(FileWriteTool)
            .tool(SearchTool)
            .max_tokens(self.config.max_tokens)
            .default_max_turns(self.config.max_turns)
            .build();

        agent
            .prompt(prompt)
            .with_hook(hook)
            .with_history(history)
            .await
            .map_err(|e| anyhow::anyhow!("Agent error: {e}"))
    }

    async fn send_openai_compat(
        &self,
        system_prompt: &str,
        prompt: &str,
        history: &mut Vec<rig::message::Message>,
        hook: ConHook,
    ) -> Result<String> {
        let api_key = self.resolve_api_key()?;

        let base_url = self
            .config
            .base_url
            .clone()
            .or_else(|| self.default_base_url())
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "No base_url configured for provider {:?}. Set [agent] base_url in config.toml.",
                    self.config.provider
                )
            })?;

        let client = openai::Client::builder()
            .api_key(&api_key)
            .base_url(&base_url)
            .build()
            .map_err(|e| anyhow::anyhow!("Client error: {e}"))?;

        let agent = client
            .agent(self.config.effective_model())
            .preamble(system_prompt)
            .tool(ShellExecTool)
            .tool(FileReadTool)
            .tool(FileWriteTool)
            .tool(SearchTool)
            .max_tokens(self.config.max_tokens)
            .default_max_turns(self.config.max_turns)
            .build();

        agent
            .prompt(prompt)
            .with_hook(hook)
            .with_history(history)
            .await
            .map_err(|e| anyhow::anyhow!("Agent error: {e}"))
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
