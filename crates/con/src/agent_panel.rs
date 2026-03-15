use crossbeam_channel::Sender;
use gpui::*;
use gpui_component::scroll::ScrollableElement;
use gpui_component::ActiveTheme;

use con_agent::ToolApprovalDecision;

/// Agent status for header indicator
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum AgentStatus {
    Idle,
    Thinking,
    Responding,
}

/// A tool call being tracked in the panel
struct ToolCallEntry {
    call_id: String,
    tool_name: String,
    args: String,
    result: Option<String>,
}

/// A dangerous tool call awaiting user approval
struct PendingApproval {
    call_id: String,
    tool_name: String,
    args: String,
    approval_tx: Sender<ToolApprovalDecision>,
}

/// The agent panel — shows conversation, reasoning steps, tool calls, approvals
pub struct AgentPanel {
    messages: Vec<PanelMessage>,
    tool_calls: Vec<ToolCallEntry>,
    pending_approvals: Vec<PendingApproval>,
    streaming: bool,
    status: AgentStatus,
    scroll_handle: ScrollHandle,
}

struct PanelMessage {
    role: String,
    content: String,
    steps: Vec<String>,
    steps_collapsed: bool,
}

impl AgentPanel {
    pub fn new(_cx: &mut Context<Self>) -> Self {
        Self {
            messages: vec![PanelMessage {
                role: "system".to_string(),
                content: "con agent ready. Press Cmd+L to toggle this panel.".to_string(),
                steps: Vec::new(),
                steps_collapsed: false,
            }],
            tool_calls: Vec::new(),
            pending_approvals: Vec::new(),
            streaming: false,
            status: AgentStatus::Idle,
            scroll_handle: ScrollHandle::new(),
        }
    }

    fn scroll_to_bottom(&self) {
        self.scroll_handle.scroll_to_bottom();
    }

    pub fn add_message(&mut self, role: &str, content: &str, cx: &mut Context<Self>) {
        self.streaming = false;
        self.status = AgentStatus::Idle;
        self.messages.push(PanelMessage {
            role: role.to_string(),
            content: content.to_string(),
            steps: Vec::new(),
            steps_collapsed: false,
        });
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

    /// Queue a dangerous tool call for user approval
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

    /// Append streaming text tokens (only fires when using Rig's streaming API)
    pub fn update_streaming(&mut self, token: &str, cx: &mut Context<Self>) {
        self.status = AgentStatus::Responding;
        if !self.streaming {
            self.messages.push(PanelMessage {
                role: "assistant".to_string(),
                content: String::new(),
                steps: Vec::new(),
                steps_collapsed: false,
            });
            self.streaming = true;
        }
        if let Some(last) = self.messages.last_mut() {
            last.content.push_str(token);
        }
        self.scroll_to_bottom();
        cx.notify();
    }

    /// Agent finished — show the final response and fold tool calls into steps
    pub fn complete_response(&mut self, final_content: &str, cx: &mut Context<Self>) {
        self.status = AgentStatus::Idle;
        if self.streaming {
            if let Some(last) = self.messages.last_mut() {
                last.content = final_content.to_string();
            }
            self.streaming = false;
        } else {
            self.messages.push(PanelMessage {
                role: "assistant".to_string(),
                content: final_content.to_string(),
                steps: Vec::new(),
                steps_collapsed: false,
            });
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

    fn render_tool_icon(tool_name: &str) -> &'static str {
        match tool_name {
            "shell_exec" => "phosphor/terminal.svg",
            "file_write" => "phosphor/pencil-simple.svg",
            "file_read" => "phosphor/file-code.svg",
            "search" => "phosphor/magnifying-glass.svg",
            _ => "phosphor/gear.svg",
        }
    }

    fn format_tool_args(tool_name: &str, args: &str) -> String {
        // Try to extract the most useful field from JSON args
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(args) {
            match tool_name {
                "shell_exec" => {
                    if let Some(cmd) = v.get("command").and_then(|c| c.as_str()) {
                        return cmd.to_string();
                    }
                }
                "file_read" | "file_write" => {
                    if let Some(path) = v.get("path").and_then(|p| p.as_str()) {
                        return path.to_string();
                    }
                }
                "search" => {
                    if let Some(query) = v.get("query").and_then(|q| q.as_str()) {
                        return format!("\"{}\"", query);
                    }
                }
                _ => {}
            }
        }
        truncate_str(args, 120)
    }
}

/// Render message content with basic code block support.
/// Splits on ``` fences and renders code blocks with monospace background.
fn render_message_content(content: &str, theme: &gpui_component::theme::Theme) -> Div {
    let parts: Vec<&str> = content.split("```").collect();
    let mut container = div().flex().flex_col().gap(px(4.0));

    for (i, part) in parts.iter().enumerate() {
        let is_code = i % 2 == 1; // odd indices are inside ``` fences
        if part.is_empty() {
            continue;
        }
        if is_code {
            // Strip optional language tag on first line
            let code = if let Some(newline_pos) = part.find('\n') {
                &part[newline_pos + 1..]
            } else {
                part
            };
            container = container.child(
                div()
                    .p(px(8.0))
                    .rounded(px(6.0))
                    .bg(theme.background)
                    .border_1()
                    .border_color(theme.border)
                    .font_family("Ioskeley Mono")
                    .text_xs()
                    .text_color(theme.foreground)
                    .child(code.trim_end().to_string()),
            );
        } else {
            container = container.child(
                div()
                    .text_sm()
                    .text_color(theme.foreground)
                    .child(part.to_string()),
            );
        }
    }

    container
}

/// Truncate a string to max_len characters, adding "..." if truncated.
/// Handles multi-byte characters correctly by using char boundaries.
fn truncate_str(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        return s.to_string();
    }
    let mut end = max_len;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}...", &s[..end])
}

