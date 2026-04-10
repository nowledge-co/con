use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use crossbeam_channel::Sender;
use rig::agent::{HookAction, PromptHook, ToolCallHookAction};
use rig::completion::CompletionModel;

use crate::provider::AgentEvent;

/// Tool approval decision sent from the UI back to the hook.
#[derive(Debug, Clone)]
pub struct ToolApprovalDecision {
    pub call_id: String,
    pub allowed: bool,
    pub reason: Option<String>,
}

/// Hook that bridges Rig's agent loop into con's event system.
///
/// Emits `AgentEvent`s for tool calls and tool results so the UI
/// can show what the agent is doing in real time.
///
/// For dangerous tools (terminal_exec, shell_exec, file_write, edit_file),
/// blocks on an approval channel waiting for the user's decision before proceeding.
///
/// ## Channel design
///
/// Each `ConHook` instance gets its own dedicated `approval_rx`.
/// The harness creates a fresh channel pair per `send_message()` call
/// and passes the sender to the UI via a `ToolApprovalNeeded` harness
/// event. This avoids race conditions: only one hook instance reads
/// from each channel, and tool calls within a single agent request
/// are sequential (Rig's default concurrency is 1).
///
/// ## Streaming
///
/// `on_text_delta` fires during streaming via `stream_prompt()`,
/// emitting `AgentEvent::Token` for each text chunk. The UI
/// receives these incrementally for real-time text rendering.
#[derive(Clone)]
pub struct ConHook {
    event_tx: Sender<AgentEvent>,
    approval_rx: crossbeam_channel::Receiver<ToolApprovalDecision>,
    auto_approve: bool,
    cancel_flag: Arc<AtomicBool>,
}

impl ConHook {
    pub fn new(
        event_tx: Sender<AgentEvent>,
        approval_rx: crossbeam_channel::Receiver<ToolApprovalDecision>,
        auto_approve: bool,
        cancel_flag: Arc<AtomicBool>,
    ) -> Self {
        Self {
            event_tx,
            approval_rx,
            auto_approve,
            cancel_flag,
        }
    }
}

pub fn is_dangerous(tool_name: &str) -> bool {
    matches!(
        tool_name,
        "shell_exec"
            | "terminal_exec"
            | "batch_exec"
            | "file_write"
            | "edit_file"
            | "send_keys"
            | "ensure_remote_shell_target"
            | "remote_exec"
            | "tmux_run_command"
            | "tmux_ensure_shell_target"
            | "tmux_ensure_agent_target"
    )
}

/// Poll interval when waiting for approval. Short enough to respond
/// quickly to cancellation, long enough to avoid busy-spinning.
const APPROVAL_POLL_INTERVAL: std::time::Duration = std::time::Duration::from_millis(200);

/// Total approval timeout: 5 minutes.
const APPROVAL_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(300);

impl<M: CompletionModel> PromptHook<M> for ConHook {
    fn on_tool_call(
        &self,
        tool_name: &str,
        _tool_call_id: Option<String>,
        internal_call_id: &str,
        args: &str,
    ) -> impl Future<Output = ToolCallHookAction> + Send {
        let call_id = internal_call_id.to_string();
        let tool_name = tool_name.to_string();
        let args = args.to_string();
        let event_tx = self.event_tx.clone();
        let approval_rx = self.approval_rx.clone();
        let auto_approve = self.auto_approve;
        let cancel_flag = self.cancel_flag.clone();

        async move {
            let _ = event_tx.send(AgentEvent::ToolCallStart {
                call_id: call_id.clone(),
                tool_name: tool_name.clone(),
                args: args.clone(),
            });

            if !is_dangerous(&tool_name) || auto_approve {
                return ToolCallHookAction::cont();
            }

            // Poll for approval with short intervals so we can respond
            // to cancellation (e.g. app quit) without blocking shutdown.
            let decision = tokio::task::block_in_place(|| {
                let deadline = std::time::Instant::now() + APPROVAL_TIMEOUT;
                loop {
                    // Check cancellation first — enables clean shutdown
                    if cancel_flag.load(Ordering::Relaxed) {
                        return None;
                    }

                    match approval_rx.recv_timeout(APPROVAL_POLL_INTERVAL) {
                        Ok(decision) => return Some(decision),
                        Err(crossbeam_channel::RecvTimeoutError::Timeout) => {
                            if std::time::Instant::now() >= deadline {
                                return None;
                            }
                        }
                        Err(crossbeam_channel::RecvTimeoutError::Disconnected) => {
                            return None;
                        }
                    }
                }
            });

            match decision {
                Some(d) if d.allowed => ToolCallHookAction::cont(),
                Some(d) => ToolCallHookAction::skip(
                    d.reason
                        .unwrap_or_else(|| "User denied tool execution".to_string()),
                ),
                None => ToolCallHookAction::skip("Tool approval timed out or cancelled"),
            }
        }
    }

    fn on_tool_result(
        &self,
        tool_name: &str,
        _tool_call_id: Option<String>,
        internal_call_id: &str,
        _args: &str,
        result: &str,
    ) -> impl Future<Output = HookAction> + Send {
        let _ = self.event_tx.send(AgentEvent::ToolCallComplete {
            call_id: internal_call_id.to_string(),
            tool_name: tool_name.to_string(),
            result: result.to_string(),
        });
        async { HookAction::cont() }
    }

    fn on_text_delta(
        &self,
        text_delta: &str,
        _aggregated_text: &str,
    ) -> impl Future<Output = HookAction> + Send {
        let _ = self
            .event_tx
            .send(AgentEvent::Token(text_delta.to_string()));
        async { HookAction::cont() }
    }
}
