use crossbeam_channel::Sender;
use gpui::*;
use gpui_component::scroll::ScrollableElement;
use gpui_component::ActiveTheme;

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

pub struct AgentPanel {
    messages: Vec<PanelMessage>,
    tool_calls: Vec<ToolCallEntry>,
    pending_approvals: Vec<PendingApproval>,
    streaming: bool,
    status: AgentStatus,
    scroll_handle: ScrollHandle,
    showing_history: bool,
    conversation_list: Vec<ConversationSummary>,
}

struct PanelMessage {
    role: String,
    content: String,
    /// Extended thinking/reasoning text from the model (collapsible)
    thinking: Option<String>,
    thinking_collapsed: bool,
    steps: Vec<String>,
    steps_collapsed: bool,
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
            last.steps.push(step.to_string());
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
        let action = if allowed { "Allowed" } else { "Denied" };
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
            last.steps
                .push(format!("{}: {}", action, approval.tool_name));
        }
        cx.notify();
    }

    /// Accumulate extended thinking/reasoning text into the current message.
    /// Creates an assistant message if one doesn't exist yet (thinking arrives
    /// before text tokens).
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
        for tc in self.tool_calls.drain(..) {
            let step = if let Some(result) = &tc.result {
                let truncated = truncate_str(result, 200);
                format!("[{}] {} → {}", tc.tool_name, tc.args, truncated)
            } else {
                format!("[{}] {}", tc.tool_name, tc.args)
            };
            if let Some(last) = self.messages.last_mut() {
                last.steps.push(step);
            }
        }
        self.scroll_to_bottom();
        cx.notify();
    }

    fn tool_icon(tool_name: &str) -> &'static str {
        match tool_name {
            "terminal_exec" => "phosphor/play.svg",
            "shell_exec" => "phosphor/terminal.svg",
            "file_write" => "phosphor/pencil-simple.svg",
            "file_read" => "phosphor/file-code.svg",
            "edit_file" => "phosphor/pencil-simple.svg",
            "list_files" => "phosphor/folder.svg",
            "search" => "phosphor/magnifying-glass.svg",
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
                _ => {}
            }
        }
        truncate_str(args, 120)
    }
}

fn render_message_content(content: &str, theme: &gpui_component::theme::Theme) -> Div {
    let parts: Vec<&str> = content.split("```").collect();
    let mut container = div().flex().flex_col().gap(px(6.0));

    for (i, part) in parts.iter().enumerate() {
        let is_code = i % 2 == 1;
        if part.is_empty() {
            continue;
        }
        if is_code {
            let code = if let Some(newline_pos) = part.find('\n') {
                &part[newline_pos + 1..]
            } else {
                part
            };
            container = container.child(
                div()
                    .px(px(10.0))
                    .py(px(8.0))
                    .rounded(px(6.0))
                    .bg(theme.background)
                    .font_family("Ioskeley Mono")
                    .text_xs()
                    .text_color(theme.foreground)
                    .child(code.trim_end().to_string()),
            );
        } else {
            for line in part.split('\n') {
                if line.trim().is_empty() {
                    container = container.child(div().h(px(6.0)));
                } else {
                    container = container.child(
                        div()
                            .text_sm()
                            .line_height(px(20.0))
                            .text_color(theme.foreground)
                            .child(line.to_string()),
                    );
                }
            }
        }
    }
    container
}

