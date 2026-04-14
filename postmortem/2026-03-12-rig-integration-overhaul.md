# 2026-03-12: Rig 0.32 integration — from placeholder to real agent

## What happened

The initial con-agent crate used `rig-core = "0.10"` from crates.io but never actually
called any LLM API — `provider.rs` returned hardcoded placeholder text. The agent harness
spawned a new thread + tokio runtime per message and recreated the AgentConfig with defaults
each time, ignoring user configuration. Tools were just JSON schema structs, not Rig Tool
trait implementations.

## Root causes

1. **API mismatch**: Rig 0.10's API surface didn't match what we scaffolded against.
   We had the 0.32.0 source cloned at `3pp/rig/` but hadn't wired it up.

2. **Thread-per-message**: The harness used `std::thread::spawn` with a new `tokio::runtime::Runtime`
   per agent message — unbounded thread creation with no lifecycle management.

3. **Config recreation**: `AgentProvider::new(AgentConfig::default())` was called per message
   instead of using the config loaded at startup.

4. **Nested workspace resolution**: Cargo doesn't support nested workspaces. Since `3pp/rig/rig/rig-core`
   is physically inside our repo, its `workspace = true` dep references resolved against OUR workspace
   Cargo.toml, not rig's. Required adding rig-core's transitive deps to our workspace.

## Fixes applied

### 1. Real Rig 0.32 integration
- Updated workspace dep: `rig-core = { path = "3pp/rig/rig/rig-core" }`
- Added rig-core's workspace deps (as-any, async-stream, bytes, etc.) to our workspace
- Added `exclude = ["3pp"]` to workspace to prevent accidental member resolution

### 2. Proper Tool trait implementations
- Rewrote `tools.rs`: `ShellExecTool`, `FileReadTool`, `FileWriteTool`, `SearchTool`
  all implement `rig::tool::Tool` with proper `Args`/`Output`/`Error` associated types
- Error type uses `thiserror` instead of strings

### 3. Real Anthropic agent
- `provider.rs` now creates `anthropic::Client::new(api_key)`, builds a real `Agent`
  with preamble + 4 tools, and calls `Chat::chat()` for multi-turn conversation
- `conversation.rs` converts our Message types to `rig::message::Message` via `to_rig_history()`

### 4. Shared tokio runtime
- `harness.rs` creates one `Arc<Runtime>` at startup
- `send_message()` uses `runtime.spawn()` instead of `std::thread::spawn`
- Bridge thread replaced with `tokio::task::spawn_blocking`

### 5. Instance config
- Harness uses the `AgentProvider` initialized with user config
- Config cleaned up: `#[serde(default)]` on struct instead of per-field default functions

### 6. Bounded conversation history
- Added `MAX_HISTORY = 100` constant
- `add_message()` trims old messages when limit exceeded

## What we learned

- Always verify path deps resolve correctly when vendoring crates that use workspace inheritance
- Cargo's workspace resolution walks up directories — nested workspaces need explicit `exclude`
- The Rig 0.32 API is well-designed: `Tool` trait, `Agent` builder, `Chat` trait, `PromptHook`
  lifecycle callbacks provide clean integration points
- A shared tokio runtime is essential — thread-per-message was architecturally broken