impl Render for AgentPanel {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();

        let mut messages_container = div()
            .id("agent-messages")
            .flex()
            .flex_col()
            .flex_1()
            .overflow_y_scroll()
            .track_scroll(&self.scroll_handle)
            .vertical_scrollbar(&self.scroll_handle)
            .p(px(12.0))
            .gap(px(12.0));

        for (msg_idx, msg) in self.messages.iter().enumerate() {
            let (role_color, role_label) = match msg.role.as_str() {
                "user" => (theme.primary, "You"),
                "assistant" => (theme.success, "Agent"),
                _ => (theme.muted_foreground, "System"),
            };

            let mut msg_div = div()
                .flex()
                .flex_col()
                .gap(px(4.0))
                .child(
                    div()
                        .text_xs()
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(role_color)
                        .child(role_label.to_string()),
                )
                .child(render_message_content(&msg.content, theme));

            if !msg.steps.is_empty() {
                let step_count = msg.steps.len();
                let collapsed = msg.steps_collapsed;
                let chevron = if collapsed { "▸" } else { "▾" };

                msg_div = msg_div.child(
                    div()
                        .id(SharedString::from(format!("steps-toggle-{msg_idx}")))
                        .flex()
                        .items_center()
                        .gap(px(4.0))
                        .ml(px(8.0))
                        .cursor_pointer()
                        .text_xs()
                        .text_color(theme.muted_foreground)
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(move |this, _, _, cx| {
                                if let Some(m) = this.messages.get_mut(msg_idx) {
                                    m.steps_collapsed = !m.steps_collapsed;
                                }
                                cx.notify();
                            }),
                        )
                        .child(chevron)
                        .child(format!(
                            "{} step{}",
                            step_count,
                            if step_count == 1 { "" } else { "s" }
                        )),
                );

                if !collapsed {
                    for step in &msg.steps {
                        msg_div = msg_div.child(
                            div()
                                .ml(px(8.0))
                                .pl(px(8.0))
                                .border_l_2()
                                .border_color(theme.secondary)
                                .text_xs()
                                .text_color(theme.muted_foreground)
                                .child(step.clone()),
                        );
                    }
                }
            }

