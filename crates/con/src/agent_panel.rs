use gpui::*;
use gpui_component::ActiveTheme;

/// Agent status for header indicator
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum AgentStatus {
    Idle,
    Thinking,
    Streaming,
}

/// The agent panel — shows conversation, reasoning steps, tool calls
pub struct AgentPanel {
    messages: Vec<PanelMessage>,
    streaming: bool,
    status: AgentStatus,
}

struct PanelMessage {
    role: String,
    content: String,
    steps: Vec<String>,
}

impl AgentPanel {
    pub fn new(_cx: &mut Context<Self>) -> Self {
        Self {
            messages: vec![PanelMessage {
                role: "system".to_string(),
                content: "con agent ready. Press Cmd+L to toggle this panel.".to_string(),
                steps: Vec::new(),
            }],
            streaming: false,
            status: AgentStatus::Idle,
        }
    }

    pub fn add_message(&mut self, role: &str, content: &str, cx: &mut Context<Self>) {
        self.streaming = false;
        self.status = AgentStatus::Idle;
        self.messages.push(PanelMessage {
            role: role.to_string(),
            content: content.to_string(),
            steps: Vec::new(),
        });
        cx.notify();
    }

    pub fn add_step(&mut self, step: &str, cx: &mut Context<Self>) {
        self.status = AgentStatus::Thinking;
        if let Some(last) = self.messages.last_mut() {
            last.steps.push(step.to_string());
        }
        cx.notify();
    }

    pub fn update_streaming(&mut self, token: &str, cx: &mut Context<Self>) {
        self.status = AgentStatus::Streaming;
        if !self.streaming {
            self.messages.push(PanelMessage {
                role: "assistant".to_string(),
                content: String::new(),
                steps: Vec::new(),
            });
            self.streaming = true;
        }
        if let Some(last) = self.messages.last_mut() {
            last.content.push_str(token);
        }
        cx.notify();
    }

    pub fn complete_streaming(&mut self, final_content: &str, cx: &mut Context<Self>) {
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
            });
        }
        cx.notify();
    }
}

impl Render for AgentPanel {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();

        let mut messages_container = div().flex().flex_col().flex_1().p(px(12.0)).gap(px(12.0));

        for msg in &self.messages {
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
                .child(
                    div()
                        .text_sm()
                        .text_color(theme.foreground)
                        .child(msg.content.clone()),
                );

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

            messages_container = messages_container.child(msg_div);
        }

        if self.streaming {
            messages_container = messages_container.child(
                div()
                    .text_xs()
                    .text_color(theme.muted_foreground)
                    .child("..."),
            );
        }

        // Status indicator color
        let status_color = match self.status {
            AgentStatus::Idle => theme.muted_foreground,
            AgentStatus::Thinking => theme.warning,
            AgentStatus::Streaming => theme.success,
        };

        let status_label = match self.status {
            AgentStatus::Idle => "Idle",
            AgentStatus::Thinking => "Thinking",
            AgentStatus::Streaming => "Streaming",
        };

        div()
            .flex()
            .flex_col()
            .size_full()
            .bg(theme.title_bar)
            // Header
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
                            // Robot icon
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
                    // Status indicator
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(6.0))
                            .child(
                                div()
                                    .size(px(6.0))
                                    .rounded_full()
                                    .bg(status_color),
                            )
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
