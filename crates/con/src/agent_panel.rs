use crossbeam_channel::Sender;
use gpui::*;
use gpui_component::scroll::ScrollableElement;
use gpui_component::text::TextView;
use gpui_component::ActiveTheme;

/// Max lines to show for tool result previews in collapsed steps
const TOOL_RESULT_PREVIEW_LINES: usize = 6;
/// Max characters to show in expanded thinking section
const THINKING_DISPLAY_LEN: usize = 2000;

use con_agent::{ConversationSummary, ToolApprovalDecision};

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

pub struct AgentPanel {
    messages: Vec<PanelMessage>,
    tool_calls: Vec<ToolCallEntry>,
    pending_approvals: Vec<PendingApproval>,
    streaming: bool,
    status: AgentStatus,
    scroll_handle: ScrollHandle,
    showing_history: bool,
    conversation_list: Vec<ConversationSummary>,
    auto_approve: bool,
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
    pub fn new(_cx: &mut Context<Self>) -> Self {
        Self {
            messages: vec![PanelMessage::new("system", "Ask anything about your terminal, code, or system. The agent can read files, run commands, and search your workspace.")],
            tool_calls: Vec::new(),
            pending_approvals: Vec::new(),
            streaming: false,
            status: AgentStatus::Idle,
            scroll_handle: ScrollHandle::new(),
            showing_history: false,
            conversation_list: Vec::new(),
            auto_approve: false,
        }
    }

    pub fn clear_messages(&mut self, cx: &mut Context<Self>) {
        self.messages = vec![PanelMessage::new("system", "Ask anything about your terminal, code, or system. The agent can read files, run commands, and search your workspace.")];
        self.tool_calls.clear();
        self.pending_approvals.clear();
        self.streaming = false;
        self.status = AgentStatus::Idle;
        self.showing_history = false;
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
        self.streaming = false;
        self.status = AgentStatus::Idle;
        self.messages.push(PanelMessage::new(role, content));
        self.scroll_to_bottom();
        cx.notify();
    }

    pub fn add_step(&mut self, step: &str, cx: &mut Context<Self>) {
        self.status = AgentStatus::Thinking;
        if let Some(last) = self.messages.last_mut() {
            last.steps.push(StepEntry {
                icon: "phosphor/sparkle.svg",
                label: step.to_string(),
                detail: None,
                status: StepStatus::Complete,
                detail_collapsed: true,
            });
        }
        cx.notify();
    }

    pub fn add_tool_call(
        &mut self,
        call_id: &str,
        tool_name: &str,
        args: &str,
        cx: &mut Context<Self>,
    ) {
        self.status = AgentStatus::Thinking;
        self.tool_calls.push(ToolCallEntry {
            call_id: call_id.to_string(),
            tool_name: tool_name.to_string(),
            args: args.to_string(),
            result: None,
        });
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
        if let Some(entry) = self.tool_calls.iter_mut().find(|e| e.call_id == call_id) {
            entry.result = Some(result.to_string());
        }
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
        self.pending_approvals.push(PendingApproval {
            call_id: call_id.to_string(),
            tool_name: tool_name.to_string(),
            args: args.to_string(),
            approval_tx,
        });
        self.scroll_to_bottom();
        cx.notify();
    }

