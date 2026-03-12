# Study: Rig — Rust AI Agent Framework

## Overview

[Rig](https://rig.rs/) is the most mature Rust-native AI agent framework. MIT licensed.

## Why Rig

- Multi-provider: OpenAI, Anthropic, Cohere, Google, xAI, Ollama, etc.
- Tool-use with type-safe Rust definitions
- Streaming completions
- Agent loop (plan → act → observe → repeat)
- RAG support (vector stores, embeddings)
- 24.3% CPU in benchmarks — most efficient of tested frameworks
- Active development, good community

## Core Concepts

```rust
// Provider client
let client = rig::providers::anthropic::Client::from_env();

// Model
let model = client.agent("claude-sonnet-4-20250514")
    .preamble("You are a terminal assistant.")
    .tool(ShellExecTool)
    .tool(FileReadTool)
    .build();

// Chat
let response = model.prompt("list files in current dir").await?;

// Streaming
let stream = model.stream_prompt("explain this error").await?;
```

## Tool Definition Pattern

```rust
#[derive(Debug, Serialize, Deserialize, rig::Tool)]
#[tool(description = "Execute a shell command in the terminal")]
struct ShellExecTool;

impl Tool for ShellExecTool {
    type Input = ShellExecInput;
    type Output = String;

    async fn call(&self, input: ShellExecInput) -> Result<String> {
        // Write command to PTY, capture output
    }
}
```

## Integration with con

1. `con-agent` crate wraps Rig
2. Tools bridge to con-core's terminal manager (via socket API or direct Rust calls)
3. Streaming tokens render in the agent panel
4. Provider/model configurable via `config.toml`

## Provider Switching

```toml
# config.toml
[agent]
provider = "anthropic"  # or "openai", "ollama"
model = "claude-sonnet-4-20250514"
```

Rig handles the provider differences. Our config just maps to Rig's client constructors.

## Limitations

- No built-in MCP (Model Context Protocol) support — we'd add this ourselves if needed
- No built-in conversation persistence — we implement this in con-agent
- Streaming API may evolve — pin to specific rig version
