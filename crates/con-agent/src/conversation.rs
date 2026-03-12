use chrono::{DateTime, Utc};
use rig::message::{Message as RigMessage, UserContent, AssistantContent, Text};
use rig::OneOrMany;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Maximum number of messages to keep in a conversation before truncating
const MAX_HISTORY: usize = 100;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum MessageRole {
    User,
    Assistant,
    System,
    Tool,
}

/// A single step in the agent's reasoning/execution
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AgentStep {
    Thinking(String),
    ToolCall {
        tool: String,
        input: serde_json::Value,
    },
    ToolResult {
        tool: String,
        output: String,
        success: bool,
    },
    Text(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub id: String,
    pub role: MessageRole,
    pub content: String,
    pub steps: Vec<AgentStep>,
    pub timestamp: DateTime<Utc>,
}

impl Message {
    pub fn user(content: impl Into<String>) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            role: MessageRole::User,
            content: content.into(),
            steps: Vec::new(),
            timestamp: Utc::now(),
        }
    }

    pub fn assistant(content: impl Into<String>) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            role: MessageRole::Assistant,
            content: content.into(),
            steps: Vec::new(),
            timestamp: Utc::now(),
        }
    }

    pub fn system(content: impl Into<String>) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            role: MessageRole::System,
            content: content.into(),
            steps: Vec::new(),
            timestamp: Utc::now(),
        }
    }
}

/// A conversation with bounded history
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Conversation {
    pub id: String,
    pub messages: Vec<Message>,
    pub created_at: DateTime<Utc>,
}

impl Conversation {
    pub fn new() -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            messages: Vec::new(),
            created_at: Utc::now(),
        }
    }

    pub fn add_message(&mut self, message: Message) {
        self.messages.push(message);
        // Keep history bounded — drop oldest messages beyond the limit,
        // but always preserve the first system message if present
        if self.messages.len() > MAX_HISTORY {
            let drain_count = self.messages.len() - MAX_HISTORY;
            let start = if self.messages.first().is_some_and(|m| m.role == MessageRole::System) {
                1 // preserve system message at index 0
            } else {
                0
            };
            self.messages.drain(start..start + drain_count);
        }
    }

    pub fn last_user_message(&self) -> Option<&Message> {
        self.messages
            .iter()
            .rev()
            .find(|m| m.role == MessageRole::User)
    }

    /// Convert our conversation history to Rig's Vec<Message> for the Chat trait.
    /// Excludes the last user message (which becomes the prompt) and system messages
    /// (which go into the preamble).
    pub fn to_rig_history(&self) -> Vec<RigMessage> {
        let mut history = Vec::new();
        // Skip the last user message — it will be sent as the prompt
        let msgs = if self
            .messages
            .last()
            .is_some_and(|m| m.role == MessageRole::User)
        {
            &self.messages[..self.messages.len() - 1]
        } else {
            &self.messages
        };

        for msg in msgs {
            match msg.role {
                MessageRole::User => {
                    history.push(RigMessage::User {
                        content: OneOrMany::one(UserContent::Text(Text {
                            text: msg.content.clone(),
                        })),
                    });
                }
                MessageRole::Assistant => {
                    history.push(RigMessage::Assistant {
                        id: None,
                        content: OneOrMany::one(AssistantContent::Text(Text {
                            text: msg.content.clone(),
                        })),
                    });
                }
                // System and Tool messages are handled separately
                MessageRole::System | MessageRole::Tool => {}
            }
        }
        history
    }
}

impl Default for Conversation {
    fn default() -> Self {
        Self::new()
    }
}
