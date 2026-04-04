use crossbeam_channel::Sender;
use gpui::*;
use gpui_component::input::{Input, InputEvent, InputState};
use gpui_component::scroll::ScrollableElement;
use gpui_component::text::{TextView, TextViewStyle};
use gpui_component::ActiveTheme;

/// Max lines to show for tool result previews in collapsed steps
const TOOL_RESULT_PREVIEW_LINES: usize = 6;
/// Max characters to show in expanded thinking section
const THINKING_DISPLAY_LEN: usize = 2000;

use con_agent::{ConversationSummary, ToolApprovalDecision};
use con_core::harness::HarnessEvent;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum AgentStatus {
    Idle,
    Thinking,
    Responding,
}

struct ToolCallEntry {
    call_id: String,
    tool_name: String,
    args: String,
    result: Option<String>,
    started_at: std::time::Instant,
    duration: Option<std::time::Duration>,
}

struct PendingApproval {
    call_id: String,
    tool_name: String,
    args: String,
    approval_tx: Sender<ToolApprovalDecision>,
}

pub struct NewConversation;
impl EventEmitter<NewConversation> for AgentPanel {}

pub struct LoadConversation {
    pub id: String,
}
impl EventEmitter<LoadConversation> for AgentPanel {}

pub struct CancelRequest;
impl EventEmitter<CancelRequest> for AgentPanel {}

/// Emitted when user clicks "Allow All" to enable auto-approve for the session.
pub struct EnableAutoApprove;
impl EventEmitter<EnableAutoApprove> for AgentPanel {}

/// Emitted when user edits and resubmits a previous message.
/// The workspace truncates the conversation and re-sends.
pub struct RerunFromMessage {
    pub content: String,
}
impl EventEmitter<RerunFromMessage> for AgentPanel {}


// ---------------------------------------------------------------------------
// PanelState — per-tab conversation UI state (no GPUI dependency)
// ---------------------------------------------------------------------------

const SYSTEM_GREETING: &str = "Ask anything about your terminal, code, or system. The agent can read files, run commands, and search your workspace.";

/// Per-tab panel state. Holds messages, tool calls, and approvals.
/// Methods on PanelState are pure data operations — no GPUI notifications.
/// AgentPanel delegates to PanelState and calls cx.notify() itself.
pub struct PanelState {
    messages: Vec<PanelMessage>,
    tool_calls: Vec<ToolCallEntry>,
    pending_approvals: Vec<PendingApproval>,
    pub(crate) streaming: bool,
    pub(crate) status: AgentStatus,
}

impl PanelState {
    pub fn new() -> Self {
        Self {
            messages: vec![PanelMessage::new("system", SYSTEM_GREETING)],
            tool_calls: Vec::new(),
            pending_approvals: Vec::new(),
            streaming: false,
            status: AgentStatus::Idle,
        }
    }

    pub fn message_count(&self) -> usize {
        self.messages.len()
    }

    /// Populate from a loaded conversation (replay messages without steps/tools).
    pub fn from_conversation(conv: &con_agent::Conversation) -> Self {
        let mut state = Self::new();
        for msg in &conv.messages {
            let role = match msg.role {
                con_agent::MessageRole::User => "user",
                con_agent::MessageRole::Assistant => "assistant",
                con_agent::MessageRole::System => "system",
                con_agent::MessageRole::Tool => "system",
            };
            state.add_message(role, &msg.content);
        }
        state
    }

    pub fn clear(&mut self) {
        self.messages = vec![PanelMessage::new("system", SYSTEM_GREETING)];
        self.tool_calls.clear();
        self.pending_approvals.clear();
        self.streaming = false;
        self.status = AgentStatus::Idle;
    }

    /// Truncate conversation back to (but not including) the message at `msg_idx`.
    /// Used for edit-and-rerun: removes the old user message and everything after it.
    pub fn truncate_to(&mut self, msg_idx: usize) {
        if msg_idx < self.messages.len() {
            self.messages.truncate(msg_idx);
        }
        self.tool_calls.clear();
        self.pending_approvals.clear();
        self.streaming = false;
        self.status = AgentStatus::Idle;
    }

    pub fn add_message(&mut self, role: &str, content: &str) {
        self.streaming = false;
        self.status = AgentStatus::Idle;
        self.messages.push(PanelMessage::new(role, content));
    }

    pub fn add_step(&mut self, step: &str) {
        self.status = AgentStatus::Thinking;
        if let Some(last) = self.messages.last_mut() {
            last.steps.push(StepEntry {
                icon: "phosphor/brain.svg",
                label: step.to_string(),
                detail: None,
                status: StepStatus::Complete,
                detail_collapsed: true,
                duration: None,
            });
        }
    }

    pub fn add_tool_call(&mut self, call_id: &str, tool_name: &str, args: &str) {
        self.status = AgentStatus::Thinking;
        self.tool_calls.push(ToolCallEntry {
            call_id: call_id.to_string(),
            tool_name: tool_name.to_string(),
            args: args.to_string(),
            result: None,
            started_at: std::time::Instant::now(),
            duration: None,
        });
    }

    pub fn complete_tool_call(&mut self, call_id: &str, result: &str) {
        if let Some(entry) = self.tool_calls.iter_mut().find(|e| e.call_id == call_id) {
            entry.duration = Some(entry.started_at.elapsed());
            entry.result = Some(result.to_string());
        }
    }

    pub fn add_pending_approval(
        &mut self,
        call_id: &str,
        tool_name: &str,
        args: &str,
        approval_tx: Sender<ToolApprovalDecision>,
    ) {
        self.pending_approvals.push(PendingApproval {
            call_id: call_id.to_string(),
            tool_name: tool_name.to_string(),
            args: args.to_string(),
            approval_tx,
        });
    }

    pub fn update_thinking(&mut self, text: &str) {
        self.status = AgentStatus::Thinking;
        if !self.streaming {
            let mut msg = PanelMessage::assistant();
            msg.thinking = Some(String::new());
            self.messages.push(msg);
            self.streaming = true;
        }
        if let Some(last) = self.messages.last_mut() {
            let thinking = last.thinking.get_or_insert_with(String::new);
            thinking.push_str(text);
        }
    }

    pub fn update_streaming(&mut self, token: &str) {
        self.status = AgentStatus::Responding;
        if !self.streaming {
            self.messages.push(PanelMessage::assistant());
            self.streaming = true;
        }
        if let Some(last) = self.messages.last_mut() {
            last.content.push_str(token);
        }
    }

    pub fn complete_response(&mut self, final_content: &str) {
        self.status = AgentStatus::Idle;
        if self.streaming {
            // Only overwrite streamed content if final_content is non-empty.
            // Some providers don't emit text items, leaving final_content empty —
            // in that case, keep whatever was accumulated during streaming.
            if !final_content.is_empty() {
                if let Some(last) = self.messages.last_mut() {
                    last.content = final_content.to_string();
                }
            }
            self.streaming = false;
        } else if !final_content.is_empty() {
            self.messages
                .push(PanelMessage::new("assistant", final_content));
        }
        // Ensure a message exists for tool call steps even when no text was produced.
        // This happens when the agent only used tools without generating text output.
        if !self.tool_calls.is_empty()
            && self
                .messages
                .last()
                .map_or(true, |m| m.role != "assistant")
        {
            self.messages.push(PanelMessage::new("assistant", ""));
        }
        // Move active tool calls into the message's step timeline
        for tc in self.tool_calls.drain(..) {
            let args_display = format_tool_args(&tc.tool_name, &tc.args);
            let human_name = humanize_tool_name(&tc.tool_name);
            let (status, detail) = if let Some(result) = &tc.result {
                let formatted = format_tool_result(&tc.tool_name, &result);
                (StepStatus::Complete, Some(formatted))
            } else {
                (StepStatus::Running, None)
            };
            if let Some(last) = self.messages.last_mut() {
                last.steps.push(StepEntry {
                    icon: tool_icon(&tc.tool_name),
                    label: format!("{}: {}", human_name, truncate_str(&args_display, 60)),
                    detail,
                    status,
                    detail_collapsed: true,
                    duration: tc.duration,
                });
            }
        }
    }

