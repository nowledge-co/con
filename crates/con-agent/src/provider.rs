use anyhow::Result;
use crossbeam_channel::Sender;
use serde::{Deserialize, Serialize};

use crate::context::TerminalContext;
use crate::conversation::{AgentStep, Conversation, Message};

/// Agent configuration from config.toml
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentConfig {
    pub provider: String,
    pub model: String,
    pub api_key_env: Option<String>,
    pub base_url: Option<String>,
    #[serde(default = "default_true")]
    pub auto_context: bool,
    #[serde(default = "default_true")]
    pub notification: bool,
}

fn default_true() -> bool {
    true
}

impl Default for AgentConfig {
    fn default() -> Self {
        Self {
            provider: "anthropic".to_string(),
            model: "claude-sonnet-4-20250514".to_string(),
            api_key_env: Some("ANTHROPIC_API_KEY".to_string()),
            base_url: None,
            auto_context: true,
            notification: true,
        }
    }
}

/// Events streamed from the agent during execution
#[derive(Debug, Clone)]
pub enum AgentEvent {
    /// Agent is processing
    Thinking,
    /// Streaming text token
    Token(String),
    /// Agent step (tool call, reasoning, etc.)
    Step(AgentStep),
    /// Agent finished
    Done(Message),
    /// Agent encountered an error
    Error(String),
}

/// The agent provider — wraps LLM interaction
pub struct AgentProvider {
    config: AgentConfig,
}

impl AgentProvider {
    pub fn new(config: AgentConfig) -> Self {
        Self { config }
    }

    /// Send a message and stream the response.
    /// This runs the full agent loop: prompt → (tool calls → execution)* → final response.
    pub async fn send(
        &self,
        conversation: &Conversation,
        context: &TerminalContext,
        event_tx: Sender<AgentEvent>,
    ) -> Result<Message> {
        let _ = event_tx.send(AgentEvent::Thinking);

        // Build the prompt with context
        let system_prompt = context.to_system_prompt();

        // Get API key
        let api_key = if let Some(env_var) = &self.config.api_key_env {
            std::env::var(env_var).ok()
        } else {
            None
        };

        if api_key.is_none() {
            let err = format!(
                "No API key found. Set {} environment variable.",
                self.config
                    .api_key_env
                    .as_deref()
                    .unwrap_or("ANTHROPIC_API_KEY")
            );
            let _ = event_tx.send(AgentEvent::Error(err.clone()));
            return Err(anyhow::anyhow!(err));
        }

        // For now, use a simple HTTP-based approach.
        // TODO: Replace with rig-core when we pin the exact API.
        // This keeps us working while rig-core API stabilizes.
        let last_user_msg = conversation
            .last_user_message()
            .map(|m| m.content.clone())
            .unwrap_or_default();

        // Build messages for the API
        let mut api_messages = vec![
            serde_json::json!({
                "role": "system",
                "content": system_prompt,
            }),
        ];

        for msg in &conversation.messages {
            let role = match msg.role {
                crate::conversation::MessageRole::User => "user",
                crate::conversation::MessageRole::Assistant => "assistant",
                crate::conversation::MessageRole::System => continue,
                crate::conversation::MessageRole::Tool => continue,
            };
            api_messages.push(serde_json::json!({
                "role": role,
                "content": msg.content,
            }));
        }

        // TODO: Implement actual API streaming via rig-core.
        // For MVP, return a placeholder that shows the architecture works.
        let response_text = format!(
            "I can see you're in `{}`. I received your message: \"{}\"\n\n\
             [Agent harness connected — provider: {}, model: {}]\n\
             [Context: {} lines of terminal output captured]\n\n\
             Once rig-core streaming is wired up, I'll be able to:\n\
             - Execute shell commands in your terminal\n\
             - Read and write files\n\
             - Search your codebase\n\
             - Reason about errors and suggest fixes",
            context.cwd.as_deref().unwrap_or("unknown"),
            last_user_msg,
            self.config.provider,
            self.config.model,
            context.recent_output.len(),
        );

        let _ = event_tx.send(AgentEvent::Token(response_text.clone()));

        let message = Message::assistant(response_text);
        let _ = event_tx.send(AgentEvent::Done(message.clone()));

        Ok(message)
    }
}
