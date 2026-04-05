use chrono::{DateTime, Utc};
use rig::OneOrMany;
use rig::message::{AssistantContent, Message as RigMessage, Text, UserContent};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
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
            let start = if self
                .messages
                .first()
                .is_some_and(|m| m.role == MessageRole::System)
            {
                1 // preserve system message at index 0
            } else {
                0
            };
            self.messages.drain(start..start + drain_count);
        }
    }

    /// Truncate conversation to keep only the first `n` messages.
    /// Preserves the system message at index 0 if present.
    pub fn truncate_to(&mut self, n: usize) {
        if n < self.messages.len() {
            self.messages.truncate(n);
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
    ///
    /// Limits history to the most recent messages to avoid context pollution —
    /// stale assistant responses can override tool definitions and system prompts.
    const MAX_HISTORY_MESSAGES: usize = 20;

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

        // Take only the most recent messages to keep history manageable
        let start = msgs.len().saturating_sub(Self::MAX_HISTORY_MESSAGES);
        let msgs = &msgs[start..];

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

/// Summary of a saved conversation for listing in the UI
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConversationSummary {
    pub id: String,
    pub title: String,
    pub created_at: DateTime<Utc>,
    pub message_count: usize,
}

impl Conversation {
    /// Save this conversation to disk
    pub fn save(&self) -> anyhow::Result<()> {
        if self.messages.is_empty() {
            return Ok(());
        }
        let path = Self::conversation_path(&self.id);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let content = serde_json::to_string(self)?;
        std::fs::write(&path, content)?;
        Ok(())
    }

    /// Load a conversation by ID
    pub fn load(id: &str) -> anyhow::Result<Self> {
        let path = Self::conversation_path(id);
        let content = std::fs::read_to_string(&path)?;
        let conv: Conversation = serde_json::from_str(&content)?;
        Ok(conv)
    }

    /// List all saved conversations as summaries, newest first
    pub fn list_all() -> Vec<ConversationSummary> {
        let dir = Self::conversations_dir();
        let Ok(entries) = std::fs::read_dir(&dir) else {
            return Vec::new();
        };

        let mut summaries: Vec<ConversationSummary> = entries
            .filter_map(|e| e.ok())
            .filter(|e| e.path().extension().is_some_and(|ext| ext == "json"))
            .filter_map(|e| {
                let content = std::fs::read_to_string(e.path()).ok()?;
                let conv: Conversation = serde_json::from_str(&content).ok()?;
                let title = conv
                    .messages
                    .iter()
                    .find(|m| m.role == MessageRole::User)
                    .map(|m| {
                        if m.content.len() > 60 {
                            format!("{}...", &m.content[..57])
                        } else {
                            m.content.clone()
                        }
                    })
                    .unwrap_or_else(|| "Empty conversation".to_string());
                Some(ConversationSummary {
                    id: conv.id,
                    title,
                    created_at: conv.created_at,
                    message_count: conv.messages.len(),
                })
            })
            .collect();

        summaries.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        summaries
    }

    fn conversations_dir() -> PathBuf {
        dirs::data_dir()
            .or_else(|| dirs::home_dir().map(|h| h.join(".local/share")))
            .unwrap_or_else(|| PathBuf::from("/tmp"))
            .join("con")
            .join("conversations")
    }

    fn conversation_path(id: &str) -> PathBuf {
        Self::conversations_dir().join(format!("{}.json", id))
    }
}

impl Default for Conversation {
    fn default() -> Self {
        Self::new()
    }
}