    /// Apply a harness event to this state (for background tabs, no GPUI).
    pub fn apply_event(&mut self, event: HarnessEvent) {
        match event {
            HarnessEvent::Thinking => {
                self.add_step("Thinking...");
            }
            HarnessEvent::ThinkingDelta(text) => {
                self.update_thinking(&text);
            }
            HarnessEvent::Step(step) => {
                self.add_step(&format!("{:?}", step));
            }
            HarnessEvent::Token(token) => {
                self.update_streaming(&token);
            }
            HarnessEvent::ToolCallStart { call_id, tool_name, args } => {
                self.add_tool_call(&call_id, &tool_name, &args);
            }
            HarnessEvent::ToolApprovalNeeded { call_id, tool_name, args, approval_tx } => {
                self.add_pending_approval(&call_id, &tool_name, &args, approval_tx);
            }
            HarnessEvent::ToolCallComplete { call_id, tool_name: _, result } => {
                self.complete_tool_call(&call_id, &result);
            }
            HarnessEvent::ResponseComplete(msg) => {
                self.complete_response(&msg.content);
            }
            HarnessEvent::Error(err) => {
                self.add_message("system", &format!("Error: {}", err));
            }
            HarnessEvent::SkillsUpdated(_) => {}
        }
    }
}

// ---------------------------------------------------------------------------
// AgentPanel — GPUI entity wrapping PanelState
// ---------------------------------------------------------------------------

pub struct AgentPanel {
    state: PanelState,
    scroll_handle: ScrollHandle,
    showing_history: bool,
    conversation_list: Vec<ConversationSummary>,
    auto_approve: bool,
    model_name: String,
    /// Index of user message currently being edited inline (None = not editing)
    editing_msg_idx: Option<usize>,
    /// Input state for the inline edit field
    edit_input_state: Option<Entity<InputState>>,
}

struct PanelMessage {
    role: String,
    content: String,
    /// Extended thinking/reasoning text from the model (collapsible)
    thinking: Option<String>,
    thinking_collapsed: bool,
    steps: Vec<StepEntry>,
    steps_collapsed: bool,
}

/// A structured step entry (replaces raw Debug strings).
struct StepEntry {
    icon: &'static str,
    label: String,
    /// Full formatted result (may be multi-line JSON, command output, etc.)
    detail: Option<String>,
    status: StepStatus,
    /// Whether the detail section is collapsed (default: true)
    detail_collapsed: bool,
    /// How long this step took to execute.
    duration: Option<std::time::Duration>,
}

#[derive(Clone, Copy, PartialEq)]
enum StepStatus {
    Running,
    Complete,
    Denied,
}

impl PanelMessage {
    fn new(role: &str, content: &str) -> Self {
        Self {
            role: role.to_string(),
            content: content.to_string(),
            thinking: None,
            thinking_collapsed: true,
            steps: Vec::new(),
            steps_collapsed: false,
        }
    }

    fn assistant() -> Self {
        Self::new("assistant", "")
    }
}

impl AgentPanel {
    #[allow(dead_code)]
    pub fn new(_cx: &mut Context<Self>) -> Self {
        Self {
            state: PanelState::new(),
            scroll_handle: ScrollHandle::new(),
            showing_history: false,
            conversation_list: Vec::new(),
            auto_approve: false,
            model_name: String::new(),
            editing_msg_idx: None,
            edit_input_state: None,
        }
    }

    pub fn state(&self) -> &PanelState {
        &self.state
    }

    pub fn set_model_name(&mut self, name: String) {
        self.model_name = name;
    }

    /// Create with a pre-populated panel state (e.g. restored from session).
    pub fn with_state(state: PanelState, _cx: &mut Context<Self>) -> Self {
        Self {
            state,
            scroll_handle: ScrollHandle::new(),
            showing_history: false,
            conversation_list: Vec::new(),
            auto_approve: false,
            model_name: String::new(),
            editing_msg_idx: None,
            edit_input_state: None,
        }
    }

    /// Swap the displayed panel state. Returns the old state (to stash back in the tab).
    pub fn swap_state(&mut self, new_state: PanelState, cx: &mut Context<Self>) -> PanelState {
        let old = std::mem::replace(&mut self.state, new_state);
        self.scroll_handle = ScrollHandle::new();
        self.showing_history = false;
        cx.notify();
        old
    }

    pub fn clear_messages(&mut self, cx: &mut Context<Self>) {
        self.state.clear();
        self.showing_history = false;
        cx.notify();
    }

    /// Edit-and-rerun: truncate conversation to before `msg_idx`, add the
    /// edited content as a new user message, and emit RerunFromMessage so
    /// the workspace re-sends to the agent.
    pub fn rerun_from(&mut self, msg_idx: usize, new_content: String, cx: &mut Context<Self>) {
        self.state.truncate_to(msg_idx);
        self.state.add_message("user", &new_content);
        cx.emit(RerunFromMessage { content: new_content });
        cx.notify();
    }

    /// Start inline editing of a user message.
    fn start_editing(&mut self, msg_idx: usize, content: &str, window: &mut Window, cx: &mut Context<Self>) {
        let content = content.to_string();
        let input_state = cx.new(|cx| {
            let mut s = InputState::new(window, cx);
            s.set_value(&content, window, cx);
            s
        });
        // Subscribe to Enter key to submit the edit
        cx.subscribe_in(&input_state, window, {
            move |this, _, ev: &InputEvent, _window, cx| {
                if let InputEvent::PressEnter { secondary: false } = ev {
                    this.submit_edit(cx);
                }
            }
        }).detach();
        self.editing_msg_idx = Some(msg_idx);
        self.edit_input_state = Some(input_state);
        cx.notify();
    }

    /// Submit the inline edit: truncate and rerun.
    fn submit_edit(&mut self, cx: &mut Context<Self>) {
        if let (Some(msg_idx), Some(input_state)) = (self.editing_msg_idx, self.edit_input_state.as_ref()) {
            let new_content = input_state.read(cx).value().to_string();
            self.editing_msg_idx = None;
            self.edit_input_state = None;
            if !new_content.trim().is_empty() {
                self.rerun_from(msg_idx, new_content, cx);
            } else {
                cx.notify();
            }
        }
    }

    /// Cancel inline editing.
    fn cancel_edit(&mut self, cx: &mut Context<Self>) {
        self.editing_msg_idx = None;
        self.edit_input_state = None;
        cx.notify();
    }

    pub fn toggle_history(&mut self, cx: &mut Context<Self>) {
        self.showing_history = !self.showing_history;
        if self.showing_history {
            self.conversation_list = con_agent::Conversation::list_all();
        }
        cx.notify();
    }

    fn scroll_to_bottom(&self) {
        self.scroll_handle.scroll_to_bottom();
    }

    pub fn add_message(&mut self, role: &str, content: &str, cx: &mut Context<Self>) {
        self.state.add_message(role, content);
        self.scroll_to_bottom();
        cx.notify();
    }

    pub fn add_step(&mut self, step: &str, cx: &mut Context<Self>) {
        self.state.add_step(step);
        cx.notify();
    }