    fn resolve_approval(&mut self, index: usize, allowed: bool, cx: &mut Context<Self>) {
        if index >= self.pending_approvals.len() {
            return;
        }
        let approval = self.pending_approvals.remove(index);
        let _ = approval.approval_tx.send(ToolApprovalDecision {
            call_id: approval.call_id,
            allowed,
            reason: if allowed {
                None
            } else {
                Some("User denied tool execution".to_string())
            },
        });
        if let Some(last) = self.messages.last_mut() {
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
            });
        }
        cx.notify();
    }

    fn resolve_all_approvals(&mut self, cx: &mut Context<Self>) {
        while !self.pending_approvals.is_empty() {
            self.resolve_approval(0, true, cx);
        }
    }

    pub fn update_thinking(&mut self, text: &str, cx: &mut Context<Self>) {
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
        self.scroll_to_bottom();
        cx.notify();
    }

    pub fn update_streaming(&mut self, token: &str, cx: &mut Context<Self>) {
        self.status = AgentStatus::Responding;
        if !self.streaming {
            self.messages.push(PanelMessage::assistant());
            self.streaming = true;
        }
        if let Some(last) = self.messages.last_mut() {
            last.content.push_str(token);
        }
        self.scroll_to_bottom();
        cx.notify();
    }

    pub fn complete_response(&mut self, final_content: &str, cx: &mut Context<Self>) {
        self.status = AgentStatus::Idle;
        if self.streaming {
            if let Some(last) = self.messages.last_mut() {
                last.content = final_content.to_string();
            }
            self.streaming = false;
        } else {
            self.messages
                .push(PanelMessage::new("assistant", final_content));
        }
        // Move active tool calls into the message's step timeline
        for tc in self.tool_calls.drain(..) {
            let args_display = Self::format_tool_args(&tc.tool_name, &tc.args);
            let human_name = humanize_tool_name(&tc.tool_name);
            let (status, detail) = if let Some(result) = &tc.result {
                let formatted = format_tool_result(&result);
                (StepStatus::Complete, Some(formatted))
            } else {
                (StepStatus::Running, None)
            };
            if let Some(last) = self.messages.last_mut() {
                last.steps.push(StepEntry {
                    icon: Self::tool_icon(&tc.tool_name),
                    label: format!("{}: {}", human_name, truncate_str(&args_display, 60)),
                    detail,
                    status,
                    detail_collapsed: true,
                });
            }
        }
        self.scroll_to_bottom();
        cx.notify();
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

    /// Derive a human-readable status line from current panel state.
    fn status_text(&self) -> Option<(&'static str, &'static str)> {
        // Priority: approval > running tool > thinking > responding
        if !self.pending_approvals.is_empty() {
            return Some(("phosphor/warning.svg", "Awaiting approval…"));
        }
        if let Some(tc) = self.tool_calls.iter().find(|tc| tc.result.is_none()) {
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
            return Some((Self::tool_icon(&tc.tool_name), label));
        }
        match self.status {
            AgentStatus::Thinking => Some(("phosphor/sparkle.svg", "Thinking…")),
            AgentStatus::Responding => Some(("phosphor/pencil-simple.svg", "Writing…")),
            AgentStatus::Idle => None,
        }
    }
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

/// Format a tool result for display. Detects JSON and formats it pretty-printed,
/// otherwise returns the raw text with literal `\n` sequences expanded.
fn format_tool_result(raw: &str) -> String {
    // First, try to parse as JSON and pretty-print
    if let Ok(json) = serde_json::from_str::<serde_json::Value>(raw) {
        return serde_json::to_string_pretty(&json).unwrap_or_else(|_| raw.to_string());
    }
    // Expand literal \n sequences that come from serialized strings
    if raw.contains("\\n") {
        raw.replace("\\n", "\n").replace("\\t", "\t")
    } else {
        raw.to_string()
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
            .pb(px(12.0))
            .gap(px(16.0));

        for (msg_idx, msg) in self.messages.iter().enumerate() {
            let is_user = msg.role == "user";
            let is_system = msg.role == "system";

            let mut msg_el = div().flex().flex_col().gap(px(6.0));

            if is_system {
                msg_el = msg_el.child(
                    div()
                        .text_xs()
                        .text_color(theme.muted_foreground)
                        .line_height(px(18.0))
                        .child(msg.content.clone()),
                );
            } else if is_user {
                msg_el = msg_el.child(
                    div()
                        .flex()
                        .justify_end()
                        .child(
                            div()
                                .max_w(rems(18.0))
                                .px(px(12.0))
                                .py(px(8.0))
                                .rounded(px(12.0))
                                .rounded_tr(px(4.0))
                                .bg(theme.primary.opacity(0.15))
                                .child(
                                    div()
                                        .text_sm()
                                        .line_height(px(20.0))
                                        .text_color(theme.foreground)
                                        .child(msg.content.clone()),
                                ),
                        ),
                );
            } else {
                // ── Assistant message ──
                msg_el = msg_el
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(6.0))
                            .child(
                                svg()
                                    .path("phosphor/sparkle.svg")
                                    .size(px(12.0))
                                    .text_color(theme.primary),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(theme.primary)
                                    .child("Agent"),
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
                                .gap(px(4.0))
                                .pl(px(18.0))
                                .cursor_pointer()
                                .text_xs()
                                .text_color(theme.muted_foreground)
                                .hover(|s| s.text_color(theme.foreground))
                                .on_mouse_down(
                                    MouseButton::Left,
                                    cx.listener(move |this, _, _, cx| {
                                        if let Some(m) = this.messages.get_mut(msg_idx) {
                                            m.thinking_collapsed = !m.thinking_collapsed;
                                        }
                                        cx.notify();
                                    }),
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
                                    .pl(px(18.0))
                                    .ml(px(4.0))
                                    .border_l_1()
                                    .border_color(theme.muted.opacity(0.15))
                                    .child(
                                        div()
                                            .pl(px(8.0))
                                            .py(px(4.0))
                                            .text_xs()
                                            .line_height(px(16.0))
                                            .text_color(theme.muted_foreground.opacity(0.7))
                                            .child(
                                                TextView::markdown(
                                                    ElementId::Name(format!("thinking-md-{msg_idx}").into()),
                                                    display_text,
                                                )
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
                            .pl(px(18.0))
                            .text_sm()
                            .child(
                                TextView::markdown(
                                    ElementId::Name(format!("msg-md-{msg_idx}").into()),
                                    content,
                                )
                                .text_sm()
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

                msg_el = msg_el.child(
                    div()
                        .id(SharedString::from(format!("steps-toggle-{msg_idx}")))
                        .flex()
                        .items_center()
                        .gap(px(4.0))
                        .pl(px(18.0))
                        .cursor_pointer()
                        .text_xs()
                        .text_color(theme.muted_foreground)
                        .hover(|s| s.text_color(theme.foreground))
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(move |this, _, _, cx| {
                                if let Some(m) = this.messages.get_mut(msg_idx) {
                                    m.steps_collapsed = !m.steps_collapsed;
                                }
                                cx.notify();
                            }),
                        )
                        .child(
                            svg()
                                .path(chevron)
                                .size(px(10.0))
                                .text_color(theme.muted_foreground),
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
                        .gap(px(2.0))
                        .pl(px(18.0))
                        .ml(px(4.0))
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

                        // Step header row — clickable if has detail
                        let mut step_header = div()
                            .flex()
                            .items_center()
                            .gap(px(5.0))
                            .pl(px(8.0))
                            .py(px(2.0))
                            .child(
                                div()
                                    .size(px(4.0))
                                    .rounded_full()
                                    .bg(status_color),
                            )
                            .child(
                                svg()
                                    .path(step.icon)
                                    .size(px(11.0))
                                    .text_color(theme.muted_foreground),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(theme.muted_foreground)
                                    .overflow_x_hidden()
                                    .child(truncate_str(&step.label, 80)),
                            );

                        // Expand/collapse chevron for details
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
                                    .text_color(theme.muted_foreground.opacity(0.5)),
                            );
                        }

                        let mut step_el = div().flex().flex_col();

                        if has_detail {
                            step_el = step_el.child(
                                step_header
                                    .id(SharedString::from(format!("step-detail-{msg_idx}-{step_idx}")))
                                    .cursor_pointer()
                                    .hover(|s| s.bg(theme.muted.opacity(0.06)))
                                    .rounded(px(4.0))
                                    .on_mouse_down(
                                        MouseButton::Left,
                                        cx.listener(move |this, _, _, cx| {
                                            if let Some(m) = this.messages.get_mut(msg_idx) {
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

                        // Expanded detail — rendered as code block
                        if let Some(detail) = &step.detail {
                            if !detail_collapsed {
                                let preview = result_preview(detail, TOOL_RESULT_PREVIEW_LINES);
                                // Wrap in markdown code fence for syntax-highlighted rendering
                                let is_json = detail.trim_start().starts_with('{')
                                    || detail.trim_start().starts_with('[');
                                let lang = if is_json { "json" } else { "" };
                                let md: SharedString = format!("```{lang}\n{preview}\n```").into();
                                step_el = step_el.child(
                                    div()
                                        .ml(px(22.0))
                                        .mr(px(4.0))
                                        .mt(px(2.0))
                                        .mb(px(4.0))
                                        .rounded(px(6.0))
                                        .bg(theme.muted.opacity(0.06))
                                        .overflow_x_hidden()
                                        .child(
                                            div()
                                                .px(px(8.0))
                                                .py(px(6.0))
                                                .child(
                                                    TextView::markdown(
                                                        ElementId::Name(
                                                            format!("step-result-{msg_idx}-{step_idx}").into(),
                                                        ),
                                                        md,
                                                    )
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
        for (tc_idx, tc) in self.tool_calls.iter().enumerate() {
            let is_done = tc.result.is_some();
            let icon = Self::tool_icon(&tc.tool_name);
            let args_display = Self::format_tool_args(&tc.tool_name, &tc.args);
            let human_name = humanize_tool_name(&tc.tool_name);

            let status_color = if is_done { theme.success } else { theme.warning };

            let mut tc_el = div()
                .flex()
                .flex_col()
                .gap(px(4.0))
                .mx(px(4.0))
                .px(px(12.0))
                .py(px(8.0))
                .rounded(px(8.0))
                .border_1()
                .border_color(status_color.opacity(0.12))
                .bg(theme.muted.opacity(0.04))
                // Header
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(6.0))
                        .child(
                            div()
                                .size(px(5.0))
                                .rounded_full()
                                .bg(status_color),
                        )
                        .child(
                            svg()
                                .path(icon)
                                .size(px(13.0))
                                .text_color(theme.muted_foreground),
                        )
                        .child(
                            div()
                                .text_xs()
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(theme.foreground)
                                .child(human_name),
                        ),
                )
                // Args
                .child(
                    div()
                        .text_xs()
                        .text_color(theme.muted_foreground)
                        .overflow_x_hidden()
                        .font_family("Berkeley Mono")
                        .child(truncate_str(&args_display, 80)),
                );

            // Result — rendered as formatted code block
            if let Some(result) = &tc.result {
                let formatted = format_tool_result(result);
                let preview = result_preview(&formatted, TOOL_RESULT_PREVIEW_LINES);
                let is_json = formatted.trim_start().starts_with('{')
                    || formatted.trim_start().starts_with('[');
                let lang = if is_json { "json" } else { "" };
                let md: SharedString = format!("```{lang}\n{preview}\n```").into();
                tc_el = tc_el.child(
                    div()
                        .mt(px(2.0))
                        .rounded(px(6.0))
                        .bg(theme.muted.opacity(0.06))
                        .overflow_x_hidden()
                        .child(
                            div()
                                .px(px(8.0))
                                .py(px(6.0))
                                .child(
                                    TextView::markdown(
                                        ElementId::Name(
                                            format!("tc-result-{tc_idx}").into(),
                                        ),
                                        md,
                                    )
                                    .text_xs()
                                ),
                        ),
                );
            }
            messages_area = messages_area.child(tc_el);
        }

        // ── Pending approvals ───────────────────────────────────
        for (i, approval) in self.pending_approvals.iter().enumerate() {
            let icon = Self::tool_icon(&approval.tool_name);
            let args_display = Self::format_tool_args(&approval.tool_name, &approval.args);
            let allow_idx = i;
            let deny_idx = i;

            let approval_el = div()
                .flex()
                .flex_col()
                .gap(px(8.0))
                .mx(px(4.0))
                .px(px(12.0))
                .py(px(10.0))
                .rounded(px(8.0))
                .border_1()
                .border_color(theme.warning.opacity(0.2))
                .bg(theme.warning.opacity(0.04))
                // Header
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(6.0))
                        .child(
                            svg()
                                .path("phosphor/warning.svg")
                                .size(px(14.0))
                                .text_color(theme.warning),
                        )
                        .child(
                            div()
                                .text_xs()
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(theme.foreground)
                                .child(format!("{} requires approval", humanize_tool_name(&approval.tool_name))),
                        ),
                )
                // Command preview
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(6.0))
                        .child(
                            svg()
                                .path(icon)
                                .size(px(12.0))
                                .text_color(theme.muted_foreground),
                        )
                        .child(
                            div()
                                .text_xs()
                                .font_family("Berkeley Mono")
                                .text_color(theme.foreground)
                                .overflow_x_hidden()
                                .child(truncate_str(&args_display, 80)),
                        ),
                )
                // Actions: Deny | Allow | Allow All
                .child(
                    div()
                        .flex()
                        .gap(px(6.0))
                        .justify_end()
                        // Deny
                        .child(
                            div()
                                .id(SharedString::from(format!("deny-{i}")))
                                .h(px(26.0))
                                .px(px(10.0))
                                .flex()
                                .items_center()
                                .rounded(px(6.0))
                                .text_xs()
                                .font_weight(FontWeight::MEDIUM)
                                .cursor_pointer()
                                .bg(theme.muted.opacity(0.12))
                                .text_color(theme.muted_foreground)
                                .hover(|s| s.bg(theme.danger.opacity(0.15)).text_color(theme.danger))
                                .on_mouse_down(
                                    MouseButton::Left,
                                    cx.listener(move |this, _, _, cx| {
                                        this.resolve_approval(deny_idx, false, cx);
                                    }),
                                )
                                .child("Deny"),
                        )
                        // Allow
                        .child(
                            div()
                                .id(SharedString::from(format!("allow-{i}")))
                                .h(px(26.0))
                                .px(px(10.0))
                                .flex()
                                .items_center()
                                .rounded(px(6.0))
                                .text_xs()
                                .font_weight(FontWeight::MEDIUM)
                                .cursor_pointer()
                                .bg(theme.primary.opacity(0.15))
                                .text_color(theme.primary)
                                .hover(|s| s.bg(theme.primary.opacity(0.25)))
                                .on_mouse_down(
                                    MouseButton::Left,
                                    cx.listener(move |this, _, _, cx| {
                                        this.resolve_approval(allow_idx, true, cx);
                                    }),
                                )
                                .child("Allow"),
                        )
                        // Allow All (YOLO)
                        .child(
                            div()
                                .id(SharedString::from(format!("allow-all-{i}")))
                                .h(px(26.0))
                                .px(px(10.0))
                                .flex()
                                .items_center()
                                .rounded(px(6.0))
                                .text_xs()
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

        // ── Status indicator (replaces "typing…") ───────────────
        if let Some((icon, label)) = self.status_text() {
            let status_color = match self.status {
                AgentStatus::Thinking => theme.warning,
                AgentStatus::Responding => theme.success,
                AgentStatus::Idle => theme.muted_foreground,
            };
            messages_area = messages_area.child(
                div()
                    .pl(px(18.0))
                    .flex()
                    .items_center()
                    .gap(px(6.0))
                    .child(
                        svg()
                            .path(icon)
                            .size(px(12.0))
                            .text_color(status_color.opacity(0.7)),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(theme.muted_foreground)
                            .child(label),
                    ),
            );
        }

        // ── Header ──────────────────────────────────────────────
        let status_dot_color = match self.status {
            AgentStatus::Idle => theme.muted_foreground.opacity(0.4),
            AgentStatus::Thinking => theme.warning,
            AgentStatus::Responding => theme.success,
        };

        let mut header_left = div()
            .flex()
            .items_center()
            .gap(px(8.0))
            .child(
                div()
                    .text_sm()
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(theme.foreground)
                    .child("Agent"),
            )
            .child(
                div()
                    .size(px(6.0))
                    .rounded_full()
                    .bg(status_dot_color),
            );

        // Show auto-approve badge when YOLO mode is active
        if self.auto_approve {
            header_left = header_left.child(
                div()
                    .px(px(6.0))
                    .py(px(1.0))
                    .rounded(px(4.0))
                    .bg(theme.warning.opacity(0.15))
                    .text_color(theme.warning)
                    .text_xs()
                    .font_weight(FontWeight::MEDIUM)
                    .child("YOLO"),
            );
        }

        let header = div()
            .flex()
            .items_center()
            .justify_between()
            .h(px(40.0))
            .px(px(16.0))
            .border_b_1()
            .border_color(theme.border)
            .flex_shrink_0()
            .child(header_left)
            .child({
                let mut actions = div()
                    .flex()
                    .items_center()
                    .gap(px(2.0));

                // Stop button — visible when agent is working
                if self.status != AgentStatus::Idle {
                    actions = actions.child(
                        icon_button("agent-stop", "phosphor/stop.svg", theme)
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(|this, _, _, cx| {
                                    cx.emit(CancelRequest);
                                    this.status = AgentStatus::Idle;
                                    this.streaming = false;
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
        let mut panel = div()
            .flex()
            .flex_col()
            .size_full()
            .bg(theme.title_bar)
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
        .size(px(28.0))
        .rounded(px(6.0))
        .cursor_pointer()
        .hover(|s| s.bg(theme.muted.opacity(0.12)))
        .child(
            svg()
                .path(icon_path)
                .size(px(14.0))
                .text_color(theme.muted_foreground),
        )
}