fn truncate_str(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else {
        format!("{}…", &s[..s.floor_char_boundary(max_len)])
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
                // System message: subtle, centered
                msg_el = msg_el.child(
                    div()
                        .text_xs()
                        .text_color(theme.muted_foreground)
                        .line_height(px(18.0))
                        .child(msg.content.clone()),
                );
            } else if is_user {
                // User message: right-aligned bubble
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
                // Assistant message
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

                // Extended thinking (collapsible, dimmed)
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
                            // Truncate display to first 2000 chars for render performance
                            let display_text = if thinking.len() > 2000 {
                                format!("{}…", &thinking[..thinking.floor_char_boundary(2000)])
                            } else {
                                thinking.clone()
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
                                            .child(display_text),
                                    ),
                            );
                        }
                    }
                }

                msg_el = msg_el
                    .child(
                        div()
                            .pl(px(18.0))
                            .child(render_message_content(&msg.content, theme)),
                    );
            }

            // Steps (collapsible)
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

                    for step in &msg.steps {
                        steps_el = steps_el.child(
                            div()
                                .pl(px(8.0))
                                .py(px(2.0))
                                .text_xs()
                                .text_color(theme.muted_foreground)
                                .child(truncate_str(step, 100)),
                        );
                    }
                    msg_el = msg_el.child(steps_el);
                }
            }

            messages_area = messages_area.child(msg_el);
        }

        // ── Active tool calls ─────────────────────────────────────
        for tc in &self.tool_calls {
            let is_done = tc.result.is_some();
            let icon = Self::tool_icon(&tc.tool_name);
            let args_display = Self::format_tool_args(&tc.tool_name, &tc.args);

            let mut tc_el = div()
                .flex()
                .flex_col()
                .gap(px(6.0))
                .mx(px(4.0))
                .px(px(12.0))
                .py(px(10.0))
                .rounded(px(8.0))
                .bg(theme.muted.opacity(0.06))
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
                                .bg(if is_done { theme.success } else { theme.warning }),
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
                                .child(tc.tool_name.clone()),
                        ),
                )
                // Args
                .child(
                    div()
                        .text_xs()
                        .text_color(theme.muted_foreground)
                        .overflow_x_hidden()
                        .font_family("Ioskeley Mono")
                        .child(truncate_str(&args_display, 80)),
                );

            if let Some(result) = &tc.result {
                tc_el = tc_el.child(
                    div()
                        .pt(px(4.0))
                        .text_xs()
                        .text_color(theme.success.opacity(0.8))
                        .child(truncate_str(result, 120)),
                );
            }
            messages_area = messages_area.child(tc_el);
        }

        // ── Pending approvals ─────────────────────────────────────
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
                .bg(theme.warning.opacity(0.06))
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
                                .child(format!("{} requires approval", approval.tool_name)),
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
                                .font_family("Ioskeley Mono")
                                .text_color(theme.foreground)
                                .overflow_x_hidden()
                                .child(truncate_str(&args_display, 80)),
                        ),
                )
                // Actions
                .child(
                    div()
                        .flex()
                        .gap(px(6.0))
                        .justify_end()
                        .child(
                            div()
                                .id(SharedString::from(format!("deny-{i}")))
                                .h(px(28.0))
                                .px(px(12.0))
                                .flex()
                                .items_center()
                                .rounded(px(6.0))
                                .text_xs()
                                .font_weight(FontWeight::MEDIUM)
                                .cursor_pointer()
                                .bg(theme.muted.opacity(0.12))
                                .text_color(theme.muted_foreground)
                                .hover(|s| s.bg(theme.muted.opacity(0.2)))
                                .on_mouse_down(
                                    MouseButton::Left,
                                    cx.listener(move |this, _, _, cx| {
                                        this.resolve_approval(deny_idx, false, cx);
                                    }),
                                )
                                .child("Deny"),
                        )
                        .child(
                            div()
                                .id(SharedString::from(format!("allow-{i}")))
                                .h(px(28.0))
                                .px(px(12.0))
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
                                        this.resolve_approval(allow_idx, true, cx);
                                    }),
                                )
                                .child("Allow"),
                        ),
                );

            messages_area = messages_area.child(approval_el);
        }

        // Streaming indicator
        if self.streaming {
            messages_area = messages_area.child(
                div()
                    .pl(px(18.0))
                    .flex()
                    .items_center()
                    .gap(px(6.0))
                    .child(
                        div()
                            .size(px(4.0))
                            .rounded_full()
                            .bg(theme.primary.opacity(0.6)),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(theme.muted_foreground)
                            .child("typing…"),
                    ),
            );
        }

        // ── Header ────────────────────────────────────────────────
        let status_dot_color = match self.status {
            AgentStatus::Idle => theme.muted_foreground.opacity(0.4),
            AgentStatus::Thinking => theme.warning,
            AgentStatus::Responding => theme.success,
        };

        let header = div()
            .flex()
            .items_center()
            .justify_between()
            .h(px(40.0))
            .px(px(16.0))
            .border_b_1()
            .border_color(theme.border)
            .flex_shrink_0()
            // Left: title + status
            .child(
                div()
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
                    ),
            )
            // Right: actions
            .child({
                let mut actions = div()
                    .flex()
                    .items_center()
                    .gap(px(2.0));

                // Stop button — only visible when agent is working
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

        // ── Panel ─────────────────────────────────────────────────
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