    pub fn add_tool_call(
        &mut self,
        call_id: &str,
        tool_name: &str,
        args: &str,
        cx: &mut Context<Self>,
    ) {
        self.state.add_tool_call(call_id, tool_name, args);
        self.scroll_to_bottom();
        cx.notify();
    }

    pub fn complete_tool_call(
        &mut self,
        call_id: &str,
        _tool_name: &str,
        result: &str,
        cx: &mut Context<Self>,
    ) {
        self.state.complete_tool_call(call_id, result);
        self.scroll_to_bottom();
        cx.notify();
    }

    pub fn add_pending_approval(
        &mut self,
        call_id: &str,
        tool_name: &str,
        args: &str,
        approval_tx: Sender<ToolApprovalDecision>,
        cx: &mut Context<Self>,
    ) {
        self.state.add_pending_approval(call_id, tool_name, args, approval_tx);
        self.scroll_to_bottom();
        cx.notify();
    }

    fn resolve_approval(&mut self, index: usize, allowed: bool, cx: &mut Context<Self>) {
        if index >= self.state.pending_approvals.len() {
            return;
        }
        let approval = self.state.pending_approvals.remove(index);
        let _ = approval.approval_tx.send(ToolApprovalDecision {
            call_id: approval.call_id,
            allowed,
            reason: if allowed {
                None
            } else {
                Some("User denied tool execution".to_string())
            },
        });
        if let Some(last) = self.state.messages.last_mut() {
            let (status, label) = if allowed {
                (StepStatus::Complete, format!("Allowed: {}", approval.tool_name))
            } else {
                (StepStatus::Denied, format!("Denied: {}", approval.tool_name))
            };
            last.steps.push(StepEntry {
                icon: if allowed { "phosphor/check.svg" } else { "phosphor/x.svg" },
                label,
                detail: None,
                status,
                detail_collapsed: true,
                duration: None,
            });
        }
        cx.notify();
    }

    fn resolve_all_approvals(&mut self, cx: &mut Context<Self>) {
        while !self.state.pending_approvals.is_empty() {
            self.resolve_approval(0, true, cx);
        }
    }

    pub fn update_thinking(&mut self, text: &str, cx: &mut Context<Self>) {
        self.state.update_thinking(text);
        self.scroll_to_bottom();
        cx.notify();
    }

    pub fn update_streaming(&mut self, token: &str, cx: &mut Context<Self>) {
        self.state.update_streaming(token);
        self.scroll_to_bottom();
        cx.notify();
    }

    pub fn complete_response(&mut self, final_content: &str, cx: &mut Context<Self>) {
        self.state.complete_response(final_content);
        self.scroll_to_bottom();
        cx.notify();
    }

    /// Derive a human-readable status line from current panel state.
    fn status_text(&self) -> Option<(&'static str, &'static str)> {
        // Priority: approval > running tool > thinking > responding
        if !self.state.pending_approvals.is_empty() {
            return Some(("phosphor/warning.svg", "Awaiting approval…"));
        }
        if let Some(tc) = self.state.tool_calls.iter().find(|tc| tc.result.is_none()) {
            let label = match tc.tool_name.as_str() {
                "terminal_exec" | "batch_exec" => "Running command…",
                "shell_exec" => "Running in background…",
                "file_read" => "Reading file…",
                "file_write" | "edit_file" => "Writing file…",
                "search" | "search_panes" => "Searching…",
                "list_panes" | "read_pane" => "Reading pane…",
                "list_files" => "Listing files…",
                "send_keys" => "Sending keys…",
                _ => "Running tool…",
            };
            return Some((tool_icon(&tc.tool_name), label));
        }
        match self.state.status {
            AgentStatus::Thinking => Some(("phosphor/brain.svg", "Thinking…")),
            AgentStatus::Responding => Some(("phosphor/pencil-simple.svg", "Writing…")),
            AgentStatus::Idle => None,
        }
    }
}

/// Markdown style for chat messages — readable prose with breathing room.
fn chat_markdown_style() -> TextViewStyle {
    TextViewStyle::default()
        .paragraph_gap(rems(0.75))
        .heading_font_size(|level, _base| {
            match level {
                1 => px(17.0),
                2 => px(15.5),
                3 => px(14.5),
                _ => px(14.0),
            }
        })
}

fn tool_icon(tool_name: &str) -> &'static str {
    match tool_name {
        "terminal_exec" | "batch_exec" => "phosphor/play.svg",
        "shell_exec" => "phosphor/terminal.svg",
        "file_write" => "phosphor/pencil-simple.svg",
        "file_read" => "phosphor/file-code.svg",
        "edit_file" => "phosphor/pencil-simple.svg",
        "list_files" => "phosphor/folder.svg",
        "search" | "search_panes" => "phosphor/magnifying-glass.svg",
        "list_panes" => "phosphor/columns.svg",
        "read_pane" => "phosphor/eye.svg",
        "send_keys" => "phosphor/keyboard.svg",
        _ => "phosphor/gear.svg",
    }
}

fn format_tool_args(tool_name: &str, args: &str) -> String {
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(args) {
        match tool_name {
            "terminal_exec" | "shell_exec" => {
                if let Some(cmd) = v.get("command").and_then(|c| c.as_str()) {
                    return cmd.to_string();
                }
            }
            "file_read" | "file_write" | "edit_file" => {
                if let Some(path) = v.get("path").and_then(|p| p.as_str()) {
                    return path.to_string();
                }
            }
            "list_files" => {
                return v
                    .get("path")
                    .and_then(|p| p.as_str())
                    .unwrap_or(".")
                    .to_string();
            }
            "search" => {
                if let Some(pattern) = v.get("pattern").and_then(|q| q.as_str()) {
                    return format!("\"{}\"", pattern);
                }
            }
            "search_panes" => {
                if let Some(pattern) = v.get("pattern").and_then(|q| q.as_str()) {
                    return format!("\"{}\"", pattern);
                }
            }
            "batch_exec" => {
                if let Some(cmds) = v.get("commands").and_then(|c| c.as_array()) {
                    return format!("{} commands", cmds.len());
                }
            }
            "send_keys" => {
                if let Some(keys) = v.get("keys").and_then(|k| k.as_str()) {
                    return keys.to_string();
                }
            }
            "read_pane" | "list_panes" => {
                if let Some(idx) = v.get("pane_index").and_then(|i| i.as_u64()) {
                    return format!("pane {}", idx);
                }
                return "all panes".to_string();
            }
            _ => {}
        }
    }
    truncate_str(args, 120)
}

fn truncate_str(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else {
        format!("{}…", &s[..s.floor_char_boundary(max_len)])
    }
}