            messages_container = messages_container.child(msg_div);
        }

        // Render active tool calls as structured cards
        for tc in &self.tool_calls {
            let is_complete = tc.result.is_some();
            let status_color = if is_complete {
                theme.success
            } else {
                theme.warning
            };
            let icon_path = Self::render_tool_icon(&tc.tool_name);
            let display_args = Self::format_tool_args(&tc.tool_name, &tc.args);

            let mut tc_card = div()
                .flex()
                .flex_col()
                .gap(px(4.0))
                .mx(px(4.0))
                .p(px(10.0))
                .rounded(px(8.0))
                .border_1()
                .border_color(theme.border)
                .bg(theme.background)
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(6.0))
                        .child(div().size(px(6.0)).rounded_full().bg(status_color))
                        .child(
                            svg()
                                .path(icon_path)
                                .size(px(14.0))
                                .text_color(theme.muted_foreground),
                        )
                        .child(
                            div()
                                .text_xs()
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(theme.foreground)
                                .child(tc.tool_name.clone()),
                        ),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(theme.muted_foreground)
                        .overflow_x_hidden()
                        .child(display_args),
                );

            if let Some(result) = &tc.result {
                tc_card = tc_card.child(
                    div()
                        .mt(px(2.0))
                        .pt(px(4.0))
                        .border_t_1()
                        .border_color(theme.border)
                        .text_xs()
                        .text_color(theme.success)
                        .child(truncate_str(result, 200)),
                );
            }

            messages_container = messages_container.child(tc_card);
        }

        // Render pending approval cards
        for (i, approval) in self.pending_approvals.iter().enumerate() {
            let icon_path = Self::render_tool_icon(&approval.tool_name);
            let display_args = Self::format_tool_args(&approval.tool_name, &approval.args);
            let allow_idx = i;
            let deny_idx = i;

            let approval_card = div()
                .flex()
                .flex_col()
                .gap(px(8.0))
                .mx(px(4.0))
                .p(px(12.0))
                .rounded(px(10.0))
                .border_1()
                .border_color(theme.warning)
                .bg(theme.background)
                // Header
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(6.0))
                        .child(div().size(px(6.0)).rounded_full().bg(theme.warning))
                        .child(
                            svg()
                                .path(icon_path)
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
                // Args display
                .child(
                    div()
                        .p(px(8.0))
                        .rounded(px(6.0))
                        .bg(theme.title_bar)
                        .text_xs()
                        .text_color(theme.foreground)
                        .overflow_x_hidden()
                        .child(display_args),
                )
                // Action buttons
                .child(
                    div()
                        .flex()
                        .gap(px(8.0))
                        .justify_end()
                        .child(
                            div()
                                .id(SharedString::from(format!("deny-{}", i)))
                                .px(px(12.0))
                                .py(px(5.0))
                                .rounded(px(6.0))
                                .text_xs()
                                .font_weight(FontWeight::MEDIUM)
                                .cursor_pointer()
                                .bg(theme.secondary)
                                .text_color(theme.secondary_foreground)
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
                                .id(SharedString::from(format!("allow-{}", i)))
                                .px(px(12.0))
                                .py(px(5.0))
                                .rounded(px(6.0))
                                .text_xs()
                                .font_weight(FontWeight::MEDIUM)
                                .cursor_pointer()
                                .bg(theme.primary)
                                .text_color(theme.primary_foreground)
                                .on_mouse_down(
                                    MouseButton::Left,
                                    cx.listener(move |this, _, _, cx| {
                                        this.resolve_approval(allow_idx, true, cx);
                                    }),
                                )
                                .child("Allow"),
                        ),
                );

            messages_container = messages_container.child(approval_card);
        }

        if self.streaming {
            messages_container = messages_container.child(
                div()
                    .text_xs()
                    .text_color(theme.muted_foreground)
                    .child("..."),
            );
        }

        let status_color = match self.status {
            AgentStatus::Idle => theme.muted_foreground,
            AgentStatus::Thinking => theme.warning,
            AgentStatus::Responding => theme.success,
        };

        let status_label = match self.status {
            AgentStatus::Idle => "Idle",
            AgentStatus::Thinking => "Thinking",
            AgentStatus::Responding => "Responding",
        };

        div()
            .flex()
            .flex_col()
            .size_full()
            .bg(theme.title_bar)
            .child(
                div()
                    .flex()
                    .h(px(38.0))
                    .px(px(16.0))
                    .items_center()
                    .justify_between()
                    .border_b_1()
                    .border_color(theme.border)
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.0))
                            .child(
                                svg()
                                    .path("phosphor/robot.svg")
                                    .size(px(16.0))
                                    .text_color(theme.foreground),
                            )
                            .child(
                                div()
                                    .text_sm()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(theme.foreground)
                                    .child("Agent"),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(6.0))
                            .child(div().size(px(6.0)).rounded_full().bg(status_color))
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(theme.muted_foreground)
                                    .child(status_label),
                            ),
                    ),
            )
            .child(messages_container)
    }
}
