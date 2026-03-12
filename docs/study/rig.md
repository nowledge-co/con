# Study: Rig v0.32.0 — Rust AI Agent Framework

## Overview

[Rig](https://rig.rs/) is the most mature Rust-native AI agent framework. MIT licensed.
Vendored at `3pp/rig/rig/rig-core` (v0.32.0).

## Core Architecture

### Client + Provider

```rust
use rig::providers::anthropic;
use rig::client::CompletionClient;

// From API key
let client = anthropic::Client::new("sk-ant-...").unwrap();

// From environment variable
let client = anthropic::Client::from_env(); // reads ANTHROPIC_API_KEY

// Model constants
anthropic::completion::CLAUDE_4_SONNET   // "claude-sonnet-4-0"
anthropic::completion::CLAUDE_4_OPUS     // "claude-opus-4-0"
anthropic::completion::CLAUDE_3_5_SONNET // "claude-3-5-sonnet-latest"
```

### Agent Builder

```rust
let agent = client
    .agent(anthropic::completion::CLAUDE_4_SONNET)
    .preamble("You are a terminal assistant.")
    .tool(ShellExecTool)
    .tool(FileReadTool)
    .max_tokens(4096)
    .default_max_turns(10) // max tool call loops
    .build();
```

### Chat (multi-turn)

```rust
use rig::completion::Chat;

let response: String = agent
    .chat("explain this error", chat_history)
    .await?;
```

### Prompt (single turn)

```rust
use rig::completion::Prompt;

let response: String = agent
    .prompt("list files in current dir")
    .await?;
```

## Tool Definition (Rig 0.32 API)

```rust
use rig::tool::Tool;
use rig::completion::ToolDefinition;

pub struct ShellExecTool;

impl Tool for ShellExecTool {
    const NAME: &'static str = "shell_exec";
    type Error = ToolError;        // must impl std::error::Error + Send + Sync
    type Args = ShellExecArgs;     // must impl Deserialize
    type Output = ShellExecOutput; // must impl Serialize

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: Self::NAME.to_string(),
            description: "Execute a shell command".to_string(),
            parameters: serde_json::json!({ /* JSON schema */ }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        // Execute the tool
    }
}
```

## PromptHook Trait (lifecycle callbacks)

```rust
use rig::agent::PromptHook;

impl PromptHook<M> for MyHook {
    async fn on_text_delta(&self, delta: &str, aggregated: &str) -> HookAction {
        // Stream token to UI
        HookAction::cont()
    }

    async fn on_tool_call(&self, name: &str, ...) -> ToolCallHookAction {
        // Log or approve/deny tool calls
        ToolCallHookAction::cont()
    }

    async fn on_tool_result(&self, name: &str, ...) -> HookAction {
        // Observe tool results
        HookAction::cont()
    }
}
```

## Message Types

```rust
use rig::message::{Message, UserContent, AssistantContent, Text};
use rig::OneOrMany;

// User message
Message::User {
    content: OneOrMany::one(UserContent::Text(Text { text: "hello".into() }))
}

// Assistant message
Message::Assistant {
    id: None,
    content: OneOrMany::one(AssistantContent::Text(Text { text: "hi".into() }))
}
```

## Streaming (RawStreamingChoice)

```rust
use rig::streaming::{StreamingCompletionResponse, RawStreamingChoice};

// Streaming responses yield these variants:
RawStreamingChoice::Message(String)       // text chunk
RawStreamingChoice::ToolCall(...)         // complete tool call
RawStreamingChoice::ToolCallDelta { .. }  // partial tool call
RawStreamingChoice::Reasoning { .. }      // reasoning content
RawStreamingChoice::FinalResponse(R)      // provider response object
```

## Integration with con

1. `con-agent/provider.rs` creates `anthropic::Client` from config
2. Builds an `Agent` with our 4 tool implementations
3. Uses `Chat::chat()` for multi-turn conversation
4. `con-agent/conversation.rs` converts our Message types to Rig's `Vec<Message>`
5. `con-core/harness.rs` runs agent work on a shared tokio runtime

## Key Differences from Rig 0.10

- `Tool` trait now uses `const NAME`, associated types for Args/Output/Error
- `definition()` and `call()` are now async methods (not derive macros)
- Client uses generic `Client<Ext, H>` architecture with `Capabilities` trait
- `Chat::chat()` takes `impl Into<Message>` + `Vec<Message>` history
- `PromptHook` replaces ad-hoc streaming callbacks
- Agent builder has typestate for tool configuration (NoToolConfig → WithBuilderTools)

## Workspace Integration Notes

Rig-core uses `workspace = true` for its deps. Since it lives inside our repo (at `3pp/rig/`),
Cargo resolves those against OUR workspace root. We added rig-core's transitive workspace deps
(as-any, async-stream, bytes, etc.) to our workspace Cargo.toml to satisfy these references.