/// "list_panes" → "List Panes", "terminal_exec" → "Terminal Exec"
fn humanize_tool_name(name: &str) -> String {
    name.split('_')
        .map(|word| {
            let mut chars = word.chars();
            match chars.next() {
                Some(c) => {
                    let mut s = c.to_uppercase().to_string();
                    s.extend(chars);
                    s
                }
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Unwrap Rig's double-encoding: Rig calls `serde_json::to_string()` on tool Output,
/// so `Output = String` gets wrapped in JSON string escaping. This unwraps it.
fn unwrap_tool_result(raw: &str) -> serde_json::Value {
    match serde_json::from_str::<serde_json::Value>(raw) {
        Ok(serde_json::Value::String(inner)) => {
            // Double-encoded: Rig wrapped our string in JSON quotes.
            // Try to parse the inner string as JSON (e.g., batch_exec's JSON array).
            match serde_json::from_str::<serde_json::Value>(&inner) {
                Ok(v) => v,
                Err(_) => serde_json::Value::String(inner),
            }
        }
        Ok(v) => v,
        Err(_) => serde_json::Value::String(raw.to_string()),
    }
}

/// Format a tool result for display. Content-aware: understands the shape of
/// each tool's output and renders it as human-readable text, not raw JSON.
fn format_tool_result(tool_name: &str, raw: &str) -> String {
    let value = unwrap_tool_result(raw);

    match tool_name {
        "batch_exec" => format_batch_result(&value),
        "terminal_exec" | "shell_exec" => format_exec_result(&value),
        "list_panes" => format_list_panes_result(&value),
        "search" | "search_panes" => format_search_result(&value),
        _ => format_generic_result(&value),
    }
}

/// batch_exec: show each pane's output as a labeled block.
fn format_batch_result(value: &serde_json::Value) -> String {
    let arr = match value.as_array() {
        Some(a) => a,
        None => return format_generic_result(value),
    };
    let mut out = String::new();
    for item in arr {
        let idx = item.get("pane_index").and_then(|v| v.as_u64()).unwrap_or(0);
        let output = item.get("output").and_then(|v| v.as_str()).unwrap_or("");
        let exit_code = item.get("exit_code").and_then(|v| v.as_i64());
        let error = item.get("error").and_then(|v| v.as_str()).filter(|s| !s.is_empty());

        if !out.is_empty() {
            out.push('\n');
        }
        out.push_str(&format!("── Pane {} ", idx));
        if let Some(code) = exit_code {
            out.push_str(&format!("(exit {}) ", code));
        }
        out.push_str("──\n");
        if let Some(err) = error {
            out.push_str(&format!("Error: {}\n", err));
        }
        let cleaned = output.trim();
        if !cleaned.is_empty() {
            out.push_str(cleaned);
            out.push('\n');
        }
    }
    out.trim_end().to_string()
}

/// terminal_exec / shell_exec: show stdout directly.
fn format_exec_result(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::Object(obj) => {
            let stdout = obj.get("stdout").and_then(|v| v.as_str()).unwrap_or("");
            let stderr = obj.get("stderr").and_then(|v| v.as_str()).unwrap_or("");
            let exit_code = obj.get("exit_code").and_then(|v| v.as_i64());

            let mut out = String::new();
            let trimmed = stdout.trim();
            if !trimmed.is_empty() {
                out.push_str(trimmed);
            }
            if !stderr.trim().is_empty() {
                if !out.is_empty() {
                    out.push('\n');
                }
                out.push_str(&format!("stderr: {}", stderr.trim()));
            }
            if let Some(code) = exit_code {
                if code != 0 {
                    if !out.is_empty() {
                        out.push('\n');
                    }
                    out.push_str(&format!("exit code: {}", code));
                }
            }
            if out.is_empty() {
                "(no output)".to_string()
            } else {
                out
            }
        }
        serde_json::Value::String(s) => s.clone(),
        _ => format_generic_result(value),
    }
}

/// list_panes: show a compact pane summary.
fn format_list_panes_result(value: &serde_json::Value) -> String {
    let arr = match value.as_array() {
        Some(a) => a,
        None => return format_generic_result(value),
    };
    let mut out = String::new();
    for item in arr {
        let idx = item.get("index").and_then(|v| v.as_u64()).unwrap_or(0);
        let title = item.get("title").and_then(|v| v.as_str()).unwrap_or("?");
        let focused = item.get("is_focused").and_then(|v| v.as_bool()).unwrap_or(false);
        let busy = item.get("is_busy").and_then(|v| v.as_bool()).unwrap_or(false);
        let alive = item.get("is_alive").and_then(|v| v.as_bool()).unwrap_or(true);
        let hostname = item.get("hostname").and_then(|v| v.as_str());
        let cwd = item.get("cwd").and_then(|v| v.as_str());

        let mut flags = Vec::new();
        if focused { flags.push("focused"); }
        if busy { flags.push("busy"); }
        if !alive { flags.push("dead"); }
        if let Some(h) = hostname { flags.push(h); }

        let location = cwd.unwrap_or("");
        let flag_str = if flags.is_empty() {
            String::new()
        } else {
            format!(" ({})", flags.join(", "))
        };
        out.push_str(&format!("{}. {}{} {}\n", idx, title, flag_str, location));
    }
    out.trim_end().to_string()
}

/// search / search_panes: show matching lines.
fn format_search_result(value: &serde_json::Value) -> String {
    // search_panes returns SearchResults: Vec<(pane_idx, line_num, text)>
    // search returns SearchOutput with matches
    match value {
        serde_json::Value::Array(arr) => {
            let mut out = String::new();
            for item in arr {
                if let Some(arr_item) = item.as_array() {
                    // Tuple format: [pane_idx, line_num, text]
                    let pane = arr_item.first().and_then(|v| v.as_u64()).unwrap_or(0);
                    let line = arr_item.get(1).and_then(|v| v.as_u64()).unwrap_or(0);
                    let text = arr_item.get(2).and_then(|v| v.as_str()).unwrap_or("");
                    out.push_str(&format!("pane {}:{}: {}\n", pane, line, text));
                }
            }
            if out.is_empty() { "(no matches)".to_string() } else { out.trim_end().to_string() }
        }
        _ => format_generic_result(value),
    }
}

/// Fallback: pretty-print JSON objects, or return strings directly.
fn format_generic_result(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::String(s) => s.clone(),
        _ => serde_json::to_string_pretty(value).unwrap_or_else(|_| format!("{:?}", value)),
    }
}

/// Create a preview of a tool result — first N lines + count of remaining.
fn result_preview(formatted: &str, max_lines: usize) -> String {
    let lines: Vec<&str> = formatted.lines().collect();
    if lines.len() <= max_lines {
        formatted.to_string()
    } else {
        let preview: String = lines[..max_lines].join("\n");
        format!("{}\n… {} more lines", preview, lines.len() - max_lines)
    }
}

impl Render for AgentPanel {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();

        // ── Messages ──────────────────────────────────────────────
        let mut messages_area = div()
            .id("agent-messages")
            .flex()
            .flex_col()
            .flex_1()
            .overflow_y_scroll()
            .track_scroll(&self.scroll_handle)
            .vertical_scrollbar(&self.scroll_handle)
            .px(px(16.0))
            .pt(px(16.0))
            .pb(px(24.0))
            .gap(px(16.0));

        for (msg_idx, msg) in self.state.messages.iter().enumerate() {
            let is_user = msg.role == "user";
            let is_system = msg.role == "system";

            let mut msg_el = div().flex().flex_col().gap(px(8.0));

            if is_system {
                // System greeting — quiet, centered feel
                msg_el = msg_el.child(
                    div()
                        .px(px(4.0))
                        .text_size(px(13.0))
                        .text_color(theme.muted_foreground.opacity(0.55))
                        .line_height(px(20.0))
                        .child(msg.content.clone()),
                );
            } else if is_user {
                let is_editing = self.editing_msg_idx == Some(msg_idx);

                if is_editing {
                    // ── Inline edit mode ──
                    if let Some(edit_input) = &self.edit_input_state {
                        msg_el = msg_el.child(
                            div()
                                .flex()
                                .flex_col()
                                .gap(px(6.0))
                                .items_end()
                                // Input field in a styled container
                                .child(
                                    div()
                                        .w_full()
                                        .px(px(10.0))
                                        .py(px(6.0))
                                        .rounded(px(12.0))
                                        .bg(theme.primary.opacity(0.07))
                                        .font_family("Ioskeley Mono")
                                        .on_key_down(cx.listener(|this, event: &KeyDownEvent, _window, cx| {
                                            if event.keystroke.key == "escape" {
                                                this.cancel_edit(cx);
                                            }
                                        }))
                                        .child(
                                            Input::new(edit_input)
                                                .appearance(false)
                                                .cleanable(false),
                                        ),
                                )
                                // Action buttons: Cancel + Send
                                .child(
                                    div()
                                        .flex()
                                        .items_center()
                                        .gap(px(6.0))
                                        .child(
                                            div()
                                                .id(ElementId::Name(format!("edit-cancel-{msg_idx}").into()))
                                                .h(px(24.0))
                                                .px(px(10.0))
                                                .flex()
                                                .items_center()
                                                .rounded(px(5.0))
                                                .text_size(px(11.0))
                                                .font_weight(FontWeight::MEDIUM)
                                                .cursor_pointer()
                                                .text_color(theme.muted_foreground)
                                                .hover(|s| s.bg(theme.muted.opacity(0.08)))
                                                .on_mouse_down(
                                                    MouseButton::Left,
                                                    cx.listener(|this, _, _, cx| {
                                                        this.cancel_edit(cx);
                                                    }),
                                                )
                                                .child("Cancel"),
                                        )
                                        .child(
                                            div()
                                                .id(ElementId::Name(format!("edit-submit-{msg_idx}").into()))
                                                .h(px(24.0))
                                                .px(px(10.0))
                                                .flex()
                                                .items_center()
                                                .rounded(px(5.0))
                                                .text_size(px(11.0))
                                                .font_weight(FontWeight::MEDIUM)
                                                .cursor_pointer()
                                                .bg(theme.primary)
                                                .text_color(theme.primary_foreground)
                                                .hover(|s| s.bg(theme.primary_hover))
                                                .on_mouse_down(
                                                    MouseButton::Left,
                                                    cx.listener(|this, _, _, cx| {
                                                        this.submit_edit(cx);
                                                    }),
                                                )
                                                .child("Send"),
                                        ),
                                ),
                        );
                    }
                } else {
                    // ── Normal user message — right-aligned bubble with hover actions ──
                    let user_content: String = msg.content.clone();
                    msg_el = msg_el.child(
                        div()
                            .id(ElementId::Name(format!("user-msg-{msg_idx}").into()))
                            .group("user-msg")
                            .flex()
                            .items_end()
                            .justify_end()
                            .gap(px(4.0))
                            // Action buttons — appear on hover, left of bubble
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .items_center()
                                    .gap(px(2.0))
                                    .mb(px(4.0))
                                    .invisible()
                                    .group_hover("user-msg", |s| s.visible())
                                    // Edit — opens inline editor
                                    .child({
                                        let content_for_edit = user_content.clone();
                                        div()
                                            .id(ElementId::Name(format!("edit-{msg_idx}").into()))
                                            .flex()
                                            .items_center()
                                            .justify_center()
                                            .size(px(22.0))
                                            .rounded(px(4.0))
                                            .cursor_pointer()
                                            .hover(|s| s.bg(theme.muted.opacity(0.12)))
                                            .on_mouse_down(
                                                MouseButton::Left,
                                                cx.listener(move |this, _, window, cx| {
                                                    this.start_editing(msg_idx, &content_for_edit, window, cx);
                                                }),
                                            )
                                            .child(
                                                svg()
                                                    .path("phosphor/pencil-simple.svg")
                                                    .size(px(12.0))
                                                    .text_color(theme.muted_foreground.opacity(0.5)),
                                            )
                                    })
                                    // Rerun — immediate re-send with same content
                                    .child({
                                        let content_for_rerun = user_content.clone();
                                        div()
                                            .id(ElementId::Name(format!("rerun-{msg_idx}").into()))
                                            .flex()
                                            .items_center()
                                            .justify_center()
                                            .size(px(22.0))
                                            .rounded(px(4.0))
                                            .cursor_pointer()
                                            .hover(|s| s.bg(theme.muted.opacity(0.12)))
                                            .on_mouse_down(
                                                MouseButton::Left,
                                                cx.listener(move |this, _, _, cx| {
                                                    this.rerun_from(msg_idx, content_for_rerun.clone(), cx);
                                                }),
                                            )
                                            .child(
                                                svg()
                                                    .path("phosphor/play.svg")
                                                    .size(px(12.0))
                                                    .text_color(theme.muted_foreground.opacity(0.5)),
                                            )
                                    }),
                            )
                            .child(
                                div()
                                    .max_w(rems(22.0))
                                    .px(px(14.0))
                                    .py(px(8.0))
                                    .rounded(px(16.0))
                                    .rounded_tr(px(4.0))
                                    .bg(theme.primary.opacity(0.07))
                                    .child(
                                        div()
                                            .text_size(px(13.5))
                                            .line_height(px(21.0))
                                            .text_color(theme.foreground)
                                            .child(user_content),
                                    ),
                            ),
                    );
                }
            } else {
                // ── Assistant message ──
                let assistant_content_for_copy: String = msg.content.clone();

                // Header row — oven icon + "Con" label
                msg_el = msg_el.child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(6.0))
                        .pb(px(2.0))
                        .child(
                            svg()
                                .path("phosphor/oven.svg")
                                .size(px(15.0))
                                .text_color(theme.primary),
                        )
                        .child(
                            div()
                                .text_size(px(12.5))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(theme.primary)
                                .child("Con"),
                        ),
                );

                // Extended thinking (collapsible)
                if let Some(thinking) = &msg.thinking {
                    if !thinking.is_empty() {
                        let thinking_collapsed = msg.thinking_collapsed;
                        let chevron = if thinking_collapsed {
                            "phosphor/caret-right.svg"
                        } else {
                            "phosphor/caret-down.svg"
                        };
                        let word_count = thinking.split_whitespace().count();

                        msg_el = msg_el.child(
                            div()
                                .id(SharedString::from(format!("thinking-toggle-{msg_idx}")))
                                .flex()
                                .items_center()
                                .gap(px(5.0))
                                .pl(px(20.0))
                                .cursor_pointer()
                                .text_xs()
                                .text_color(theme.muted_foreground)
                                .hover(|s| s.text_color(theme.foreground))
                                .on_mouse_down(
                                    MouseButton::Left,
                                    cx.listener(move |this, _, _, cx| {
                                        if let Some(m) = this.state.messages.get_mut(msg_idx) {
                                            m.thinking_collapsed = !m.thinking_collapsed;
                                        }
                                        cx.notify();
                                    }),
                                )
                                .child(
                                    svg()
                                        .path("phosphor/brain.svg")
                                        .size(px(12.0))
                                        .text_color(theme.primary.opacity(0.6)),
                                )
                                .child(
                                    svg()
                                        .path(chevron)
                                        .size(px(10.0))
                                        .text_color(theme.muted_foreground),
                                )
                                .child(format!("Thinking ({} words)", word_count)),
                        );

                        if !thinking_collapsed {
                            let display_text: SharedString = if thinking.len() > THINKING_DISPLAY_LEN {
                                format!("{}…", &thinking[..thinking.floor_char_boundary(THINKING_DISPLAY_LEN)]).into()
                            } else {
                                thinking.clone().into()
                            };
                            msg_el = msg_el.child(
                                div()
                                    .ml(px(24.0))
                                    .pl(px(12.0))
                                    .border_l_1()
                                    .border_color(theme.primary.opacity(0.15))
                                    .max_h(px(256.0))
                                    .overflow_y_hidden()
                                    .child(
                                        div()
                                            .py(px(4.0))
                                            .text_xs()
                                            .line_height(px(18.0))
                                            .text_color(theme.muted_foreground.opacity(0.65))
                                            .child(
                                                TextView::markdown(
                                                    ElementId::Name(format!("thinking-md-{msg_idx}").into()),
                                                    display_text,
                                                )
                                                .selectable(true)
                                                .style(chat_markdown_style())
                                                .text_xs()
                                            ),
                                    ),
                            );
                        }
                    }
                }

                // Message content — render as markdown
                if !msg.content.is_empty() {
                    let content: SharedString = msg.content.clone().into();
                    msg_el = msg_el.child(
                        div()
                            .pl(px(20.0))
                            .pr(px(8.0))
                            .text_size(px(14.5))
                            .line_height(px(24.0))
                            .text_color(theme.foreground)
                            .child(
                                TextView::markdown(
                                    ElementId::Name(format!("msg-md-{msg_idx}").into()),
                                    content,
                                )
                                .selectable(true)
                                .style(chat_markdown_style())
                                .text_size(px(14.5))
                            ),
                    );

                    // Copy button — subtle, below content
                    let content_for_clip = assistant_content_for_copy;
                    msg_el = msg_el.child(
                        div()
                            .pl(px(20.0))
                            .child(
                                div()
                                    .id(ElementId::Name(format!("copy-asst-{msg_idx}").into()))
                                    .flex()
                                    .items_center()
                                    .gap(px(4.0))
                                    .h(px(20.0))
                                    .px(px(6.0))
                                    .rounded(px(4.0))
                                    .cursor_pointer()
                                    .text_size(px(10.0))
                                    .text_color(theme.muted_foreground.opacity(0.35))
                                    .hover(|s| s.bg(theme.muted.opacity(0.08)).text_color(theme.muted_foreground.opacity(0.7)))
                                    .on_mouse_down(
                                        MouseButton::Left,
                                        cx.listener(move |_this, _, _, cx| {
                                            cx.write_to_clipboard(
                                                ClipboardItem::new_string(content_for_clip.clone()),
                                            );
                                        }),
                                    )
                                    .child(
                                        svg()
                                            .path("phosphor/copy.svg")
                                            .size(px(11.0))
                                            .text_color(theme.muted_foreground.opacity(0.35)),
                                    )
                                    .child("Copy"),
                            ),
                    );
                }
            }

            // ── Steps timeline (collapsible) ──
            if !msg.steps.is_empty() {
                let step_count = msg.steps.len();
                let collapsed = msg.steps_collapsed;
                let chevron = if collapsed {
                    "phosphor/caret-right.svg"
                } else {
                    "phosphor/caret-down.svg"
                };

                // Toggle header
                msg_el = msg_el.child(
                    div()
                        .id(SharedString::from(format!("steps-toggle-{msg_idx}")))
                        .flex()
                        .items_center()
                        .gap(px(5.0))
                        .pl(px(16.0))
                        .py(px(2.0))
                        .cursor_pointer()
                        .text_size(px(11.0))
                        .text_color(theme.muted_foreground.opacity(0.6))
                        .hover(|s| s.text_color(theme.foreground))
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(move |this, _, _, cx| {
                                if let Some(m) = this.state.messages.get_mut(msg_idx) {
                                    m.steps_collapsed = !m.steps_collapsed;
                                }
                                cx.notify();
                            }),
                        )
                        .child(
                            svg()
                                .path(chevron)
                                .size(px(10.0))
                                .text_color(theme.muted_foreground.opacity(0.5)),
                        )
                        .child(format!(
                            "{} step{}",
                            step_count,
                            if step_count == 1 { "" } else { "s" }
                        )),
                );

                if !collapsed {
                    let mut steps_el = div()
                        .flex()
                        .flex_col()
                        .ml(px(20.0))
                        .pl(px(12.0))
                        .border_l_1()
                        .border_color(theme.muted.opacity(0.15));

                    for (step_idx, step) in msg.steps.iter().enumerate() {
                        let status_color = match step.status {
                            StepStatus::Running => theme.warning,
                            StepStatus::Complete => theme.success,
                            StepStatus::Denied => theme.danger,
                        };

                        let has_detail = step.detail.is_some();
                        let detail_collapsed = step.detail_collapsed;

                        // Step header row
                        let mut step_header = div()
                            .flex()
                            .items_center()
                            .gap(px(6.0))
                            .py(px(3.0))
                            .px(px(4.0))
                            // Status dot
                            .child(
                                div()
                                    .size(px(5.0))
                                    .rounded_full()
                                    .flex_shrink_0()
                                    .bg(status_color),
                            )
                            .child(
                                svg()
                                    .path(step.icon)
                                    .size(px(11.0))
                                    .flex_shrink_0()
                                    .text_color(theme.muted_foreground.opacity(0.6)),
                            )
                            .child(
                                div()
                                    .text_size(px(11.0))
                                    .text_color(theme.muted_foreground.opacity(0.75))
                                    .overflow_x_hidden()
                                    .flex_1()
                                    .child(truncate_str(&step.label, 50)),
                            );

                        // Duration badge
                        if let Some(dur) = step.duration {
                            let dur_text = if dur.as_secs() >= 60 {
                                format!("{}m {}s", dur.as_secs() / 60, dur.as_secs() % 60)
                            } else if dur.as_millis() >= 1000 {
                                format!("{:.1}s", dur.as_secs_f64())
                            } else {
                                format!("{}ms", dur.as_millis())
                            };
                            step_header = step_header.child(
                                div()
                                    .flex_shrink_0()
                                    .text_size(px(9.0))
                                    .text_color(theme.muted_foreground.opacity(0.4))
                                    .child(dur_text),
                            );
                        }

                        // Expand/collapse chevron
                        if has_detail {
                            let detail_chevron = if detail_collapsed {
                                "phosphor/caret-right.svg"
                            } else {
                                "phosphor/caret-down.svg"
                            };
                            step_header = step_header.child(
                                svg()
                                    .path(detail_chevron)
                                    .size(px(9.0))
                                    .flex_shrink_0()
                                    .text_color(theme.muted_foreground.opacity(0.4)),
                            );
                        }

                        let mut step_el = div().flex().flex_col();

                        if has_detail {
                            step_el = step_el.child(
                                step_header
                                    .id(SharedString::from(format!("step-detail-{msg_idx}-{step_idx}")))
                                    .cursor_pointer()
                                    .rounded(px(4.0))
                                    .hover(|s| s.bg(theme.muted.opacity(0.06)))
                                    .on_mouse_down(
                                        MouseButton::Left,
                                        cx.listener(move |this, _, _, cx| {
                                            if let Some(m) = this.state.messages.get_mut(msg_idx) {
                                                if let Some(s) = m.steps.get_mut(step_idx) {
                                                    s.detail_collapsed = !s.detail_collapsed;
                                                }
                                            }
                                            cx.notify();
                                        }),
                                    ),
                            );
                        } else {
                            step_el = step_el.child(step_header);
                        }

                        // Expanded detail
                        if let Some(detail) = &step.detail {
                            if !detail_collapsed {
                                let preview = result_preview(detail, TOOL_RESULT_PREVIEW_LINES);
                                let md: SharedString = format!("```\n{preview}\n```").into();
                                step_el = step_el.child(
                                    div()
                                        .ml(px(18.0))
                                        .mr(px(4.0))
                                        .mt(px(2.0))
                                        .mb(px(4.0))
                                        .rounded(px(6.0))
                                        .bg(theme.muted.opacity(0.05))
                                        .overflow_x_hidden()
                                        .child(
                                            div()
                                                .px(px(10.0))
                                                .py(px(6.0))
                                                .child(
                                                    TextView::markdown(
                                                        ElementId::Name(
                                                            format!("step-result-{msg_idx}-{step_idx}").into(),
                                                        ),
                                                        md,
                                                    )
                                                    .selectable(true)
                                                    .style(chat_markdown_style())
                                                    .text_xs()
                                                ),
                                        ),
                                );
                            }
                        }

                        steps_el = steps_el.child(step_el);
                    }
                    msg_el = msg_el.child(steps_el);
                }
            }

            messages_area = messages_area.child(msg_el);
        }

        // ── Active tool calls (live cards) ───────────────────────
        for (tc_idx, tc) in self.state.tool_calls.iter().enumerate() {
            let is_done = tc.result.is_some();
            let icon = tool_icon(&tc.tool_name);
            let args_display = format_tool_args(&tc.tool_name, &tc.args);
            let human_name = humanize_tool_name(&tc.tool_name);

            let status_color = if is_done { theme.success } else { theme.warning };

            let mut tc_el = div()
                .flex()
                .flex_col()
                .gap(px(6.0))
                .px(px(12.0))
                .py(px(10.0))
                .rounded(px(8.0))
                .border_1()
                .border_color(theme.muted.opacity(0.12))
                .bg(theme.muted.opacity(0.04))
                // Header row
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(6.0))
                        // Status + icon combined
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .justify_center()
                                .size(px(20.0))
                                .rounded(px(4.0))
                                .bg(status_color.opacity(0.10))
                                .child(
                                    svg()
                                        .path(icon)
                                        .size(px(12.0))
                                        .text_color(status_color),
                                ),
                        )
                        .child(
                            div()
                                .text_size(px(12.0))
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(theme.foreground.opacity(0.85))
                                .flex_1()
                                .child(human_name),
                        )
                        // Elapsed / duration
                        .child({
                            let dur = tc.duration.unwrap_or_else(|| tc.started_at.elapsed());
                            let dur_text = if dur.as_secs() >= 60 {
                                format!("{}m {}s", dur.as_secs() / 60, dur.as_secs() % 60)
                            } else if dur.as_millis() >= 1000 {
                                format!("{:.1}s", dur.as_secs_f64())
                            } else {
                                format!("{}ms", dur.as_millis())
                            };
                            div()
                                .flex_shrink_0()
                                .text_size(px(10.0))
                                .text_color(theme.muted_foreground.opacity(0.45))
                                .child(dur_text)
                        }),
                )
                // Args — monospace preview
                .child(
                    div()
                        .text_size(px(11.0))
                        .text_color(theme.muted_foreground.opacity(0.7))
                        .overflow_x_hidden()
                        .font_family("Ioskeley Mono")
                        .child(truncate_str(&args_display, 60)),
                );

            // Result — rendered as formatted code block
            if let Some(result) = &tc.result {
                let formatted = format_tool_result(&tc.tool_name, result);
                let preview = result_preview(&formatted, TOOL_RESULT_PREVIEW_LINES);
                let md: SharedString = format!("```\n{preview}\n```").into();
                tc_el = tc_el.child(
                    div()
                        .mt(px(2.0))
                        .rounded(px(6.0))
                        .bg(theme.muted.opacity(0.06))
                        .overflow_x_hidden()
                        .child(
                            div()
                                .px(px(10.0))
                                .py(px(8.0))
                                .child(
                                    TextView::markdown(
                                        ElementId::Name(
                                            format!("tc-result-{tc_idx}").into(),
                                        ),
                                        md,
                                    )
                                    .selectable(true)
                                    .style(chat_markdown_style())
                                    .text_xs()
                                ),
                        ),
                );
            }
            messages_area = messages_area.child(tc_el);
        }

        // ── Pending approvals ───────────────────────────────────
        for (i, approval) in self.state.pending_approvals.iter().enumerate() {
            let icon = tool_icon(&approval.tool_name);
            let args_display = format_tool_args(&approval.tool_name, &approval.args);
            let allow_idx = i;
            let deny_idx = i;
            let human_tool = humanize_tool_name(&approval.tool_name);

            let approval_el = div()
                .flex()
                .flex_col()
                .gap(px(10.0))
                .px(px(12.0))
                .py(px(12.0))
                .rounded(px(8.0))
                .bg(theme.warning.opacity(0.05))
                // Header — icon badge + title
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(8.0))
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .justify_center()
                                .size(px(22.0))
                                .rounded(px(5.0))
                                .bg(theme.warning.opacity(0.12))
                                .child(
                                    svg()
                                        .path("phosphor/warning.svg")
                                        .size(px(13.0))
                                        .text_color(theme.warning),
                                ),
                        )
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .child(
                                    div()
                                        .text_size(px(12.0))
                                        .font_weight(FontWeight::MEDIUM)
                                        .text_color(theme.foreground)
                                        .child(format!("{} requires approval", human_tool)),
                                ),
                        ),
                )
                // Command preview — monospace code block style
                .child(
                    div()
                        .flex()
                        .items_start()
                        .gap(px(6.0))
                        .px(px(8.0))
                        .py(px(8.0))
                        .rounded(px(6.0))
                        .bg(theme.muted.opacity(0.06))
                        .child(
                            svg()
                                .path(icon)
                                .size(px(12.0))
                                .mt(px(1.0))
                                .flex_shrink_0()
                                .text_color(theme.muted_foreground.opacity(0.6)),
                        )
                        .child(
                            div()
                                .text_size(px(11.0))
                                .font_family("Ioskeley Mono")
                                .text_color(theme.foreground.opacity(0.8))
                                .overflow_x_hidden()
                                .child(truncate_str(&args_display, 80)),
                        ),
                )
                // Action buttons — right-aligned, clear hierarchy
                .child(
                    div()
                        .flex()
                        .gap(px(6.0))
                        .justify_end()
                        // Deny — ghost button
                        .child(
                            div()
                                .id(SharedString::from(format!("deny-{i}")))
                                .h(px(28.0))
                                .px(px(12.0))
                                .flex()
                                .items_center()
                                .rounded(px(6.0))
                                .text_size(px(11.0))
                                .font_weight(FontWeight::MEDIUM)
                                .cursor_pointer()
                                .text_color(theme.muted_foreground)
                                .hover(|s| s.bg(theme.danger.opacity(0.08)).text_color(theme.danger))
                                .on_mouse_down(
                                    MouseButton::Left,
                                    cx.listener(move |this, _, _, cx| {
                                        this.resolve_approval(deny_idx, false, cx);
                                    }),
                                )
                                .child("Deny"),
                        )
                        // Allow — secondary
                        .child(
                            div()
                                .id(SharedString::from(format!("allow-{i}")))
                                .h(px(28.0))
                                .px(px(12.0))
                                .flex()
                                .items_center()
                                .rounded(px(6.0))
                                .text_size(px(11.0))
                                .font_weight(FontWeight::MEDIUM)
                                .cursor_pointer()
                                .bg(theme.primary.opacity(0.10))
                                .text_color(theme.primary)
                                .hover(|s| s.bg(theme.primary.opacity(0.18)))
                                .on_mouse_down(
                                    MouseButton::Left,
                                    cx.listener(move |this, _, _, cx| {
                                        this.resolve_approval(allow_idx, true, cx);
                                    }),
                                )
                                .child("Allow"),
                        )
                        // Allow All — primary solid
                        .child(
                            div()
                                .id(SharedString::from(format!("allow-all-{i}")))
                                .h(px(28.0))
                                .px(px(12.0))
                                .flex()
                                .items_center()
                                .rounded(px(6.0))
                                .text_size(px(11.0))
                                .font_weight(FontWeight::MEDIUM)
                                .cursor_pointer()
                                .bg(theme.primary)
                                .text_color(theme.primary_foreground)
                                .hover(|s| s.bg(theme.primary_hover))
                                .on_mouse_down(
                                    MouseButton::Left,
                                    cx.listener(move |this, _, _, cx| {
                                        this.auto_approve = true;
                                        cx.emit(EnableAutoApprove);
                                        this.resolve_all_approvals(cx);
                                    }),
                                )
                                .child("Allow All"),
                        ),
                );

            messages_area = messages_area.child(approval_el);
        }

        // ── Status indicator ──────────────────────────────────────
        if let Some((icon, label)) = self.status_text() {
            let status_color = match self.state.status {
                AgentStatus::Thinking => theme.warning,
                AgentStatus::Responding => theme.success,
                AgentStatus::Idle => theme.muted_foreground,
            };
            messages_area = messages_area.child(
                div()
                    .pl(px(18.0))
                    .flex()
                    .items_center()
                    .gap(px(5.0))
                    .child(
                        svg()
                            .path(icon)
                            .size(px(11.0))
                            .text_color(status_color.opacity(0.6)),
                    )
                    .child(
                        div()
                            .text_size(px(11.0))
                            .text_color(theme.muted_foreground.opacity(0.7))
                            .child(label),
                    ),
            );
        }

        // ── Header ──────────────────────────────────────────────
        let status_dot_color = match self.state.status {
            AgentStatus::Idle => theme.muted_foreground.opacity(0.3),
            AgentStatus::Thinking => theme.warning,
            AgentStatus::Responding => theme.success,
        };

        // Model label — show short name (e.g. "claude-sonnet-4-6" → "Sonnet 4.6")
        let model_display = humanize_model_name(&self.model_name);

        let mut header_left = div()
            .flex()
            .items_center()
            .gap(px(7.0))
            // Oven icon
            .child(
                svg()
                    .path("phosphor/oven.svg")
                    .size(px(14.0))
                    .text_color(theme.primary),
            )
            // Model name
            .child(
                div()
                    .text_size(px(12.0))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(theme.foreground.opacity(0.7))
                    .child(model_display),
            )
            // Status dot — animated when active
            .child(
                div()
                    .size(px(6.0))
                    .rounded_full()
                    .bg(status_dot_color),
            );

        // Auto-approve badge
        if self.auto_approve {
            header_left = header_left.child(
                div()
                    .px(px(5.0))
                    .py(px(1.0))
                    .rounded(px(3.0))
                    .bg(theme.warning.opacity(0.10))
                    .text_color(theme.warning)
                    .text_size(px(9.0))
                    .font_weight(FontWeight::BOLD)
                    .child("AUTO"),
            );
        }

        let header = div()
            .flex()
            .items_center()
            .justify_between()
            .h(px(38.0))
            .px(px(12.0))
            .flex_shrink_0()
            .child(header_left)
            .child({
                let mut actions = div()
                    .flex()
                    .items_center()
                    .gap(px(2.0));

                // Stop button — visible when agent is working
                if self.state.status != AgentStatus::Idle {
                    actions = actions.child(
                        icon_button("agent-stop", "phosphor/stop.svg", theme)
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(|this, _, _, cx| {
                                    cx.emit(CancelRequest);
                                    this.state.status = AgentStatus::Idle;
                                    this.state.streaming = false;
                                    cx.notify();
                                }),
                            ),
                    );
                }

                actions
                    .child(
                        icon_button("agent-history-toggle", "phosphor/clock-counter-clockwise.svg", theme)
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(|this, _, _, cx| {
                                    this.toggle_history(cx);
                                }),
                            ),
                    )
                    .child(
                        icon_button("agent-new-chat", "phosphor/plus.svg", theme)
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(|this, _, _, cx| {
                                    cx.emit(NewConversation);
                                    this.clear_messages(cx);
                                }),
                            ),
                    )
            });

        // ── Panel ───────────────────────────────────────────────
        // Use system proportional font for readable prose — the workspace root
        // sets Ioskeley Mono which would cascade here without this override.
        let mut panel = div()
            .flex()
            .flex_col()
            .size_full()
            .bg(theme.title_bar)
            .font_family(".SystemUIFont")
            .child(header);

        if self.showing_history {
            let mut history = div()
                .id("agent-history-list")
                .flex()
                .flex_col()
                .flex_1()
                .overflow_y_scroll()
                .track_scroll(&self.scroll_handle)
                .px(px(12.0))
                .pt(px(8.0))
                .gap(px(1.0));

            if self.conversation_list.is_empty() {
                history = history.child(
                    div()
                        .text_sm()
                        .text_color(theme.muted_foreground)
                        .p(px(16.0))
                        .child("No saved conversations"),
                );
            } else {
                for (i, summary) in self.conversation_list.iter().enumerate() {
                    let conv_id = summary.id.clone();
                    let date = summary.created_at.format("%b %d, %H:%M").to_string();
                    history = history.child(
                        div()
                            .id(SharedString::from(format!("conv-{i}")))
                            .flex()
                            .items_center()
                            .justify_between()
                            .px(px(12.0))
                            .h(px(40.0))
                            .rounded(px(6.0))
                            .cursor_pointer()
                            .hover(|s| s.bg(theme.muted.opacity(0.08)))
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(move |this, _, _, cx| {
                                    cx.emit(LoadConversation {
                                        id: conv_id.clone(),
                                    });
                                    this.showing_history = false;
                                    cx.notify();
                                }),
                            )
                            .child(
                                div()
                                    .text_sm()
                                    .text_color(theme.foreground)
                                    .overflow_x_hidden()
                                    .flex_1()
                                    .child(truncate_str(&summary.title, 30)),
                            )
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(6.0))
                                    .flex_shrink_0()
                                    .text_xs()
                                    .text_color(theme.muted_foreground)
                                    .child(date)
                                    .child(format!("{}m", summary.message_count)),
                            ),
                    );
                }
            }
            panel = panel.child(history);
        } else {
            panel = panel.child(messages_area);
        }

        panel
    }
}

fn icon_button(
    id: &str,
    icon_path: &str,
    theme: &gpui_component::Theme,
) -> Stateful<Div> {
    let icon_path = SharedString::from(icon_path.to_string());
    div()
        .id(SharedString::from(id.to_string()))
        .flex()
        .items_center()
        .justify_center()
        .size(px(26.0))
        .rounded(px(5.0))
        .cursor_pointer()
        .hover(|s| s.bg(theme.muted.opacity(0.10)))
        .child(
            svg()
                .path(icon_path)
                .size(px(13.0))
                .text_color(theme.muted_foreground),
        )
}

/// Convert model ID to a human-readable short name.
fn humanize_model_name(model: &str) -> String {
    // Common patterns: "claude-sonnet-4-6" → "Sonnet 4.6"
    if model.contains("claude") {
        if let Some(rest) = model.strip_prefix("claude-") {
            // "sonnet-4-6" → "Sonnet 4.6", "opus-4-6" → "Opus 4.6"
            let parts: Vec<&str> = rest.splitn(2, '-').collect();
            if parts.len() >= 2 {
                let family = parts[0];
                let version = parts[1].replace('-', ".");
                let family_cap = format!("{}{}", &family[..1].to_uppercase(), &family[1..]);
                return format!("{} {}", family_cap, version);
            }
        }
    }
    if model.contains("gpt-4") {
        return model.replace("gpt-", "GPT-");
    }
    if model.is_empty() {
        return "No model".to_string();
    }
    // Fallback: show as-is but truncated
    truncate_str(model, 24)
}
